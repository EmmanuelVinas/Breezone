//! Deye micro-inverters (SUN-M200G4, SUN600 to SUN2000G3…), through the Solarman logger built in:
//! protocol V5 on TCP port 8899, a Modbus RTU frame wrapped as
//! `A5 · length (2, LE) · control (2) · sequence (2) · logger serial (4, LE) · payload · checksum · 15`.
//!
//! Each frame carries the serial number of the logger: typed in, or read from its web page
//! (`/status.html`, `cover_mid`, admin/admin unless changed). The registers are holding ones
//! (function 03), as `ha-solarman` reads them (`deye_micro.yaml`): 32-bit values low word first.
//!
//! When the logger refuses V5 (a firmware that closes port 8899), its web page still gives the
//! power, today's and the total production: Breezone shows the inverter then, without its panels.
//!
//! Read only: the inverter takes an on/off and a power limit, which Breezone does not send yet.

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use breezone_module_sdk::{device, host, json, module, Described, DeviceInfo, Driver, Error, Found, LengthPrefix, Map, ReadUntil, Result, State, Value};
use std::cell::{Cell, RefCell};

pub const PORT: u16 = 8899;
const UNIT: u8 = 1;
const TIMEOUT_MS: u32 = 5000;
/// Milliseconds between two requests: a logger copes badly with more.
const SPACING_MS: i64 = 500;

// ---------------------------------------------------------------------------------------
// Modbus RTU in Solarman V5
// ---------------------------------------------------------------------------------------

/// CRC-16/MODBUS.
fn crc16(data: &[u8]) -> u16 {
    let mut crc = 0xffffu16;
    for b in data {
        crc ^= u16::from(*b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xa001 } else { crc >> 1 };
        }
    }
    crc
}

fn read_holding(address: u16, count: u16) -> Vec<u8> {
    let mut out = vec![UNIT, 0x03];
    out.extend_from_slice(&address.to_be_bytes());
    out.extend_from_slice(&count.to_be_bytes());
    out.extend_from_slice(&crc16(&out).to_le_bytes());
    out
}

fn checksum(frame: &[u8]) -> u8 {
    frame.iter().fold(0u8, |sum, b| sum.wrapping_add(*b))
}

fn v5_request(logger: u32, sequence: u16, modbus: &[u8]) -> Vec<u8> {
    // Frame type 2 (inverter), sensor type 0, then three times (total, power on, offset) at 0.
    let mut payload = vec![0x02, 0x00, 0x00];
    payload.extend_from_slice(&[0; 12]);
    payload.extend_from_slice(modbus);
    let mut out = vec![0xa5];
    out.extend_from_slice(&(payload.len() as u16).to_le_bytes());
    out.extend_from_slice(&[0x10, 0x45]);
    out.extend_from_slice(&sequence.to_le_bytes());
    out.extend_from_slice(&logger.to_le_bytes());
    out.extend_from_slice(&payload);
    out.push(checksum(&out[1..]));
    out.push(0x15);
    out
}

/// The answer ends at the length of its payload (bytes 1 and 2), plus its header and its end.
fn until() -> ReadUntil {
    ReadUntil::LengthPrefixed(LengthPrefix { offset: 1, size: 2, big_endian: false, adds: 13 })
}

/// The Modbus RTU frame of an answer, checked.
fn v5_answer(buf: &[u8], logger: u32) -> Result<&[u8]> {
    if buf.len() < 13 || buf[0] != 0xa5 || buf[buf.len() - 1] != 0x15 {
        return Err(Error::Invalid("not a Solarman V5 answer".into()));
    }
    let len = u16::from_le_bytes([buf[1], buf[2]]) as usize;
    if buf.len() != len + 13 {
        return Err(Error::Invalid(format!("incomplete answer ({} of {} bytes)", buf.len(), len + 13)));
    }
    if checksum(&buf[1..buf.len() - 2]) != buf[buf.len() - 2] {
        return Err(Error::Invalid("corrupted answer (checksum)".into()));
    }
    if buf[3..5] != [0x10, 0x15] {
        return Err(Error::Invalid(format!("unexpected Solarman frame ({:02X}{:02X})", buf[4], buf[3])));
    }
    if u32::from_le_bytes([buf[7], buf[8], buf[9], buf[10]]) != logger {
        return Err(Error::Invalid("answer of another logger: check its serial number".into()));
    }
    // Frame type, status, then three times: 14 bytes before the Modbus frame.
    buf.get(25..buf.len() - 2).filter(|m| m.len() >= 5).ok_or_else(|| Error::Invalid("the logger got no answer from the inverter".into()))
}

