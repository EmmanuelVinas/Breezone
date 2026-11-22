//! Hoymiles micro-inverters, through their Wi-Fi DTU (DTU-W100, DTU-Lite-S, DTU-WLite…): local
//! protocol on TCP port 10081, protobuf messages framed as
//! `"HM" · command (2) · sequence (2) · CRC-16/MODBUS of the payload (2) · total length (2) · payload`.
//!
//! Read only: `AppInfoData` (the DTU, its inverters and panels) and `RealDataNew` (their values).
//! The DTU also takes commands (power limit, restart…): Breezone sends none. A DTU copes badly with
//! frequent requests: it is read every 30 s, with 2 s at least between two requests. DTUs that
//! encrypt their messages (recent firmware) are recognised but not supported yet.
//!
//! Devices: each inverter (`energy.power@1`, `solar.inverter@1`), which can be given the role of
//! the solar production on the energy page, and each of its panels (`solar.panel@1`).

use breezone_module_sdk::{device, host, json, module, Described, Driver, Error, Found, LengthPrefix, ReadUntil, Result, State, Value};
use std::cell::Cell;

pub const PORT: u16 = 10081;
const APP_INFO: [u8; 2] = [0xa3, 0x01];
const REAL_DATA: [u8; 2] = [0xa3, 0x11];
/// Time zone offset the DTU expects in requests (the official app always sends it).
const OFFSET: u64 = 28_800;
/// Bit of `dtu_info.dfs` set when the DTU encrypts its messages.
const ENCRYPTED_BIT: u32 = 25;
/// Milliseconds between two requests: a DTU copes badly with more.
const SPACING_MS: i64 = 2000;
const TIMEOUT_MS: u32 = 5000;

// ---------------------------------------------------------------------------------------
// Protobuf, just what these messages need
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Pb {
    Int(u64),
    Bytes(Vec<u8>),
}

fn put_varint(out: &mut Vec<u8>, mut n: u64) {
    loop {
        let b = (n & 0x7f) as u8;
        n >>= 7;
        if n == 0 {
            out.push(b);
            return;
        }
        out.push(b | 0x80);
    }
}

fn put_int(out: &mut Vec<u8>, field: u32, value: u64) {
    put_varint(out, u64::from(field) << 3);
    put_varint(out, value);
}

fn put_bytes(out: &mut Vec<u8>, field: u32, value: &[u8]) {
    put_varint(out, (u64::from(field) << 3) | 2);
    put_varint(out, value.len() as u64);
    out.extend_from_slice(value);
}

fn varint(buf: &[u8], i: &mut usize) -> Option<u64> {
    let mut n = 0u64;
    for shift in (0..64).step_by(7) {
        let b = *buf.get(*i)?;
        *i += 1;
        n |= u64::from(b & 0x7f) << shift;
        if b & 0x80 == 0 {
            return Some(n);
        }
    }
    None
}

/// The fields of a message, in order (a repeated field appears several times).
fn fields(buf: &[u8]) -> Option<Vec<(u32, Pb)>> {
    let mut out = vec![];
    let mut i = 0;
    while i < buf.len() {
        let key = varint(buf, &mut i)?;
        let field = (key >> 3) as u32;
        match key & 7 {
            0 => out.push((field, Pb::Int(varint(buf, &mut i)?))),
            2 => {
                let len = varint(buf, &mut i)? as usize;
                out.push((field, Pb::Bytes(buf.get(i..i.checked_add(len)?)?.to_vec())));
                i += len;
            }
            1 => i += 8,
            5 => i += 4,
            _ => return None,
        }
    }
    Some(out)
}

struct Msg(Vec<(u32, Pb)>);

impl Msg {
    fn parse(buf: &[u8]) -> Option<Msg> {
        fields(buf).map(Msg)
    }

    fn int(&self, field: u32) -> Option<u64> {
        self.0.iter().find_map(|(f, v)| match v {
            Pb::Int(n) if *f == field => Some(*n),
            _ => None,
        })
    }

