//! Anker SOLIX Smart Plug Gen 2 and Smart Meter Gen 2, through their local Modbus TCP (port 502),
//! which the Anker app turns on: device › settings › « Three-Party Control Settings » › Modbus TCP.
//! No password; the registers are those of Anker's own integration (`ha-anker-solix-official`).
//!
//! Input registers (function 04), big-endian, 32-bit values high word first. The product number
//! at 32768 (5 registers, text) tells which device answers: `A17X8` a plug, `AE1X0` a meter.
//!
//! - Plug: state 30029 (0 off, 1 on), power 30030 (W ÷10), voltage 30031 (V ÷10), current 30032
//!   (A ÷100), energy 30033 (kWh ÷1000); switched by writing 30047 (function 06, 0 or 1).
//! - Meter: type 10630 (1 single phase, 2 three phase), power 10638–10644 (W), energies 10650–10664
//!   (kWh ÷10, forward and reverse), the second set of clamps (CT 2) at 10666–10694.
//!
//! The SolarBank 2 and the Smart Meter Gen 1 (`A17X7`, no Modbus) have no local access: they go
//! through Anker's cloud, in a module of its own (`anker_cloud`, which says what it reads of each).

use breezone_module_sdk::{device, host, json, module, Described, DeviceInfo, Driver, Error, Found, LengthPrefix, Map, ReadUntil, Result, State, Value};
use std::cell::{Cell, RefCell};

pub const PORT: u16 = 502;
const UNIT: u8 = 1;
const TIMEOUT_MS: u32 = 5000;
const PN: u16 = 32768;

pub const PLUG_PN: &str = "A17X8";
pub const METER_PN: &str = "AE1X0";

// ---------------------------------------------------------------------------------------
// Modbus TCP
// ---------------------------------------------------------------------------------------

/// `transaction · protocol 0 · length · unit · PDU`.
fn frame(transaction: u16, pdu: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(pdu.len() + 7);
    out.extend_from_slice(&transaction.to_be_bytes());
    out.extend_from_slice(&[0, 0]);
    out.extend_from_slice(&(pdu.len() as u16 + 1).to_be_bytes());
    out.push(UNIT);
    out.extend_from_slice(pdu);
    out
}

fn read_input(transaction: u16, address: u16, count: u16) -> Vec<u8> {
    let mut pdu = vec![0x04];
    pdu.extend_from_slice(&address.to_be_bytes());
    pdu.extend_from_slice(&count.to_be_bytes());
    frame(transaction, &pdu)
}

fn write_single(transaction: u16, address: u16, value: u16) -> Vec<u8> {
    let mut pdu = vec![0x06];
    pdu.extend_from_slice(&address.to_be_bytes());
    pdu.extend_from_slice(&value.to_be_bytes());
    frame(transaction, &pdu)
}

/// The answer ends at the length its header announces (bytes 4 and 5, after the 6 of the header).
fn until() -> ReadUntil {
    ReadUntil::LengthPrefixed(LengthPrefix { offset: 4, size: 2, big_endian: true, adds: 6 })
}

fn exception(code: u8) -> &'static str {
    match code {
        1 => "function not supported",
        2 => "address not available",
        3 => "value refused",
        4 => "device failure",
        6 => "device busy",
        _ => "error",
    }
}

/// The PDU of an answer, checked against the request (transaction, function).
fn pdu_of(answer: &[u8], transaction: u16, function: u8) -> Result<&[u8]> {
    if answer.len() < 9 || answer[2..4] != [0, 0] {
        return Err(Error::Invalid("not a Modbus TCP answer".into()));
    }
    if u16::from_be_bytes([answer[0], answer[1]]) != transaction {
        return Err(Error::Invalid("answer to another request".into()));
    }
    let len = u16::from_be_bytes([answer[4], answer[5]]) as usize;
    let pdu = answer.get(7..6 + len).ok_or_else(|| Error::Invalid(format!("incomplete answer ({} of {} bytes)", answer.len(), len + 6)))?;
    match pdu.first() {
        Some(f) if *f == function => Ok(&pdu[1..]),
        Some(f) if *f == function | 0x80 => Err(Error::Invalid(format!("Modbus: {}", exception(pdu.get(1).copied().unwrap_or(0))))),
        _ => Err(Error::Invalid("unexpected Modbus answer".into())),
    }
}