/// The registers of a Modbus RTU answer to a read of `count` registers.
fn registers(rtu: &[u8], count: u16) -> Result<Vec<u16>> {
    let body = &rtu[..rtu.len() - 2];
    if crc16(body).to_le_bytes() != rtu[rtu.len() - 2..] {
        return Err(Error::Invalid("corrupted Modbus answer (CRC)".into()));
    }
    match body.get(1) {
        Some(0x03) => {}
        Some(0x83) => return Err(Error::Invalid(format!("Modbus exception {}", body.get(2).copied().unwrap_or(0)))),
        _ => return Err(Error::Invalid("unexpected Modbus answer".into())),
    }
    let n = body[2] as usize;
    let data = body.get(3..3 + n).filter(|_| n == count as usize * 2).ok_or_else(|| Error::Invalid(format!("{n} bytes for {count} registers")))?;
    Ok(data.chunks(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect())
}

struct Block {
    start: u16,
    regs: Vec<u16>,
}

impl Block {
    fn u16(&self, address: u16) -> Option<u16> {
        self.regs.get(address.checked_sub(self.start)? as usize).copied()
    }

    /// 32 bits, low word first.
    fn u32(&self, low: u16) -> Option<u32> {
        Some(u32::from(self.u16(low)?) | u32::from(self.u16(low + 1)?) << 16)
    }

    fn text(&self, address: u16, count: u16) -> Option<String> {
        let bytes: Vec<u8> = (0..count).map(|i| self.u16(address + i)).collect::<Option<Vec<u16>>>()?.iter().flat_map(|r| r.to_be_bytes()).collect();
        let s: String = String::from_utf8_lossy(&bytes).chars().filter(|c| c.is_ascii_alphanumeric()).collect();
        (!s.is_empty()).then_some(s)
    }
}

fn round(v: f64, decimals: i32) -> f64 {
    let f = 10f64.powi(decimals);
    (v * f).round() / f
}

// ---------------------------------------------------------------------------------------
// The inverter
// ---------------------------------------------------------------------------------------

const INFO: (u16, u16) = (0x0000, 0x13);
const VALUES: (u16, u16) = (0x003b, 0x20);
const PANELS: (u16, u16) = (0x006d, 0x08);
/// Total production of each input (one register each, not evenly spaced).
const PANEL_TOTALS: [u16; 4] = [0x0045, 0x0047, 0x004a, 0x004d];

#[derive(Debug, Clone, PartialEq)]
pub struct Info {
    pub serial: String,
    pub rated_w: Option<f64>,
    pub inputs: u16,
}

fn info_of(b: &Block) -> Result<Info> {
    // 4 (or 0x0400): a micro-inverter.
    if !matches!(b.u16(0x0000), Some(0x0004 | 0x0400)) {
        return Err(Error::Unsupported(format!("not a Deye micro-inverter (type {:#06x})", b.u16(0).unwrap_or(0))));
    }
    let serial = b.text(0x0003, 5).ok_or_else(|| Error::Invalid("no serial number".into()))?;
    let rated_w = b.u32(0x0010).map(|w| f64::from(w) / 10.0).filter(|w| *w > 0.0 && *w <= 2250.0);
    // Inputs in the high byte of 0x0012 (from 0x0101 on), else from the rated power.
    let inputs = b.u16(0x0012).filter(|v| *v >= 0x0101).map(|v| (v & 0x0f00) >> 8).filter(|n| (1..=4).contains(n)).unwrap_or(match rated_w {
        Some(w) if w <= 500.0 => 1,
        Some(w) if w <= 1000.0 => 2,
        _ => 4,
    });
    Ok(Info { serial, rated_w, inputs })
}

/// "SUN-M200G4 (2 kW, 4 entrées)".
fn model_of(info: &Info) -> String {
    let mut parts = vec![];
    if let Some(w) = info.rated_w {
        parts.push(if w >= 1000.0 { format!("{} kW", round(w / 1000.0, 2)) } else { format!("{w} W") });
    }
    parts.push(format!("{} entrée{}", info.inputs, if info.inputs > 1 { "s" } else { "" }));
    format!("Deye micro-onduleur ({})", parts.join(", "))
}

fn devices_of(info: &Info) -> Vec<DeviceInfo> {
    let short = &info.serial[info.serial.len().saturating_sub(4)..];
    let mut out = vec![device(format!("inv:{}", info.serial), "solar.inverter", &["energy.power@1", "solar.inverter@1"], format!("Deye ({short})"))];
    for port in 1..=info.inputs {
        out.push(device(format!("pv:{}:{port}", info.serial), "solar.panel", &["solar.panel@1"], format!("Panneau {port} ({short})")));
    }
    out
}

fn states_of(info: &Info, v: &Block, p: &Block) -> Vec<(String, State)> {
    let tenth = |r: Option<u16>| r.map(|x| round(f64::from(x) / 10.0, 1));
    let mut inv = State::new();
    inv.insert("power".into(), json!(v.u32(0x0056).map(|w| round(f64::from(w) / 10.0, 1)).unwrap_or(0.0)));
    inv.insert("energy_today".into(), json!(tenth(v.u16(0x003c)).unwrap_or(0.0)));
    inv.insert("energy_total".into(), json!(v.u32(0x003f).map(|e| round(f64::from(e) / 10.0, 1)).unwrap_or(0.0)));
    if let Some(volts) = tenth(v.u16(0x0049)).filter(|x| *x > 0.0) {
        inv.insert("voltage".into(), json!(volts));
    }
    if let Some(a) = v.u16(0x004c) {
        inv.insert("current".into(), json!(round(f64::from(a as i16) / 10.0, 1)));
    }
    if let Some(hz) = v.u16(0x004f).filter(|x| *x > 0) {
        inv.insert("frequency".into(), json!(round(f64::from(hz) / 100.0, 2)));
    }
    if let Some(t) = v.u16(0x005a).filter(|x| *x > 0) {
        inv.insert("temperature".into(), json!(round((f64::from(t) - 1000.0) / 100.0, 1)));
    }
    inv.insert("panels".into(), json!(info.inputs));
    let mut out = vec![(format!("inv:{}", info.serial), inv)];
    for port in 1..=info.inputs {
        let i = port - 1;
        let volts = tenth(p.u16(0x006d + 2 * i)).unwrap_or(0.0);
        let amps = tenth(p.u16(0x006e + 2 * i)).unwrap_or(0.0);
        let mut s = State::new();
        s.insert("power".into(), json!(round(volts * amps, 1)));
        s.insert("voltage".into(), json!(volts));
        s.insert("current".into(), json!(amps));
        s.insert("energy_today".into(), json!(tenth(v.u16(0x0041 + i)).unwrap_or(0.0)));
        s.insert("energy_total".into(), json!(tenth(v.u16(PANEL_TOTALS[i as usize])).unwrap_or(0.0)));
        s.insert("inverter".into(), json!(info.serial));
        s.insert("port".into(), json!(port));
        out.push((format!("pv:{}:{port}", info.serial), s));
    }
    out
}

// ---------------------------------------------------------------------------------------
// The web page of the logger
// ---------------------------------------------------------------------------------------

/// `var name = "value";` of the status page.
fn page_var(page: &str, name: &str) -> Option<String> {
    let at = page.find(&format!("var {name}"))?;
    let rest = &page[at..];
    let open = rest.find('"')? + 1;
    let close = rest[open..].find('"')? + open;
    let v = rest[open..close].trim();
    (!v.is_empty()).then(|| v.to_string())
}

#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    /// Serial number of the inverter.
    pub serial: Option<String>,
    /// Serial number of the logger (`cover_mid`).
    pub logger: Option<u32>,
    pub power: Option<f64>,
    pub today: Option<f64>,
    pub total: Option<f64>,
}