    /// An `int32`, negative values included (sent as 64-bit two's complement).
    fn signed(&self, field: u32) -> Option<i64> {
        self.int(field).map(|n| n as i64)
    }

    fn text(&self, field: u32) -> Option<String> {
        self.0.iter().find_map(|(f, v)| match v {
            Pb::Bytes(b) if *f == field => String::from_utf8(b.clone()).ok(),
            _ => None,
        })
    }

    fn messages(&self, field: u32) -> Vec<Msg> {
        self.0
            .iter()
            .filter_map(|(f, v)| match v {
                Pb::Bytes(b) if *f == field => Msg::parse(b),
                _ => None,
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------------------
// Framing
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

fn frame(command: [u8; 2], sequence: u16, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 10);
    out.extend_from_slice(b"HM");
    out.extend_from_slice(&command);
    out.extend_from_slice(&sequence.to_be_bytes());
    out.extend_from_slice(&crc16(payload).to_be_bytes());
    out.extend_from_slice(&(payload.len() as u16 + 10).to_be_bytes());
    out.extend_from_slice(payload);
    out
}

/// The payload of an answer, checked.
fn unframe(buf: &[u8]) -> Result<&[u8]> {
    if buf.len() < 10 || &buf[..2] != b"HM" {
        return Err(Error::Invalid("not a Hoymiles DTU answer".into()));
    }
    let crc = u16::from_be_bytes([buf[6], buf[7]]);
    let len = u16::from_be_bytes([buf[8], buf[9]]) as usize;
    let payload = buf.get(10..len).ok_or_else(|| Error::Invalid(format!("incomplete answer ({} of {len} bytes)", buf.len())))?;
    if crc16(payload) != crc {
        return Err(Error::Invalid("corrupted answer (CRC)".into()));
    }
    Ok(payload)
}

/// The request body the DTU expects: local time, offset, timestamp (field numbers differ).
fn time_request(ymd: u32, offset: u32, time: u32, extra: &[(u32, u64)]) -> Vec<u8> {
    // A module has no time zone: the time of the request in UTC (the DTU does not use it).
    let now = host::now();
    let shown = chrono::DateTime::from_timestamp(now, 0).map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string()).unwrap_or_default();
    let mut out = vec![];
    put_bytes(&mut out, ymd, shown.as_bytes());
    put_int(&mut out, offset, OFFSET);
    put_int(&mut out, time, now as u64);
    for (f, v) in extra {
        put_int(&mut out, *f, *v);
    }
    out
}

// ---------------------------------------------------------------------------------------
// Models
// ---------------------------------------------------------------------------------------

/// Serial numbers are shown in hexadecimal, as on the label and in the official app.
fn serial_hex(n: u64) -> String {
    format!("{n:X}")
}

fn prefix(serial: &str) -> Option<u16> {
    u16::from_str_radix(serial.get(..4)?, 16).ok()
}

/// "DTU-W100/DTU-Lite-S" from the DTU's serial number.
pub fn dtu_model(serial: &str) -> &'static str {
    match prefix(serial) {
        Some(0x10f7 | 0x10f8 | 0x10fb | 0x10fc | 0x10fd | 0x4100 | 0x4101 | 0x4120 | 0x4121) => "DTU-Pro",
        Some(0x10d3 | 0x10d8 | 0x10d9 | 0x4110 | 0x4111 | 0x4130 | 0x4132 | 0x4133) => "DTU-W100/DTU-Lite-S",
        Some(0x10d2) => "DTU-G100",
        Some(0x10d6 | 0x10d7 | 0x4131) => "DTU-Lite",
        _ => "DTU",
    }
}

/// "HMS-1600/1800/2000-4T" from an inverter's serial number (series, power, inputs).
pub fn inverter_model(serial: &str) -> String {
    let Some(p) = prefix(serial) else { return "Hoymiles".into() };
    let (hi, lo) = ((p >> 8) as u8, p as u8);
    let series = match hi {
        0x10 if lo & 0x03 == 0x02 => "HM",
        0x10 => "HMS",
        0x11 if lo & 0x0f == 0x04 || lo == 0x46 => "HMS",
        0x11 => "HM",
        0x12 | 0x14 => "HMS",
        0x13 => "HMT",
        _ => return "Hoymiles".into(),
    };
    let power = match p {
        0x1011 => "100",
        0x1020 => "250",
        0x1021 | 0x1121 => "300/350/400",
        0x1125 => "400",
        0x1403 => "400/500",
        0x1400 => "450",
        0x1040 => "500",
        0x1041 | 0x1042 | 0x1141 => "600/700/800",
        0x1146 => "600/700/800/900/1000",
        0x1412 => "800/1000",
        0x1144 | 0x1060 | 0x1410 => "1000",
        0x1061 => "1200/1500",
        0x1161 => "1000/1200/1500",
        0x1164 | 0x1422 => "1600/1800/2000",
        0x1222 => "2000DW",
        0x1382 => "2250",
        _ => "",
    };
    let inputs = match (hi, lo) {
        (0x10, 0x14) => "2T",
        (0x11, 0x21 | 0x22 | 0x24 | 0x25) | (0x14, 0x00 | 0x03) => "1T",
        (0x11, 0x41 | 0x42 | 0x44 | 0x46) | (0x14, 0x10 | 0x12) => "2T",
        (0x11, 0x61 | 0x62 | 0x64) | (0x12, 0x22) | (0x14, 0x22) => "4T",
        (0x13, _) => "6T",
        _ => "",
    };
    [series, power, inputs].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("-")
}

/// What answers `AppInfoData` at an address.
#[derive(Debug, Clone, PartialEq)]
pub struct Identified {
    /// DTU serial number ("4133A0054CBB").
    pub serial: String,
    pub model: String,
    pub firmware: Option<String>,
    /// Serial numbers of the inverters.
    pub inverters: Vec<String>,
    pub panels: u64,
    /// Recent firmware: messages encrypted, not supported yet.
    pub encrypted: bool,
}

impl Identified {
    /// "1 micro-onduleur · 4 panneaux".
    pub fn note(&self) -> String {
        let n = self.inverters.len();
        let mut note = format!("{n} micro-onduleur{} · {} panneau{}", if n > 1 { "s" } else { "" }, self.panels, if self.panels > 1 { "x" } else { "" });
        if self.encrypted {
            note += " · chiffré : pas encore pris en charge";
        }
        note
    }
}

fn identify(payload: &[u8]) -> Option<Identified> {
    identify_msg(&Msg::parse(payload)?)
}

fn identify_msg(m: &Msg) -> Option<Identified> {
    let serial = m.text(1).filter(|s| !s.is_empty())?;
    let dtu = m.messages(8).into_iter().next();
    let firmware = dtu.as_ref().and_then(|d| d.text(11)).filter(|s| !s.is_empty());
    let encrypted = dtu.as_ref().and_then(|d| d.int(24)).is_some_and(|dfs| dfs >> ENCRYPTED_BIT & 1 == 1);
    let inverters = m.messages(11).iter().filter_map(|pv| pv.int(2)).map(serial_hex).collect();
    Some(Identified { model: format!("Hoymiles {}", dtu_model(&serial)), serial, firmware, inverters, panels: m.int(4).unwrap_or(0), encrypted })
}

// ---------------------------------------------------------------------------------------
// Values
// ---------------------------------------------------------------------------------------

fn scaled(m: &Msg, field: u32, divisor: f64) -> Option<f64> {
    m.signed(field).map(|v| ((v as f64 / divisor) * 1000.0).round() / 1000.0)
}

/// The state of every inverter and panel, from the `RealDataNew` messages.
fn states_of(answers: &[Msg]) -> Vec<(String, State)> {
    let mut out = vec![];
    let pv: Vec<Msg> = answers.iter().flat_map(|a| a.messages(11)).collect();
    for p in &pv {
        let (Some(serial), Some(port)) = (p.int(1).map(serial_hex), p.int(2)) else { continue };
        let mut s = State::new();
        s.insert("power".into(), json!(scaled(p, 5, 10.0).unwrap_or(0.0)));
        if let Some(v) = scaled(p, 3, 10.0) {
            s.insert("voltage".into(), json!(v));
        }
        if let Some(a) = scaled(p, 4, 100.0) {
            s.insert("current".into(), json!(a));
        }
        s.insert("energy_today".into(), json!(scaled(p, 7, 1000.0).unwrap_or(0.0)));
        s.insert("energy_total".into(), json!(scaled(p, 6, 1000.0).unwrap_or(0.0)));
        s.insert("inverter".into(), json!(serial));
        s.insert("port".into(), json!(port));
        out.push((format!("pv:{serial}:{port}"), s));
    }
    for inv in answers.iter().flat_map(|a| a.messages(9)) {
        let Some(serial) = inv.int(1).map(serial_hex) else { continue };
        let ports: Vec<&Msg> = pv.iter().filter(|p| p.int(1).map(serial_hex).as_deref() == Some(serial.as_str())).collect();
        let kwh = |field: u32| ports.iter().filter_map(|p| p.signed(field)).sum::<i64>() as f64 / 1000.0;
        let mut s = State::new();
        s.insert("power".into(), json!(scaled(&inv, 5, 10.0).unwrap_or(0.0)));
        if let Some(v) = scaled(&inv, 3, 10.0).filter(|v| *v > 0.0) {
            s.insert("voltage".into(), json!(v));
        }
        if let Some(a) = scaled(&inv, 7, 100.0) {
            s.insert("current".into(), json!(a));
        }
        s.insert("energy_today".into(), json!(kwh(7)));
        s.insert("energy_total".into(), json!(kwh(6)));
        if let Some(t) = scaled(&inv, 9, 10.0) {
            s.insert("temperature".into(), json!(t));
        }
        if let Some(f) = scaled(&inv, 4, 100.0).filter(|f| *f > 0.0) {
            s.insert("frequency".into(), json!(f));
        }
        s.insert("panels".into(), json!(ports.len()));
        out.push((format!("inv:{serial}"), s));
    }
    out
}

// ---------------------------------------------------------------------------------------
// Driver
// ---------------------------------------------------------------------------------------

/// The answer of a DTU ends at the total length its header announces (bytes 8 and 9).
fn until() -> ReadUntil {
    ReadUntil::LengthPrefixed(LengthPrefix { offset: 8, size: 2, big_endian: true, adds: 0 })
}

/// Host or `host:port` on the local network: letters, digits, dots, dashes, colons.
fn valid_host(host: &str) -> bool {
    !host.is_empty() && host.len() <= 253 && host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '[' | ']'))
}