/// The registers of a read answer.
fn registers(answer: &[u8], transaction: u16, count: u16) -> Result<Vec<u16>> {
    let data = pdu_of(answer, transaction, 0x04)?;
    let n = *data.first().ok_or_else(|| Error::Invalid("empty answer".into()))? as usize;
    let bytes = data.get(1..1 + n).filter(|_| n == count as usize * 2).ok_or_else(|| Error::Invalid(format!("{n} bytes for {count} registers")))?;
    Ok(bytes.chunks(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect())
}

/// Registers read from `start`, looked up by address.
struct Block {
    start: u16,
    regs: Vec<u16>,
}

impl Block {
    fn u16(&self, address: u16) -> Option<u16> {
        self.regs.get(address.checked_sub(self.start)? as usize).copied()
    }

    fn i16(&self, address: u16) -> Option<i16> {
        self.u16(address).map(|v| v as i16)
    }

    fn u32(&self, address: u16) -> Option<u32> {
        Some(u32::from(self.u16(address)?) << 16 | u32::from(self.u16(address + 1)?))
    }

    fn i32(&self, address: u16) -> Option<i32> {
        self.u32(address).map(|v| v as i32)
    }

    /// Text, two characters a register, without the padding.
    fn text(&self, address: u16, count: u16) -> Option<String> {
        let bytes: Vec<u8> = (0..count).map(|i| self.u16(address + i)).collect::<Option<Vec<u16>>>()?.iter().flat_map(|r| r.to_be_bytes()).collect();
        let s: String = String::from_utf8_lossy(&bytes).chars().filter(|c| !c.is_control() && *c != ' ').collect();
        (!s.is_empty()).then_some(s)
    }
}

fn round(v: f64, decimals: i32) -> f64 {
    let f = 10f64.powi(decimals);
    (v * f).round() / f
}

// ---------------------------------------------------------------------------------------
// Devices
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Product {
    Plug,
    Meter,
}

impl Product {
    pub fn of(pn: &str) -> Option<Product> {
        match pn {
            PLUG_PN => Some(Product::Plug),
            METER_PN => Some(Product::Meter),
            _ => None,
        }
    }

    pub fn model(self) -> &'static str {
        match self {
            Product::Plug => "Anker SOLIX Smart Plug Gen 2",
            Product::Meter => "Anker SOLIX Smart Meter Gen 2",
        }
    }

    /// The blocks read for the values: those of Anker's integration.
    fn blocks(self) -> &'static [(u16, u16)] {
        match self {
            Product::Plug => &[(30000, 38)],
            Product::Meter => &[(10620, 28), (10648, 48), (10696, 17)],
        }
    }
}

fn plug_state(b: &Block) -> State {
    let mut s = State::new();
    if let Some(on) = b.u16(30029) {
        s.insert("on".into(), json!(on == 1));
    }
    for (address, field, divisor) in [(30030, "power", 10.0), (30031, "voltage", 10.0), (30032, "current", 100.0)] {
        if let Some(v) = b.u16(address) {
            s.insert(field.into(), json!(round(f64::from(v) / divisor, 2)));
        }
    }
    if let Some(e) = b.u32(30033) {
        s.insert("energy_in".into(), json!(round(f64::from(e) / 1000.0, 3)));
    }
    s
}

/// One set of clamps of the meter: where its currents, powers and energies start.
struct Clamps {
    currents: u16,
    powers: u16,
    forward: u16,
}

const CT1: Clamps = Clamps { currents: 10635, powers: 10638, forward: 10650 };
const CT2: Clamps = Clamps { currents: 10666, powers: 10669, forward: 10680 };

fn meter_three_phase(b: &Block) -> bool {
    b.u16(10630) == Some(2)
}

fn clamps_state(b: &Block, c: &Clamps) -> Option<State> {
    let mut s = State::new();
    // Per phase: three powers, then the total (two registers each).
    s.insert("power".into(), json!(b.i32(c.powers + 6)?));
    let three = meter_three_phase(b);
    if three {
        for (i, field) in ["power_a", "power_b", "power_c"].iter().enumerate() {
            if let Some(p) = b.i32(c.powers + 2 * i as u16) {
                s.insert((*field).into(), json!(p));
            }
        }
    } else {
        // The voltages are shared by both sets of clamps.
        if let Some(v) = b.u16(10632).filter(|v| *v > 0) {
            s.insert("voltage".into(), json!(round(f64::from(v) / 10.0, 1)));
        }
        if let Some(a) = b.i16(c.currents) {
            s.insert("current".into(), json!(round(f64::from(a) / 100.0, 2)));
        }
    }
    // Forward A, B, C, total, then reverse A, B, C, total.
    let kwh = |address: u16| b.u32(address).map(|e| round(f64::from(e) / 10.0, 1));
    if let Some(e) = kwh(c.forward + 6) {
        s.insert("energy_in".into(), json!(e));
    }
    if let Some(e) = kwh(c.forward + 14) {
        s.insert("energy_out".into(), json!(e));
    }
    Some(s)
}