fn page_of(text: &str) -> Option<Page> {
    let num = |name: &str| page_var(text, name).and_then(|v| v.parse::<f64>().ok());
    let page = Page {
        serial: page_var(text, "webdata_sn").filter(|s| s.chars().all(|c| c.is_ascii_alphanumeric())),
        logger: page_var(text, "cover_mid").and_then(|v| v.parse().ok()),
        power: num("webdata_now_p"),
        today: num("webdata_today_e"),
        total: num("webdata_total_e"),
    };
    (page.serial.is_some() || page.logger.is_some()).then_some(page)
}

fn fetch_page(host_: &str, password: &str, timeout_ms: u32) -> Result<Page> {
    let auth = format!("Basic {}", STANDARD.encode(format!("admin:{password}")));
    let a = host::request("GET", &format!("http://{host_}/status.html"), &[("Authorization", &auth)], None, timeout_ms)?;
    match a.status {
        200 => page_of(&String::from_utf8_lossy(&a.body)).ok_or_else(|| Error::Invalid("not the page of a Solarman logger".into())),
        401 => Err(Error::Invalid("the logger refuses its password (web page)".into())),
        s => Err(Error::Invalid(format!("web page of the logger: HTTP {s}"))),
    }
}

// ---------------------------------------------------------------------------------------
// Driver
// ---------------------------------------------------------------------------------------