pub struct Hoymiles {
    addr: String,
    /// Serial number of the DTU expected at this address (from the scanner).
    device_id: Option<String>,
    /// When the last request was sent (ms, from the host's clock), and its sequence number.
    last: Cell<Option<i64>>,
    sequence: Cell<u16>,
}

impl Hoymiles {
    fn request(&self, command: [u8; 2], payload: &[u8]) -> Result<Msg> {
        // One request at a time, SPACING_MS apart (the host waits, the module does not compute).
        if let Some(last) = self.last.get() {
            let gone = (host::now() * 1000 - last).max(0);
            if gone < SPACING_MS {
                host::wait((SPACING_MS - gone) as u32);
            }
        }
        let sequence = self.sequence.get().wrapping_add(1);
        self.sequence.set(sequence);
        let answer = host::exchange(&format!("tcp://{}", self.addr), &frame(command, sequence, payload), until(), TIMEOUT_MS);
        self.last.set(Some(host::now() * 1000));
        let answer = answer?;
        Msg::parse(unframe(&answer)?).ok_or_else(|| Error::Invalid("unreadable answer (encrypted DTU?)".into()))
    }

    fn real_data(&self) -> Result<Vec<Msg>> {
        let first = self.request(REAL_DATA, &time_request(1, 4, 5, &[(2, 0)]))?;
        let packages = first.int(3).unwrap_or(1);
        let mut out = vec![first];
        for cp in 1..packages.min(16) {
            out.push(self.request(REAL_DATA, &time_request(1, 4, 5, &[(2, cp)]))?);
        }
        Ok(out)
    }
}