/// Whether the second set of clamps is used: it measured something once.
fn ct2_used(b: &Block) -> bool {
    [CT2.forward + 6, CT2.forward + 14, CT2.powers + 6].iter().any(|a| b.u32(*a).is_some_and(|v| v != 0))
}

fn devices_of(product: Product, b: &Block) -> Vec<DeviceInfo> {
    match product {
        Product::Plug => vec![device("plug", "switch.relay", &["switch.relay@1", "energy.power@1", "energy.meter@1"], "Prise Anker")],
        Product::Meter => {
            let mut out = vec![device("ct1", "energy.power", &["energy.power@1", "energy.meter@1"], "Compteur Anker")];
            if ct2_used(b) {
                out.push(device("ct2", "energy.power", &["energy.power@1", "energy.meter@1"], "Compteur Anker · pinces 2"));
            }
            out
        }
    }
}

fn states_of(product: Product, b: &Block) -> Vec<(String, State)> {
    match product {
        Product::Plug => vec![("plug".into(), plug_state(b))],
        Product::Meter => {
            let mut out = vec![];
            if let Some(s) = clamps_state(b, &CT1) {
                out.push(("ct1".into(), s));
            }
            if ct2_used(b) {
                if let Some(s) = clamps_state(b, &CT2) {
                    out.push(("ct2".into(), s));
                }
            }
            out
        }
    }
}

fn serial(product: Product, b: &Block) -> Option<String> {
    match product {
        Product::Plug => b.text(30005, 12),
        Product::Meter => b.text(10702, 10),
    }
}

fn firmware(product: Product, b: &Block) -> Option<String> {
    match product {
        Product::Plug => b.text(30017, 6),
        Product::Meter => {
            let bytes: Vec<u8> = [b.u16(10696)?, b.u16(10697)?].iter().flat_map(|r| r.to_be_bytes()).collect();
            Some(bytes.iter().map(u8::to_string).collect::<Vec<_>>().join("."))
        }
    }
}

// ---------------------------------------------------------------------------------------
// Driver
// ---------------------------------------------------------------------------------------

/// Host or `host:port` on the local network: letters, digits, dots, dashes, colons.
fn valid_host(host: &str) -> bool {
    !host.is_empty() && host.len() <= 253 && host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '[' | ']'))
}

fn address_of(host: &str) -> String {
    if host.contains(':') {
        host.to_string()
    } else {
        format!("{host}:{PORT}")
    }
}

/// One question and its answer on a connection of its own.
fn ask(addr: &str, request: &[u8], timeout_ms: u32) -> Result<Vec<u8>> {
    host::exchange(&format!("tcp://{addr}"), request, until(), timeout_ms)
}

fn read_block(addr: &str, transaction: u16, start: u16, count: u16, timeout_ms: u32) -> Result<Block> {
    let answer = ask(addr, &read_input(transaction, start, count), timeout_ms)?;
    Ok(Block { start, regs: registers(&answer, transaction, count)? })
}

/// The product at an address, from its product number.
fn product_at(addr: &str, timeout_ms: u32) -> Result<(String, Option<Product>)> {
    let b = read_block(addr, 1, PN, 5, timeout_ms)?;
    let pn = b.text(PN, 5).unwrap_or_default();
    let product = Product::of(&pn);
    Ok((pn, product))
}

pub struct Anker {
    addr: String,
    /// Serial number expected at this address (from the scanner).
    device_id: Option<String>,
    product: Cell<Option<Product>>,
    transaction: Cell<u16>,
    /// The last answer read, for the diagnostic.
    last: RefCell<Option<String>>,
}

impl Anker {
    fn next(&self) -> u16 {
        let t = self.transaction.get().wrapping_add(1).max(1);
        self.transaction.set(t);
        t
    }

    fn product(&self) -> Result<Product> {
        if let Some(p) = self.product.get() {
            return Ok(p);
        }
        let (pn, product) = product_at(&self.addr, TIMEOUT_MS)?;
        let p = product.ok_or_else(|| Error::Unsupported(format!("Anker product {pn:?}: not supported (Smart Plug Gen 2 and Smart Meter Gen 2 only)")))?;
        self.product.set(Some(p));
        Ok(p)
    }