/// A host on the local network: letters, digits, dots, dashes (the port is that of V5).
fn valid_host(host: &str) -> bool {
    !host.is_empty() && host.len() <= 253 && host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'))
}

pub struct Deye {
    host: String,
    password: String,
    /// Serial number of the inverter expected at this address (from the scanner).
    device_id: Option<String>,
    logger: Cell<Option<u32>>,
    info: RefCell<Option<Info>>,
    /// V5 refused at the last try: the web page is read instead.
    page_only: Cell<bool>,
    sequence: Cell<u16>,
    last: Cell<Option<i64>>,
}

impl Deye {
    fn logger(&self) -> Result<u32> {
        if let Some(l) = self.logger.get() {
            return Ok(l);
        }
        let l = fetch_page(&self.host, &self.password, TIMEOUT_MS)?.logger.ok_or_else(|| Error::Invalid("no logger serial number on its web page: type it in".into()))?;
        self.logger.set(Some(l));
        Ok(l)
    }

    fn read(&self, (start, count): (u16, u16)) -> Result<Block> {
        let logger = self.logger()?;
        if let Some(last) = self.last.get() {
            let gone = (host::now() * 1000 - last).max(0);
            if gone < SPACING_MS {
                host::wait((SPACING_MS - gone) as u32);
            }
        }
        let sequence = self.sequence.get().wrapping_add(1);
        self.sequence.set(sequence);
        let answer = host::exchange(&format!("tcp://{}:{PORT}", self.host), &v5_request(logger, sequence, &read_holding(start, count)), until(), TIMEOUT_MS);
        self.last.set(Some(host::now() * 1000));
        Ok(Block { start, regs: registers(v5_answer(&answer?, logger)?, count)? })
    }

    fn info(&self) -> Result<Info> {
        if let Some(i) = self.info.borrow().clone() {
            return Ok(i);
        }
        let i = info_of(&self.read(INFO)?)?;
        *self.info.borrow_mut() = Some(i.clone());
        Ok(i)
    }

    fn check_serial(&self, serial: &str) -> Result<()> {
        match &self.device_id {
            Some(expected) if expected != serial => Err(Error::Invalid(format!("another inverter answers at this address ({serial}): scan the network again"))),
            _ => Ok(()),
        }
    }

    fn read_page(&self) -> Result<(Page, String)> {
        let page = fetch_page(&self.host, &self.password, TIMEOUT_MS)?;
        let serial = page.serial.clone().ok_or_else(|| Error::Invalid("no inverter serial number on the logger's page".into()))?;
        self.check_serial(&serial)?;
        Ok((page, serial))
    }
}

fn page_state(page: &Page) -> State {
    let mut s = State::new();
    s.insert("power".into(), json!(page.power.unwrap_or(0.0)));
    s.insert("energy_today".into(), json!(page.today.unwrap_or(0.0)));
    s.insert("energy_total".into(), json!(page.total.unwrap_or(0.0)));
    s.insert("panels".into(), json!(0));
    s
}

impl Driver for Deye {
    fn open(config: Value, secret: Option<Value>) -> Result<Self> {
        let host = config["host"].as_str().map(str::trim).unwrap_or_default();
        if !valid_host(host) {
            return Err(Error::Invalid("invalid logger address".into()));
        }
        let logger = match config["serial"].as_str().map(str::trim).filter(|s| !s.is_empty()) {
            Some(s) => Some(s.parse::<u32>().map_err(|_| Error::Invalid("the logger serial number is a number (10 digits, on its label)".into()))?),
            None => config["serial"].as_u64().and_then(|n| u32::try_from(n).ok()),
        };
        let password = secret.and_then(|s| s["password"].as_str().map(str::to_string)).filter(|p| !p.is_empty()).unwrap_or_else(|| "admin".into());
        let device_id = config["device_id"].as_str().map(str::trim).filter(|d| !d.is_empty()).map(str::to_string);
        Ok(Deye {
            host: host.to_string(),
            password,
            device_id,
            logger: Cell::new(logger),
            info: RefCell::new(None),
            page_only: Cell::new(false),
            sequence: Cell::new(0),
            last: Cell::new(None),
        })
    }