impl Driver for Hoymiles {
    fn open(config: Value, _secret: Option<Value>) -> Result<Self> {
        let host = config["host"].as_str().map(str::trim).unwrap_or_default();
        if !valid_host(host) {
            return Err(Error::Invalid("invalid DTU address".into()));
        }
        let addr = if host.contains(':') { host.to_string() } else { format!("{host}:{PORT}") };
        let device_id = config["device_id"].as_str().map(str::trim).filter(|d| !d.is_empty()).map(str::to_string);
        Ok(Hoymiles { addr, device_id, last: Cell::new(None), sequence: Cell::new(0) })
    }

    fn describe(&self) -> Result<Described> {
        let info = self.request(APP_INFO, &time_request(1, 2, 5, &[]))?;
        let found = identify_msg(&info).ok_or_else(|| Error::Invalid("not a Hoymiles DTU".into()))?;
        if self.device_id.as_ref().is_some_and(|expected| *expected != found.serial) {
            return Err(Error::Invalid(format!("another DTU answers at this address ({}): scan the network again", found.serial)));
        }
        if found.encrypted {
            return Err(Error::Unsupported("this DTU encrypts its messages (recent firmware): not supported yet".into()));
        }
        let states = states_of(&self.real_data()?);
        let mut devices = vec![];
        for inv in &found.inverters {
            let short = &inv[inv.len().saturating_sub(4)..];
            devices.push(device(format!("inv:{inv}"), "solar.inverter", &["energy.power@1", "solar.inverter@1"], format!("{} ({short})", inverter_model(inv))));
            let mut ports: Vec<u64> =
                states.iter().filter(|(_, s)| s.get("inverter").and_then(Value::as_str) == Some(inv.as_str())).filter_map(|(_, s)| s.get("port")?.as_u64()).collect();
            ports.sort_unstable();
            for port in ports {
                devices.push(device(format!("pv:{inv}:{port}"), "solar.panel", &["solar.panel@1"], format!("Panneau {port} ({short})")));
            }
        }
        let model = format!("{} ({})", found.model, found.note());
        Ok(Described { model, firmware: found.firmware, devices, identity: Some(found.serial) })
    }