    /// Every block of the product, joined into one.
    fn values(&self, product: Product) -> Result<Block> {
        let blocks = product.blocks();
        let start = blocks[0].0;
        let end = blocks.iter().map(|(s, n)| s + n).max().unwrap_or(start);
        let mut regs = vec![0u16; (end - start) as usize];
        for (s, n) in blocks {
            let t = self.next();
            let b = read_block(&self.addr, t, *s, *n, TIMEOUT_MS)?;
            regs[(s - start) as usize..(s - start + n) as usize].copy_from_slice(&b.regs);
        }
        *self.last.borrow_mut() = Some(regs.iter().map(|r| format!("{r:04X}")).collect::<Vec<_>>().join(" "));
        Ok(Block { start, regs })
    }
}

impl Driver for Anker {
    fn open(config: Value, _secret: Option<Value>) -> Result<Self> {
        let host = config["host"].as_str().map(str::trim).unwrap_or_default();
        if !valid_host(host) {
            return Err(Error::Invalid("invalid address".into()));
        }
        let device_id = config["device_id"].as_str().map(str::trim).filter(|d| !d.is_empty()).map(str::to_string);
        Ok(Anker { addr: address_of(host), device_id, product: Cell::new(None), transaction: Cell::new(1), last: RefCell::new(None) })
    }

    fn describe(&self) -> Result<Described> {
        let product = self.product()?;
        let b = self.values(product)?;
        let serial = serial(product, &b);
        if let (Some(expected), Some(found)) = (&self.device_id, &serial) {
            if expected != found {
                return Err(Error::Invalid(format!("another Anker device answers at this address ({found}): scan the network again")));
            }
        }
        let mut model = product.model().to_string();
        if product == Product::Meter {
            model += if meter_three_phase(&b) { " (triphasé)" } else { " (monophasé)" };
        }
        Ok(Described { model, firmware: firmware(product, &b), devices: devices_of(product, &b), identity: serial })
    }

    fn read(&self) -> Result<Vec<(String, State)>> {
        let product = self.product()?;
        Ok(states_of(product, &self.values(product)?))
    }

    fn act(&self, reference: &str, trait_name: &str, action: &str, params: &State) -> Result<()> {
        if self.product()? != Product::Plug || reference != "plug" || (trait_name, action) != ("switch.relay", "set") {
            return Err(Error::Unsupported(format!("{reference}: {trait_name}.{action}")));
        }
        let on = params.get("on").and_then(Value::as_bool).ok_or_else(|| Error::Invalid("on is required".into()))?;
        let t = self.next();
        let answer = ask(&self.addr, &write_single(t, 30047, u16::from(on)), TIMEOUT_MS)?;
        pdu_of(&answer, t, 0x06).map(|_| ())
    }

    fn diagnostic(&self) -> Result<Value> {
        let (pn, product) = product_at(&self.addr, TIMEOUT_MS)?;
        let mut d = json!({"product_number": pn, "supported": product.is_some()});
        if let Some(p) = product {
            self.product.set(Some(p));
            let b = self.values(p)?;
            d["model"] = json!(p.model());
            d["firmware"] = json!(firmware(p, &b));
            d["values"] = json!(states_of(p, &b).into_iter().map(|(k, s)| (k, Value::Object(s))).collect::<Map<String, Value>>());
            if p == Product::Meter {
                d["three_phase"] = json!(meter_three_phase(&b));
                d["ct2_used"] = json!(ct2_used(&b));
            }
            d["registers"] = json!({"start": b.start, "values": self.last.borrow().clone()});
        }
        Ok(d)
    }

    fn identify(host_: &str, announced: Option<&Value>) -> Option<Found> {
        if announced.is_some() {
            return None;
        }
        let addr = address_of(host_);
        let product = product_at(&addr, 2000).ok()?.1?;
        let serial = match product {
            Product::Plug => read_block(&addr, 2, 30005, 12, 2000).ok().and_then(|b| b.text(30005, 12)),
            Product::Meter => read_block(&addr, 2, 10702, 10, 2000).ok().and_then(|b| b.text(10702, 10)),
        };
        Some(Found { model: product.model().into(), device_id: serial, port: Some(PORT), ..Found::default() })
    }