    fn describe(&self) -> Result<Described> {
        match self.info() {
            Ok(info) => {
                self.check_serial(&info.serial)?;
                self.page_only.set(false);
                Ok(Described { model: model_of(&info), firmware: None, devices: devices_of(&info), identity: Some(info.serial) })
            }
            Err(Error::Unreachable(e)) => {
                // Port 8899 closed: the web page, if it answers, gives the inverter alone.
                let (_, serial) = self.read_page().map_err(|_| Error::Unreachable(e))?;
                self.page_only.set(true);
                let short = serial[serial.len().saturating_sub(4)..].to_string();
                let devices = vec![device(format!("inv:{serial}"), "solar.inverter", &["energy.power@1", "solar.inverter@1"], format!("Deye ({short})"))];
                Ok(Described { model: "Deye micro-onduleur (page web du boîtier)".into(), firmware: None, devices, identity: Some(serial) })
            }
            Err(e) => Err(e),
        }
    }

    fn read(&self) -> Result<Vec<(String, State)>> {
        if self.page_only.get() {
            let (page, serial) = self.read_page()?;
            return Ok(vec![(format!("inv:{serial}"), page_state(&page))]);
        }
        let info = self.info()?;
        let values = self.read(VALUES)?;
        let panels = self.read(PANELS)?;
        Ok(states_of(&info, &values, &panels))
    }

    fn act(&self, reference: &str, trait_name: &str, action: &str, _params: &State) -> Result<()> {
        Err(Error::Unsupported(format!("{reference}: {trait_name}.{action}")))
    }

    fn diagnostic(&self) -> Result<Value> {
        let mut d = Map::new();
        d.insert(
            "page".into(),
            json!(fetch_page(&self.host, &self.password, TIMEOUT_MS)
                .map(|p| json!({"inverter": p.serial, "logger": p.logger, "power": p.power, "today": p.today, "total": p.total}))
                .unwrap_or_else(|e| json!(format!("{e:?}")))),
        );
        d.insert("logger".into(), json!(self.logger.get()));
        let hex = |b: &Block| b.regs.iter().map(|r| format!("{r:04X}")).collect::<Vec<_>>().join(" ");
        for (name, range) in [("info", INFO), ("values", VALUES), ("panels", PANELS)] {
            d.insert(name.into(), json!(self.read(range).map(|b| hex(&b)).unwrap_or_else(|e| format!("{e:?}"))));
        }
        if let Ok(info) = self.info() {
            d.insert("model".into(), json!(model_of(&info)));
            d.insert("inputs".into(), json!(info.inputs));
        }
        Ok(Value::Object(d))
    }

    fn identify(host_: &str, announced: Option<&Value>) -> Option<Found> {
        if announced.is_some() || !valid_host(host_) {
            return None;
        }
        let page = fetch_page(host_, "admin", 2000).ok()?;
        Some(Found { model: "Deye micro-onduleur".into(), note: page.logger.map(|l| format!("boîtier Wi-Fi {l}")), device_id: page.serial, ..Found::default() })
    }

    fn probe(host_: &str) -> Option<String> {
        valid_host(host_).then(|| fetch_page(host_, "admin", 2000).ok()).flatten().map(|_| "Deye micro-onduleur".into())
    }
}

module!(Deye);

#[cfg(test)]
mod tests {
    use super::*;
    use breezone_module_sdk::testing;

    const MANIFEST: &str = include_str!("../manifest.json");
    const LOGGER: u32 = 4_123_456_789;

    /// The answer of a logger: frame type, status, three times, then the Modbus frame.
    fn v5(logger: u32, sequence: u16, rtu: &[u8]) -> Vec<u8> {
        let mut payload = vec![0x02, 0x01];
        payload.extend_from_slice(&[0; 12]);
        payload.extend_from_slice(rtu);
        let mut out = vec![0xa5];
        out.extend_from_slice(&(payload.len() as u16).to_le_bytes());
        out.extend_from_slice(&[0x10, 0x15]);
        out.extend_from_slice(&sequence.to_le_bytes());
        out.extend_from_slice(&logger.to_le_bytes());
        out.extend_from_slice(&payload);
        out.push(checksum(&out[1..]));
        out.push(0x15);
        out
    }