    fn read(&self) -> Result<Vec<(String, State)>> {
        Ok(states_of(&self.real_data()?))
    }

    fn act(&self, reference: &str, trait_name: &str, action: &str, _params: &State) -> Result<()> {
        Err(Error::Unsupported(format!("{reference}: {trait_name}.{action}")))
    }

    fn diagnostic(&self) -> Result<Value> {
        let info = self.request(APP_INFO, &time_request(1, 2, 5, &[]))?;
        let found = identify_msg(&info).ok_or_else(|| Error::Invalid("not a Hoymiles DTU".into()))?;
        Ok(json!({"model": found.model, "firmware": found.firmware, "inverters": found.inverters.len(), "panels": found.panels, "encrypted": found.encrypted}))
    }

    fn identify(host_: &str, announced: Option<&Value>) -> Option<Found> {
        if announced.is_some() {
            return None;
        }
        let addr = if host_.contains(':') { host_.to_string() } else { format!("{host_}:{PORT}") };
        let answer = host::exchange(&format!("tcp://{addr}"), &frame(APP_INFO, 1, &time_request(1, 2, 5, &[])), until(), 2000).ok()?;
        let h = identify(unframe(&answer).ok()?)?;
        Some(Found {
            note: Some(h.note()),
            unsupported: h.encrypted,
            device_id: Some(h.serial.clone()),
            firmware: h.firmware.clone(),
            model: h.model,
            port: Some(PORT),
            ..Found::default()
        })
    }