    fn probe(host_: &str) -> Option<String> {
        product_at(&address_of(host_), 2000).ok()?.1.map(|p| p.model().to_string())
    }
}

module!(Anker);

#[cfg(test)]
mod tests {
    use super::*;
    use breezone_module_sdk::testing;

    const MANIFEST: &str = include_str!("../manifest.json");

    /// The answer of a device to a read of `count` registers.
    fn answer(transaction: u16, regs: &[u16]) -> Vec<u8> {
        let mut pdu = vec![0x04, (regs.len() * 2) as u8];
        for r in regs {
            pdu.extend_from_slice(&r.to_be_bytes());
        }
        frame(transaction, &pdu)
    }

    fn text(s: &str, count: usize) -> Vec<u16> {
        let mut bytes = s.as_bytes().to_vec();
        bytes.resize(count * 2, 0);
        bytes.chunks(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect()
    }

    fn put(regs: &mut [u16], start: u16, address: u16, values: &[u16]) {
        regs[(address - start) as usize..(address - start) as usize + values.len()].copy_from_slice(values);
    }

    fn u32s(v: u32) -> [u16; 2] {
        [(v >> 16) as u16, v as u16]
    }

    /// A plug drawing 23.4 W, on, 12.345 kWh counted.
    fn plug_block() -> Vec<u16> {
        let mut r = vec![0u16; 38];
        put(&mut r, 30000, 30005, &text("APC1234567890ABCDEFGH", 12));
        put(&mut r, 30000, 30017, &text("0.0.8.4", 6));
        put(&mut r, 30000, 30029, &[1, 234, 2301, 12]);
        put(&mut r, 30000, 30033, &u32s(12345));
        r
    }

    /// A single-phase meter drawing 512 W from the grid, its second clamps unused.
    fn meter_blocks() -> (Vec<u16>, Vec<u16>, Vec<u16>) {
        let mut a = vec![0u16; 28];
        put(&mut a, 10620, 10620, &text("AE1X0", 10));
        put(&mut a, 10620, 10630, &[1, 0, 2315, 0, 0, 221, 0, 0]);
        put(&mut a, 10620, 10638, &u32s(512));
        put(&mut a, 10620, 10644, &u32s(512));
        let mut b = vec![0u16; 48];
        put(&mut b, 10648, 10656, &u32s(45678));
        put(&mut b, 10648, 10664, &u32s(1234));
        let mut c = vec![0u16; 17];
        put(&mut c, 10696, 10696, &[0x0001, 0x0203]);
        put(&mut c, 10696, 10702, &text("AMT0001", 10));
        (a, b, c)
    }

    #[test]
    fn frames_a_read_and_a_write() {
        assert_eq!(read_input(7, 32768, 5), vec![0, 7, 0, 0, 0, 6, 1, 4, 0x80, 0, 0, 5]);
        assert_eq!(write_single(8, 30047, 1), vec![0, 8, 0, 0, 0, 6, 1, 6, 0x75, 0x5f, 0, 1]);
    }

    #[test]
    fn reads_registers_and_refuses_errors() {
        assert_eq!(registers(&answer(3, &[1, 0xffff]), 3, 2).unwrap(), vec![1, 0xffff]);
        assert!(registers(&answer(3, &[1, 2]), 4, 2).is_err(), "another transaction");
        assert!(registers(&answer(3, &[1]), 3, 2).is_err(), "too short");
        let refused = frame(3, &[0x84, 2]);
        let e = registers(&refused, 3, 2).unwrap_err();
        assert!(format!("{e:?}").contains("address not available"), "{e:?}");
    }

    #[test]
    fn the_product_number_tells_the_device() {
        assert_eq!(Product::of("A17X8"), Some(Product::Plug));
        assert_eq!(Product::of("AE1X0"), Some(Product::Meter));
        assert_eq!(Product::of("A17C1"), None);
        let b = Block { start: PN, regs: text("A17X8", 5) };
        assert_eq!(b.text(PN, 5).as_deref(), Some("A17X8"));
    }

    #[test]
    fn a_plug_and_its_values() {
        let b = Block { start: 30000, regs: plug_block() };
        let s = plug_state(&b);
        assert_eq!(s["on"], true);
        assert_eq!((s["power"].clone(), s["voltage"].clone(), s["current"].clone()), (json!(23.4), json!(230.1), json!(0.12)));
        assert_eq!(s["energy_in"], 12.345);
        assert_eq!(serial(Product::Plug, &b).as_deref(), Some("APC1234567890ABCDEFGH"));
        assert_eq!(firmware(Product::Plug, &b).as_deref(), Some("0.0.8.4"));
    }

    #[test]
    fn a_single_phase_meter() {
        let (a, b, c) = meter_blocks();
        let mut regs = a;
        regs.extend(b);
        regs.extend(c);
        let block = Block { start: 10620, regs };
        let states: std::collections::HashMap<String, State> = states_of(Product::Meter, &block).into_iter().collect();
        assert_eq!(states.len(), 1, "the second clamps are not used");
        let s = &states["ct1"];
        assert_eq!(s["power"], 512);
        assert_eq!((s["voltage"].clone(), s["current"].clone()), (json!(231.5), json!(2.21)));
        assert_eq!((s["energy_in"].clone(), s["energy_out"].clone()), (json!(4567.8), json!(123.4)));
        assert!(s.get("power_a").is_none());
        assert_eq!(firmware(Product::Meter, &block).as_deref(), Some("0.1.2.3"));
        assert_eq!(serial(Product::Meter, &block).as_deref(), Some("AMT0001"));
    }

    #[test]
    fn a_three_phase_meter_with_its_second_clamps() {
        let (mut a, mut b, c) = meter_blocks();
        put(&mut a, 10620, 10630, &[2]);
        put(&mut a, 10620, 10638, &[0, 100, 0, 200, 0xffff, 0xff9c]);
        put(&mut a, 10620, 10644, &u32s(200));
        put(&mut b, 10648, 10675, &u32s(0xffff_fc18));
        put(&mut b, 10648, 10694, &u32s(50));
        let mut regs = a;
        regs.extend(b);
        regs.extend(c);
        let block = Block { start: 10620, regs };
        let states: std::collections::HashMap<String, State> = states_of(Product::Meter, &block).into_iter().collect();
        let s = &states["ct1"];
        assert_eq!((s["power_a"].clone(), s["power_b"].clone(), s["power_c"].clone()), (json!(100), json!(200), json!(-100)));
        assert!(s.get("voltage").is_none());
        assert_eq!(states["ct2"]["power"], -1000);
        assert_eq!(states["ct2"]["energy_out"], 5.0);
        assert_eq!(devices_of(Product::Meter, &block).len(), 2);
    }

    #[test]
    fn a_plug_through_the_host() {
        testing::reset();
        testing::queue_exchanges(vec![answer(1, &text("A17X8", 5))]);
        assert_eq!(Anker::probe("192.168.1.90").as_deref(), Some("Anker SOLIX Smart Plug Gen 2"));
        let a: Anker = testing::open(json!({"host": "192.168.1.90"}), None);
        // describe: the product number, then the values; read: the values.
        testing::queue_exchanges(vec![answer(1, &text("A17X8", 5)), answer(2, &plug_block()), answer(3, &plug_block())]);
        let s = testing::check(&a, MANIFEST);
        assert_eq!(s["plug"]["power"], 23.4);
        assert!(testing::requests().iter().all(|r| r.starts_with("EXCHANGE tcp://192.168.1.90:502")));
        testing::queue_exchanges(vec![write_single(4, 30047, 0)]);
        let mut off = State::new();
        off.insert("on".into(), json!(false));
        a.act("plug", "switch.relay", "set", &off).unwrap();
    }

    #[test]
    fn a_meter_through_the_host() {
        testing::reset();
        let (x, y, z) = meter_blocks();
        let a: Anker = testing::open(json!({"host": "192.168.1.91"}), None);
        testing::queue_exchanges(vec![answer(1, &text("AE1X0", 5)), answer(2, &x), answer(3, &y), answer(4, &z), answer(5, &x), answer(6, &y), answer(7, &z)]);
        let s = testing::check(&a, MANIFEST);
        assert_eq!(s["ct1"]["power"], 512);
        let mut on = State::new();
        on.insert("on".into(), json!(true));
        assert!(a.act("ct1", "switch.relay", "set", &on).is_err(), "a meter switches nothing");
    }

    #[test]
    fn another_product_is_not_supported() {
        testing::reset();
        testing::queue_exchanges(vec![answer(1, &text("A17C1", 5))]);
        assert_eq!(Anker::probe("192.168.1.92"), None);
        let a: Anker = testing::open(json!({"host": "192.168.1.92"}), None);
        testing::queue_exchanges(vec![answer(1, &text("A17C1", 5))]);
        assert!(matches!(a.describe(), Err(Error::Unsupported(_))));
    }
}