    fn rtu(regs: &[u16]) -> Vec<u8> {
        let mut out = vec![UNIT, 0x03, (regs.len() * 2) as u8];
        for r in regs {
            out.extend_from_slice(&r.to_be_bytes());
        }
        out.extend_from_slice(&crc16(&out).to_le_bytes());
        out
    }

    fn put(regs: &mut [u16], start: u16, address: u16, values: &[u16]) {
        regs[(address - start) as usize..(address - start) as usize + values.len()].copy_from_slice(values);
    }

    /// A 2 kW micro-inverter with four inputs, serial 2403123456.
    fn info_regs() -> Vec<u16> {
        let mut r = vec![0u16; INFO.1 as usize];
        put(&mut r, 0, 0x0000, &[0x0004, 1]);
        put(&mut r, 0, 0x0003, &[0x3234, 0x3033, 0x3132, 0x3334, 0x3536]);
        put(&mut r, 0, 0x0010, &[20000, 0]);
        put(&mut r, 0, 0x0012, &[0x0401]);
        r
    }

    /// 812.3 W, 3.4 kWh today, 1234.5 kWh in all, 231.2 V, 3.5 A, 50.01 Hz, 41.5 °C.
    fn value_regs() -> Vec<u16> {
        let mut r = vec![0u16; VALUES.1 as usize];
        let s = VALUES.0;
        put(&mut r, s, 0x003c, &[34]);
        put(&mut r, s, 0x003f, &[12345, 0]);
        put(&mut r, s, 0x0041, &[9, 8, 9, 8]);
        put(&mut r, s, 0x0045, &[3100]);
        put(&mut r, s, 0x0047, &[3000]);
        put(&mut r, s, 0x0049, &[2312]);
        put(&mut r, s, 0x004a, &[3150]);
        put(&mut r, s, 0x004c, &[35]);
        put(&mut r, s, 0x004d, &[2995]);
        put(&mut r, s, 0x004f, &[5001]);
        put(&mut r, s, 0x0056, &[8123, 0]);
        put(&mut r, s, 0x005a, &[5150]);
        r
    }

    fn panel_regs() -> Vec<u16> {
        vec![385, 55, 384, 54, 386, 55, 383, 53]
    }

    fn info() -> Info {
        info_of(&Block { start: 0, regs: info_regs() }).unwrap()
    }

    #[test]
    fn frames_a_request() {
        let modbus = read_holding(0x003b, 0x20);
        assert_eq!(&modbus[..6], &[1, 3, 0, 0x3b, 0, 0x20]);
        assert_eq!(crc16(b"123456789"), 0x4b37);
        let f = v5_request(LOGGER, 7, &modbus);
        assert_eq!(f[0], 0xa5);
        assert_eq!(u16::from_le_bytes([f[1], f[2]]) as usize, 15 + modbus.len());
        assert_eq!(&f[3..5], &[0x10, 0x45]);
        assert_eq!(u32::from_le_bytes([f[7], f[8], f[9], f[10]]), LOGGER);
        assert_eq!(&f[26..f.len() - 2], &modbus[..]);
        assert_eq!(*f.last().unwrap(), 0x15);
        assert_eq!(f[f.len() - 2], checksum(&f[1..f.len() - 2]));
    }

    #[test]
    fn checks_an_answer() {
        let a = v5(LOGGER, 1, &rtu(&[1, 2]));
        assert_eq!(registers(v5_answer(&a, LOGGER).unwrap(), 2).unwrap(), vec![1, 2]);
        assert!(v5_answer(&a, LOGGER + 1).is_err(), "another logger");
        let mut bad = a.clone();
        bad[20] ^= 1;
        assert!(v5_answer(&bad, LOGGER).is_err(), "checksum");
        let mut wrong_crc = rtu(&[1, 2]);
        *wrong_crc.last_mut().unwrap() ^= 1;
        assert!(registers(v5_answer(&v5(LOGGER, 1, &wrong_crc), LOGGER).unwrap(), 2).is_err(), "CRC");
        let mut exception = vec![UNIT, 0x83, 2];
        exception.extend_from_slice(&crc16(&exception).to_le_bytes());
        assert!(registers(v5_answer(&v5(LOGGER, 1, &exception), LOGGER).unwrap(), 2).is_err());
        assert!(v5_answer(&v5(LOGGER, 1, &[]), LOGGER).is_err(), "no answer from the inverter");
    }