    fn probe(host_: &str) -> Option<String> {
        let addr = if host_.contains(':') { host_.to_string() } else { format!("{host_}:{PORT}") };
        let answer = host::exchange(&format!("tcp://{addr}"), &frame(APP_INFO, 1, &time_request(1, 2, 5, &[])), until(), 2000).ok()?;
        identify(unframe(&answer).ok()?).map(|f| f.model)
    }
}

module!(Hoymiles);

#[cfg(test)]
mod tests {
    use super::*;
    use breezone_module_sdk::testing;

    const MANIFEST: &str = include_str!("../manifest.json");

    fn msg(fields: &[(u32, Pb)]) -> Vec<u8> {
        let mut out = vec![];
        for (f, v) in fields {
            match v {
                Pb::Int(n) => put_int(&mut out, *f, *n),
                Pb::Bytes(b) => put_bytes(&mut out, *f, b),
            }
        }
        out
    }

    fn i(n: i64) -> Pb {
        Pb::Int(n as u64)
    }

    /// The real answers of a DTU-W100 with one HMS-1600-4T and four panels (values as read).
    fn app_info() -> Vec<u8> {
        let dtu = msg(&[(2, i(1537)), (3, i(25089)), (8, i(1794653406)), (9, i(100)), (11, Pb::Bytes(b"2.3.0.0-dev".to_vec()))]);
        let pv = msg(&[(2, i(19123694208274)), (3, i(101)), (4, i(10027))]);
        msg(&[(1, Pb::Bytes(b"4133A0054CBB".to_vec())), (2, i(1794653457)), (3, i(1)), (4, i(4)), (8, Pb::Bytes(dtu)), (11, Pb::Bytes(pv))])
    }

    fn real_data() -> Vec<u8> {
        let sgs = msg(&[(1, i(19123694208274)), (2, i(1)), (3, i(2322)), (4, i(5000)), (5, i(4869)), (7, i(209)), (8, i(1000)), (9, i(251)), (10, i(1)), (12, i(1)), (20, i(-64))]);
        let pv = |port: i64, v: i64, a: i64, w: i64, total: i64, day: i64| {
            Pb::Bytes(msg(&[(1, i(19123694208274)), (2, i(port)), (3, i(v)), (4, i(a)), (5, i(w)), (6, i(total)), (7, i(day)), (8, i(50986752))]))
        };
        msg(&[
            (1, Pb::Bytes(b"4133A0054CBB".to_vec())),
            (2, i(1794653459)),
            (3, i(1)),
            (5, i(1)),
            (9, Pb::Bytes(sgs)),
            (11, pv(1, 402, 390, 1571, 1176763, 338)),
            (11, pv(2, 403, 387, 1562, 1176453, 337)),
            (11, pv(3, 399, 249, 996, 957769, 224)),
            (11, pv(4, 400, 248, 995, 956530, 224)),
        ])
    }

    #[test]
    fn frames_and_checks_messages() {
        let payload = msg(&[(1, Pb::Bytes(b"x".to_vec())), (2, Pb::Int(28800))]);
        let f = frame(APP_INFO, 7, &payload);
        assert_eq!(&f[..4], b"HM\xa3\x01");
        assert_eq!(u16::from_be_bytes([f[8], f[9]]) as usize, f.len());
        // An answer has the same shape.
        assert_eq!(unframe(&f).unwrap(), &payload[..]);
        let mut bad = f.clone();
        *bad.last_mut().unwrap() ^= 1;
        assert!(unframe(&bad).is_err(), "CRC");
        assert!(unframe(&f[..f.len() - 1]).is_err(), "incomplete");
        assert_eq!(crc16(b"123456789"), 0x4b37, "CRC-16/MODBUS check value");
    }

    #[test]
    fn identifies_the_dtu_and_its_inverters() {
        let found = identify(&app_info()).unwrap();
        assert_eq!(found.serial, "4133A0054CBB");
        assert_eq!(found.model, "Hoymiles DTU-W100/DTU-Lite-S");
        assert_eq!(found.firmware.as_deref(), Some("2.3.0.0-dev"));
        assert_eq!(found.inverters, vec!["116495000512"]);
        assert_eq!(found.note(), "1 micro-onduleur · 4 panneaux");
        assert!(!found.encrypted);
        assert_eq!(inverter_model("116495000512"), "HMS-1600/1800/2000-4T");
        assert_eq!(inverter_model("114172220001"), "HM-600/700/800-2T");
        assert_eq!(identify(b"\x08\x01"), None);
    }

    #[test]
    fn an_encrypting_dtu_is_recognised() {
        let dtu = msg(&[(24, Pb::Int(1 << ENCRYPTED_BIT))]);
        let found = identify(&msg(&[(1, Pb::Bytes(b"4143A0000001".to_vec())), (8, Pb::Bytes(dtu))])).unwrap();
        assert!(found.encrypted);
        assert!(found.note().contains("chiffré"));
    }

    #[test]
    fn inverter_and_panels_with_their_units() {
        let states: std::collections::HashMap<String, State> = states_of(&[Msg::parse(&real_data()).unwrap()]).into_iter().collect();
        let inv = &states["inv:116495000512"];
        assert_eq!(inv["power"], 486.9);
        assert_eq!((inv["voltage"].clone(), inv["frequency"].clone(), inv["temperature"].clone()), (json!(232.2), json!(50.0), json!(25.1)));
        assert_eq!(inv["current"], 2.09);
        assert_eq!(inv["panels"], 4);
        assert_eq!(inv["energy_today"], 1.123, "338 + 337 + 224 + 224 Wh");
        assert_eq!(inv["energy_total"], 4267.515);
        let p1 = &states["pv:116495000512:1"];
        assert_eq!((p1["power"].clone(), p1["voltage"].clone(), p1["current"].clone()), (json!(157.1), json!(40.2), json!(3.9)));
        assert_eq!((p1["energy_today"].clone(), p1["energy_total"].clone()), (json!(0.338), json!(1176.763)));
        assert_eq!(states.len(), 5);
    }

    #[test]
    fn decodes_negative_values() {
        let m = Msg::parse(&msg(&[(9, i(-35))])).unwrap();
        assert_eq!(scaled(&m, 9, 10.0), Some(-3.5));
    }

    #[test]
    fn read_through_the_host_two_seconds_apart() {
        testing::reset();
        let (info, data) = (frame(APP_INFO, 1, &app_info()), frame(REAL_DATA, 2, &real_data()));
        testing::queue_exchanges(vec![info.clone()]);
        assert_eq!(Hoymiles::probe("192.168.1.181").as_deref(), Some("Hoymiles DTU-W100/DTU-Lite-S"));
        let h: Hoymiles = testing::open(json!({"host": "192.168.1.181"}), None);
        // describe: AppInfoData then RealDataNew; read: RealDataNew.
        testing::queue_exchanges(vec![info, data.clone(), data]);
        let s = testing::check(&h, MANIFEST);
        assert!(s.keys().any(|k| k.starts_with("inv:")) && s.keys().any(|k| k.starts_with("pv:")), "{s:?}");
        assert!(testing::waited_ms() > 0, "requests are spaced");
        assert!(testing::requests().iter().all(|r| r.starts_with("EXCHANGE tcp://192.168.1.181:10081")));
    }
}