    #[test]
    fn identifies_the_inverter() {
        let i = info();
        assert_eq!(i, Info { serial: "2403123456".into(), rated_w: Some(2000.0), inputs: 4 });
        assert_eq!(model_of(&i), "Deye micro-onduleur (2 kW, 4 entrées)");
        assert_eq!(devices_of(&i).len(), 5);
        let mut other = info_regs();
        other[0] = 0x0003;
        assert!(matches!(info_of(&Block { start: 0, regs: other }), Err(Error::Unsupported(_))));
    }

    #[test]
    fn inverter_and_panels_with_their_units() {
        let states: std::collections::HashMap<String, State> =
            states_of(&info(), &Block { start: VALUES.0, regs: value_regs() }, &Block { start: PANELS.0, regs: panel_regs() }).into_iter().collect();
        let inv = &states["inv:2403123456"];
        assert_eq!((inv["power"].clone(), inv["energy_today"].clone(), inv["energy_total"].clone()), (json!(812.3), json!(3.4), json!(1234.5)));
        assert_eq!((inv["voltage"].clone(), inv["current"].clone(), inv["frequency"].clone(), inv["temperature"].clone()), (json!(231.2), json!(3.5), json!(50.01), json!(41.5)));
        assert_eq!(inv["panels"], 4);
        let p1 = &states["pv:2403123456:1"];
        assert_eq!((p1["voltage"].clone(), p1["current"].clone(), p1["power"].clone()), (json!(38.5), json!(5.5), json!(211.8)));
        assert_eq!((p1["energy_today"].clone(), p1["energy_total"].clone()), (json!(0.9), json!(310.0)));
        assert_eq!(states["pv:2403123456:3"]["energy_total"], 315.0);
        assert_eq!(states["pv:2403123456:4"]["energy_total"], 299.5);
    }

    const PAGE: &str = r#"<script>var webdata_sn = "2403123456";
var webdata_now_p = "812";
var webdata_today_e = "3.4";
var webdata_total_e = "1234.5";
var cover_mid = "4123456789";</script>"#;

    #[test]
    fn reads_the_web_page() {
        let p = page_of(PAGE).unwrap();
        assert_eq!(p, Page { serial: Some("2403123456".into()), logger: Some(LOGGER), power: Some(812.0), today: Some(3.4), total: Some(1234.5) });
        assert_eq!(page_of("<html>nothing</html>"), None);
    }

    #[test]
    fn through_the_host_with_the_serial_read_from_the_page() {
        testing::reset();
        testing::answer("GET", "/status.html", 200, PAGE);
        assert_eq!(Deye::probe("192.168.1.95").as_deref(), Some("Deye micro-onduleur"));
        let d: Deye = testing::open(json!({"host": "192.168.1.95"}), None);
        testing::queue_exchanges(vec![v5(LOGGER, 1, &rtu(&info_regs())), v5(LOGGER, 2, &rtu(&value_regs())), v5(LOGGER, 3, &rtu(&panel_regs()))]);
        let s = testing::check(&d, MANIFEST);
        assert_eq!(s["inv:2403123456"]["power"], 812.3);
        assert!(testing::requests().iter().any(|r| r.starts_with("EXCHANGE tcp://192.168.1.95:8899")));
        assert!(testing::waited_ms() > 0, "requests are spaced");
    }

    #[test]
    fn the_web_page_when_port_8899_is_closed() {
        testing::reset();
        testing::answer("GET", "/status.html", 200, PAGE);
        let d: Deye = testing::open(json!({"host": "192.168.1.96", "serial": "4123456789"}), None);
        // No exchange answered on the bench: V5 is unreachable.
        let s = testing::check(&d, MANIFEST);
        assert_eq!(s.len(), 1);
        assert_eq!(s["inv:2403123456"]["power"], 812.0);
    }

    #[test]
    fn a_serial_that_is_not_a_number_is_refused() {
        assert!(Deye::open(json!({"host": "192.168.1.96", "serial": "abc"}), None).is_err());
        assert!(Deye::open(json!({"host": "http://x"}), None).is_err());
    }
}
