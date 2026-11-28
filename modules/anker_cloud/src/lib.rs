//! Anker SOLIX's cloud, for the SolarBank 2 and 3: Anker gives them no local access. The API is
//! the one of the Anker app (not documented by Anker), as `anker-solix-api` uses it.
//!
//!   POST passport/login {ab, client_secret_info: {public_key}, enc: 0, email, password, time_zone, transaction}
//!        → {code, data: {user_id, auth_token, token_expires_at}}
//!        the password AES-256-CBC with the ECDH secret of a key of ours and Anker's (P-256), IV its
//!        first 16 bytes; then `x-auth-token` and `gtoken` (MD5 of the user id) in every request.
//!   POST power_service/v1/site/get_site_list {} → {data: {site_list: [{site_id, site_name}]}}
//!   POST power_service/v1/site/get_scen_info {site_id} → {data: {solarbank_info: {solarbank_list: [
//!        {device_pn, device_sn, device_name, battery_power (%), bat_charge_power, photovoltaic_power,
//!        solar_power_1…4 (each input), output_power}], battery_discharge_power}}}
//!
//! An account logged in here may log the Anker app out: a second account, with which the system is
//! shared, avoids it. Read only, every minute (the cloud refreshes a SolarBank 2 every few minutes).
//!
//! The panels wired to a SolarBank are shown as an inverter and its inputs. The cloud tells their
//! power, not what they gave today: it is counted here from each read, since midnight (from the
//! start of the module on its first day).

use aes::cipher::{BlockModeEncrypt, KeyIvInit};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use breezone_module_sdk::{device, host, json, module, Described, DeviceInfo, Driver, Error, Result, Service, State, Value};
use md5::{Digest, Md5};
use std::cell::RefCell;

/// The public key of Anker's servers (the same for both regions).
const SERVER_KEY: &str = "04c5c00c4f8d1197cc7c3167c52bf7acb054d722f0ef08dcd7e0883236e0d72a3868d9750cb47fa4619248f3d83f0f662671dadc6e2d31c2f41db0161651c7c076";
const EU: &str = "https://ankerpower-api-eu.anker.com";
const COM: &str = "https://ankerpower-api.anker.com";
/// The countries of the other server; every other one goes to Europe's.
const COM_COUNTRIES: &[&str] = &["DZ", "LB", "SY", "EG", "LY", "TN", "MA", "JO", "PS", "AR", "AU", "BR", "HK", "IN", "MX", "NG", "NZ", "RU", "ZA", "KR", "TW", "US", "CA"];
const TIMEOUT_MS: u32 = 20_000;
/// The sites (and their devices) are listed again this often.
const LIST_EVERY: i64 = 3600;
/// Below this, the battery neither charges nor discharges.
const IDLE_W: f64 = 5.0;
/// Two reads further apart than this are not counted in today's energy.
const MAX_GAP_S: i64 = 900;

fn md5_hex(s: &str) -> String {
    Md5::digest(s.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

fn unreachable(e: impl std::fmt::Display) -> Error {
    Error::Unreachable(format!("Anker : {e}"))
}

fn server_of(country: &str) -> &'static str {
    if COM_COUNTRIES.contains(&country) {
        COM
    } else {
        EU
    }
}

/// "GMT+01:00" and the offset in milliseconds, in the time zone of the server, now.
fn time_zone(tz: chrono_tz::Tz) -> (String, i64) {
    use chrono::{Offset, TimeZone};
    let now = chrono::DateTime::from_timestamp(host::now(), 0).unwrap_or_default();
    let seconds = i64::from(tz.offset_from_utc_datetime(&now.naive_utc()).fix().local_minus_utc());
    let (sign, abs) = if seconds < 0 { ('-', -seconds) } else { ('+', seconds) };
    (format!("GMT{sign}{:02}:{:02}", abs / 3600, abs % 3600 / 60), seconds * 1000)
}

// ---------------------------------------------------------------------------------------
// The password, encrypted for Anker
// ---------------------------------------------------------------------------------------

/// Our public key (uncompressed, in hex) and the password encrypted with the secret shared with
/// Anker's servers.
pub fn encrypt_password(secret: &p256::SecretKey, password: &str) -> std::result::Result<(String, String), String> {
    let server = p256::PublicKey::from_sec1_bytes(&hex::decode(SERVER_KEY).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let shared = p256::ecdh::diffie_hellman(secret.to_nonzero_scalar(), server.as_affine());
    let key: [u8; 32] = shared.raw_secret_bytes().as_slice().try_into().map_err(|_| "shared secret")?;
    let iv: [u8; 16] = key[..16].try_into().map_err(|_| "iv")?;
    let len = password.len();
    let mut buf = password.as_bytes().to_vec();
    buf.resize(len + 16 - len % 16, 0);
    let cipher = cbc::Encryptor::<aes::Aes256>::new(&key.into(), &iv.into()).encrypt_padded::<cbc::cipher::block_padding::Pkcs7>(&mut buf, len).map_err(|_| "padding")?.to_vec();
    let public = hex::encode(p256::EncodedPoint::from(secret.public_key()).as_bytes());
    Ok((public, STANDARD.encode(cipher)))
}

fn new_secret() -> Result<p256::SecretKey> {
    for _ in 0..8 {
        let mut b = [0u8; 32];
        getrandom::fill(&mut b).map_err(|e| unreachable(format!("random: {e}")))?;
        if let Ok(k) = p256::SecretKey::from_slice(&b) {
            return Ok(k);
        }
    }
    Err(unreachable("no key"))
}

// ---------------------------------------------------------------------------------------
// The session
// ---------------------------------------------------------------------------------------

struct Session {
    email: String,
    password: String,
    country: String,
    token: Option<String>,
    gtoken: Option<String>,
    expires: Option<i64>,
    tz: chrono_tz::Tz,
}

impl Session {
    fn load(tz: chrono_tz::Tz) -> Result<Self> {
        let a = host::account("anker_cloud")?;
        let s = |k: &str| a[k].as_str().map(str::trim).filter(|v| !v.is_empty()).map(str::to_string);
        let (Some(email), Some(password)) = (s("email"), a["password"].as_str().filter(|p| !p.is_empty()).map(str::to_string)) else {
            return Err(Error::Invalid("l'e-mail et le mot de passe du compte Anker sont requis".into()));
        };
        let country = s("country").map(|c| c.to_uppercase()).unwrap_or_else(|| "FR".into());
        if country.len() != 2 || !country.chars().all(|c| c.is_ascii_uppercase()) {
            return Err(Error::Invalid("le pays du compte s'écrit en deux lettres (FR, BE, CH…)".into()));
        }
        Ok(Session { email, password, country, token: s("auth_token"), gtoken: s("gtoken"), expires: a["token_expires_at"].as_i64(), tz })
    }

    fn keep(&self) -> Result<()> {
        host::account_update(&json!({"auth_token": self.token, "gtoken": self.gtoken, "token_expires_at": self.expires}))
    }

    fn base(&self) -> &'static str {
        server_of(&self.country)
    }

    fn send(&self, path: &str, body: &Value) -> Result<(u16, Value)> {
        let (gmt, _) = time_zone(self.tz);
        let mut headers: Vec<(&str, String)> = vec![
            ("Content-Type", "application/json".into()),
            ("model-type", "DESKTOP".into()),
            ("app-name", "anker_power".into()),
            ("os-type", "android".into()),
            ("country", self.country.clone()),
            ("timezone", gmt),
        ];
        if let (Some(t), Some(g)) = (&self.token, &self.gtoken) {
            headers.push(("x-auth-token", t.clone()));
            headers.push(("gtoken", g.clone()));
        }
        let refs: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let a = host::request("POST", &format!("{}/{path}", self.base()), &refs, Some(body.to_string().as_bytes()), TIMEOUT_MS)?;
        let v = a.json().unwrap_or(Value::Null);
        Ok((a.status, v))
    }

    fn login(&mut self) -> Result<()> {
        self.token = None;
        self.gtoken = None;
        let secret = new_secret()?;
        let (public, password) = encrypt_password(&secret, &self.password).map_err(unreachable)?;
        let (_, offset_ms) = time_zone(self.tz);
        let body = json!({
            "ab": self.country,
            "client_secret_info": {"public_key": public},
            "enc": 0,
            "email": self.email,
            "password": password,
            "time_zone": offset_ms,
            "transaction": (host::now() * 1000).to_string(),
        });
        let (status, answer) = self.send("passport/login", &body)?;
        match code_of(&answer) {
            Some(0) => {
                let d = &answer["data"];
                self.token = d["auth_token"].as_str().map(str::to_string);
                self.gtoken = d["user_id"].as_str().map(md5_hex);
                self.expires = d["token_expires_at"].as_i64();
                if self.token.is_none() || self.gtoken.is_none() {
                    return Err(unreachable("pas de session"));
                }
                Ok(())
            }
            Some(26108 | 26156) => Err(Error::Invalid("Anker refuse l'e-mail ou le mot de passe".into())),
            Some(26050..=26055) => {
                Err(Error::Invalid("Anker demande un code de vérification : connectez-vous une fois à ce compte dans l'application Anker, puis réessayez".into()))
            }
            Some(100053) => Err(Error::Invalid("trop d'essais de connexion : réessayez dans une heure".into())),
            Some(code) => Err(unreachable(format!("connexion refusée ({code} : {})", answer["msg"].as_str().unwrap_or_default()))),
            None => Err(unreachable(format!("HTTP {status}"))),
        }
    }

    fn fresh(&self) -> bool {
        self.token.is_some() && self.gtoken.is_some() && self.expires.is_none_or(|e| e - host::now() > 60)
    }
}

fn code_of(v: &Value) -> Option<i64> {
    v["code"].as_i64().or_else(|| v["code"].as_str()?.parse().ok())
}

/// The session is over: logged out by another login, or expired.
fn expired(status: u16, body: &Value) -> bool {
    matches!(status, 401 | 403) || matches!(code_of(body), Some(401 | 403 | 26084))
}

/// A request of the API, logging in first when the session is over; the session kept when it changed.
fn call(tz: chrono_tz::Tz, path: &str, body: &Value) -> Result<Value> {
    let mut s = Session::load(tz)?;
    let mut changed = false;
    if !s.fresh() {
        s.login()?;
        changed = true;
    }
    let (mut status, mut answer) = s.send(path, body)?;
    if expired(status, &answer) {
        s.login()?;
        changed = true;
        (status, answer) = s.send(path, body)?;
    }
    if changed {
        s.keep()?;
    }
    match code_of(&answer) {
        Some(0) => Ok(answer["data"].clone()),
        Some(code) => Err(unreachable(format!("{path} : {code} {}", answer["msg"].as_str().unwrap_or_default()))),
        None => Err(unreachable(format!("{path} : HTTP {status}"))),
    }
}

// ---------------------------------------------------------------------------------------
// SolarBanks
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Bank {
    pub site: String,
    pub sn: String,
    pub pn: String,
    pub name: String,
    /// The solar inputs whose power the cloud tells (1 to 4).
    pub ports: Vec<usize>,
}

/// A number the cloud gives as text ("75", "0.00", "") or as a number.
fn num(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str()?.trim().parse().ok())
}

pub fn model_of(pn: &str) -> &'static str {
    match pn {
        "A17C0" => "Solarbank 2 E1600 Pro",
        "A17C1" => "Solarbank 2 E1600 Plus",
        "A17C2" => "Solarbank 2 E1600 AC",
        "A17C3" => "Solarbank 2 E1600",
        "A17C5" => "Solarbank 3 E2700 Pro",
        "A17C4" => "Solarbank 3 E2700",
        _ => "Solarbank",
    }
}

/// The solar inputs of a model, when known (each input has its `solar_power_<n>`).
fn inputs_of(pn: &str) -> Option<usize> {
    match pn {
        "A17C0" => Some(4),
        "A17C1" | "A17C2" => Some(2),
        _ => None,
    }
}

/// The inputs the cloud tells the power of, up to those of the model.
fn ports_of(pn: &str, bank: &Value) -> Vec<usize> {
    let told: Vec<usize> = (1..=4).filter(|n| num(&bank[format!("solar_power_{n}")]).is_some()).collect();
    match inputs_of(pn) {
        Some(max) => told.into_iter().filter(|n| *n <= max).collect(),
        None => told,
    }
}

fn banks_of(site: &str, scene: &Value) -> Vec<Bank> {
    scene["solarbank_info"]["solarbank_list"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|b| {
            let sn = b["device_sn"].as_str().filter(|s| !s.is_empty())?.to_string();
            let pn = b["device_pn"].as_str().unwrap_or_default().to_string();
            let name = b["device_name"].as_str().filter(|n| !n.trim().is_empty()).map(str::to_string).unwrap_or_else(|| model_of(&pn).to_string());
            let ports = ports_of(&pn, b);
            Some(Bank { site: site.to_string(), sn, pn, name, ports })
        })
        .collect()
}

fn devices_of(banks: &[Bank]) -> Vec<DeviceInfo> {
    let mut out = vec![];
    for b in banks {
        out.push(device(format!("sb:{}", b.sn), "energy.battery", &["energy.battery@1", "energy.power@1"], b.name.clone()));
        out.push(device(format!("pv:{}", b.sn), "solar.inverter", &["energy.power@1", "solar.inverter@1"], format!("{} · panneaux", b.name)));
        for port in &b.ports {
            out.push(device(format!("pv:{}:{port}", b.sn), "solar.panel", &["solar.panel@1"], format!("Panneau {port} ({})", b.name)));
        }
    }
    out
}

/// What a device gave today, counted from the power of each read.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Counter {
    day: Option<chrono::NaiveDate>,
    kwh: f64,
    last: Option<(i64, f64)>,
}

impl Counter {
    /// Adds the energy since the previous read (the mean of both powers), then gives today's.
    pub fn add(&mut self, tz: chrono_tz::Tz, ts: i64, watts: f64) -> f64 {
        let day = chrono::DateTime::from_timestamp(ts, 0).map(|t| t.with_timezone(&tz).date_naive());
        if self.day != day {
            *self = Counter { day, ..Counter::default() };
        } else if let Some((at, w)) = self.last.filter(|(at, _)| (1..=MAX_GAP_S).contains(&(ts - at))) {
            self.kwh += (w + watts) / 2.0 * (ts - at) as f64 / 3_600_000.0;
        }
        self.last = Some((ts, watts));
        (self.kwh * 1000.0).round() / 1000.0
    }
}

/// The battery (positive power: it charges, as the energy page counts a battery) and the panels
/// wired to it, as an inverter and its inputs; `today` gives what a device gave today from its
/// power now.
fn states_of(bank: &Bank, scene: &Value, today: &mut dyn FnMut(&str, f64) -> f64) -> Vec<(String, State)> {
    let info = &scene["solarbank_info"];
    let Some(b) = info["solarbank_list"].as_array().into_iter().flatten().find(|b| b["device_sn"].as_str() == Some(bank.sn.as_str())) else { return vec![] };
    let single = info["solarbank_list"].as_array().map_or(0, Vec::len) == 1;
    let charge = num(&b["bat_charge_power"]).or_else(|| num(&b["charging_power"])).unwrap_or(0.0);
    // The discharge is told for the site: it is this battery's when it is alone there.
    let discharge = if single { num(&info["battery_discharge_power"]).unwrap_or(0.0) } else { 0.0 };
    let power = charge - discharge;
    let solar = num(&b["photovoltaic_power"]);
    let mut battery = State::new();
    if let Some(soc) = num(&b["battery_power"]) {
        battery.insert("soc".into(), json!(soc.round().clamp(0.0, 100.0) as i64));
    }
    battery.insert("power".into(), json!(power));
    battery.insert(
        "status".into(),
        json!(if power > IDLE_W {
            "charging"
        } else if power < -IDLE_W {
            "discharging"
        } else {
            "idle"
        }),
    );
    if let Some(w) = solar {
        battery.insert("solar_power".into(), json!(w.max(0.0)));
    }
    if let Some(w) = num(&b["output_power"]) {
        battery.insert("output_power".into(), json!(w.max(0.0)));
    }
    let solar = solar.unwrap_or(0.0).max(0.0);
    let inverter = format!("pv:{}", bank.sn);
    let mut panels = State::new();
    panels.insert("power".into(), json!(solar));
    panels.insert("energy_today".into(), json!(today(&inverter, solar)));
    panels.insert("panels".into(), json!(bank.ports.len()));
    let mut out = vec![(format!("sb:{}", bank.sn), battery), (inverter, panels)];
    for port in &bank.ports {
        let reference = format!("pv:{}:{port}", bank.sn);
        let w = num(&b[format!("solar_power_{port}")]).unwrap_or(0.0).max(0.0);
        let mut s = State::new();
        s.insert("power".into(), json!(w));
        s.insert("energy_today".into(), json!(today(&reference, w)));
        s.insert("inverter".into(), json!(bank.sn));
        s.insert("port".into(), json!(port));
        out.push((reference, s));
    }
    out
}

fn sites(tz: chrono_tz::Tz) -> Result<Vec<(String, String)>> {
    let data = call(tz, "power_service/v1/site/get_site_list", &json!({}))?;
    Ok(data["site_list"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| Some((s["site_id"].as_str()?.to_string(), s["site_name"].as_str().unwrap_or_default().to_string())))
        .collect())
}

fn scene(tz: chrono_tz::Tz, site: &str) -> Result<Value> {
    call(tz, "power_service/v1/site/get_scen_info", &json!({"site_id": site}))
}

// ---------------------------------------------------------------------------------------
// Driver and account
// ---------------------------------------------------------------------------------------

pub struct AnkerCloud {
    tz: chrono_tz::Tz,
    banks: RefCell<(Vec<Bank>, Option<i64>)>,
    today: RefCell<std::collections::HashMap<String, Counter>>,
}

impl AnkerCloud {
    fn load(&self) -> Result<Vec<Bank>> {
        let mut banks = vec![];
        for (site, _) in sites(self.tz)? {
            banks.extend(banks_of(&site, &scene(self.tz, &site)?));
        }
        *self.banks.borrow_mut() = (banks.clone(), Some(host::now()));
        Ok(banks)
    }

    fn banks(&self) -> Result<Vec<Bank>> {
        if self.banks.borrow().1.is_some_and(|t| host::now() - t < LIST_EVERY) {
            return Ok(self.banks.borrow().0.clone());
        }
        self.load()
    }
}

fn tz_of(config: &Value) -> chrono_tz::Tz {
    config["timezone"].as_str().and_then(|t| t.parse().ok()).unwrap_or(chrono_tz::Europe::Paris)
}

impl Driver for AnkerCloud {
    fn open(config: Value, _secret: Option<Value>) -> Result<Self> {
        Ok(AnkerCloud { tz: tz_of(&config), banks: RefCell::new((vec![], None)), today: RefCell::new(Default::default()) })
    }

    fn describe(&self) -> Result<Described> {
        let banks = self.load()?;
        let model = match banks.first() {
            Some(b) if banks.iter().all(|x| x.pn == b.pn) => format!("Anker SOLIX {}", model_of(&b.pn)),
            _ => "Anker SOLIX".into(),
        };
        Ok(Described { model, firmware: None, devices: devices_of(&banks), identity: None })
    }

    fn read(&self) -> Result<Vec<(String, State)>> {
        let banks = self.banks()?;
        let mut out = vec![];
        let mut sites: Vec<&str> = banks.iter().map(|b| b.site.as_str()).collect();
        sites.dedup();
        for site in sites {
            let s = scene(self.tz, site)?;
            let now = host::now();
            let mut counters = self.today.borrow_mut();
            let mut today = |reference: &str, w: f64| counters.entry(reference.to_string()).or_default().add(self.tz, now, w);
            for b in banks.iter().filter(|b| b.site == site) {
                out.extend(states_of(b, &s, &mut today));
            }
        }
        Ok(out)
    }

    fn act(&self, reference: &str, trait_name: &str, action: &str, _params: &State) -> Result<()> {
        Err(Error::Unsupported(format!("{reference}: {trait_name}.{action}")))
    }

    fn diagnostic(&self) -> Result<Value> {
        // What tells the hardware and its values apart, not who owns it.
        let mut out = vec![];
        for (site, _) in sites(self.tz)? {
            let s = scene(self.tz, &site)?;
            let info = &s["solarbank_info"];
            let banks: Vec<Value> = info["solarbank_list"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|b| {
                    let mut b = b.clone();
                    for k in ["device_sn", "device_name", "device_img"] {
                        if let Some(o) = b.as_object_mut() {
                            o.remove(k);
                        }
                    }
                    b
                })
                .collect();
            out.push(json!({
                "solarbanks": banks,
                "battery_discharge_power": info["battery_discharge_power"],
                "to_home_load": info["to_home_load"],
                "grid_to_battery_power": info["grid_to_battery_power"],
                "updated_time": info["updated_time"],
                "scene_mode": s["scene_mode"],
                "home_load_power": s["home_load_power"],
                // The Smart Meter and the plugs of the site, and its totals: to read them later.
                "grid_info": anonymous(&s["grid_info"]),
                "smart_plug_info": anonymous(&s["smart_plug_info"]),
                "statistics": s["statistics"],
            }));
        }
        Ok(json!({"sites": out}))
    }
}

/// A value of the cloud without what tells who owns it (serial numbers, names, pictures).
fn anonymous(v: &Value) -> Value {
    match v {
        Value::Object(o) => {
            let mut o = o.clone();
            o.retain(|k, _| !["device_sn", "device_name", "device_img", "site_id", "site_name"].contains(&k.as_str()));
            o.values_mut().for_each(|v| *v = anonymous(v));
            Value::Object(o)
        }
        Value::Array(a) => a.iter().map(anonymous).collect::<Vec<_>>().into(),
        _ => v.clone(),
    }
}

/// The account: a login, then what it reaches; its session is kept, its password too (Anker asks
/// it again at each login, encrypted for this one).
pub struct Account;

impl Service for Account {
    fn check() -> Result<()> {
        let tz = tz_of(&host::account("anker_cloud")?);
        let mut s = Session::load(tz)?;
        s.login()?;
        s.keep()?;
        let mut banks = 0;
        let mut names = vec![];
        for (site, name) in sites(tz)? {
            banks += banks_of(&site, &scene(tz, &site)?).len();
            names.push(json!(name));
        }
        host::account_update(&json!({"shown": {"sites": names, "solarbanks": banks}}))
    }

    fn query(contract: &str, query: &str, _params: Value) -> Result<Value> {
        Err(Error::Unsupported(format!("{contract} {query}")))
    }
}

module!(driver: AnkerCloud, service: Account);

#[cfg(test)]
mod tests {
    use super::*;
    use breezone_module_sdk::testing;

    const MANIFEST: &str = include_str!("../manifest.json");

    /// The scene of a site with a Solarbank 2 AC at 75 %, charging 120 W from its 300 W of panels.
    fn scene_json() -> Value {
        json!({"code": 0, "data": {"solarbank_info": {
            "solarbank_list": [{"device_pn": "A17C2", "device_sn": "SB2AC0001", "device_name": "Garage", "battery_power": "75",
                                "bat_charge_power": "120", "charging_power": "120", "photovoltaic_power": "300", "output_power": "180",
                                "solar_power_1": "170", "solar_power_2": "130", "solar_power_3": "0", "solar_power_4": ""}],
            "battery_discharge_power": "0", "to_home_load": "180"}, "scene_mode": 1}})
    }

    fn cloud() {
        testing::answer_json("POST", "passport/login", &json!({"code": 0, "data": {"user_id": "u1", "auth_token": "t1", "token_expires_at": 4_102_444_800i64}}));
        testing::answer_json("POST", "get_site_list", &json!({"code": 0, "data": {"site_list": [{"site_id": "s1", "site_name": "Maison"}]}}));
        testing::answer_json("POST", "get_scen_info", &scene_json());
    }

    #[test]
    fn the_password_is_encrypted_as_openssl_does() {
        // A fixed key of ours; openssl gives the same secret with Anker's public key:
        //   openssl pkeyutl -derive -inkey ours.pem -peerkey anker.pem | xxd -p
        let secret = p256::SecretKey::from_slice(&[7u8; 32]).unwrap();
        let (public, cipher) = encrypt_password(&secret, "motdepasse").unwrap();
        assert_eq!(public.len(), 130);
        assert!(public.starts_with("04"));
        assert_eq!(cipher, include_str!("../tests/password.b64").trim());
    }

    #[test]
    fn a_solarbank_and_its_panels() {
        let banks = banks_of("s1", &scene_json()["data"]);
        assert_eq!(banks, vec![Bank { site: "s1".into(), sn: "SB2AC0001".into(), pn: "A17C2".into(), name: "Garage".into(), ports: vec![1, 2] }], "an AC has two inputs");
        assert_eq!(devices_of(&banks).len(), 4);
        let states: std::collections::HashMap<String, State> = states_of(&banks[0], &scene_json()["data"], &mut |_, _| 0.0).into_iter().collect();
        let b = &states["sb:SB2AC0001"];
        assert_eq!((b["soc"].clone(), b["power"].clone(), b["status"].clone()), (json!(75), json!(120.0), json!("charging")));
        assert_eq!((b["solar_power"].clone(), b["output_power"].clone()), (json!(300.0), json!(180.0)));
        assert_eq!((states["pv:SB2AC0001"]["power"].clone(), states["pv:SB2AC0001"]["panels"].clone()), (json!(300.0), json!(2)));
        let panel = &states["pv:SB2AC0001:1"];
        assert_eq!((panel["power"].clone(), panel["port"].clone(), panel["inverter"].clone()), (json!(170.0), json!(1), json!("SB2AC0001")));
        assert_eq!(states["pv:SB2AC0001:2"]["power"], 130.0);
    }

    #[test]
    fn today_counts_the_power_of_each_read_and_starts_again_at_midnight() {
        let paris = chrono_tz::Europe::Paris;
        // 2026-06-01 10:00 in Paris.
        let t = 1_780_300_800;
        let mut c = Counter::default();
        assert_eq!(c.add(paris, t, 600.0), 0.0);
        assert_eq!(c.add(paris, t + 60, 600.0), 0.01, "600 W for a minute");
        assert_eq!(c.add(paris, t + 60 + 3600, 0.0), 0.01, "a gap is not counted");
        assert_eq!(c.add(paris, t + 14 * 3600, 100.0), 0.0, "the next day");
    }

    #[test]
    fn a_discharging_battery_has_a_negative_power() {
        let mut s = scene_json()["data"].clone();
        s["solarbank_info"]["solarbank_list"][0]["bat_charge_power"] = json!("0");
        s["solarbank_info"]["solarbank_list"][0]["charging_power"] = json!("0");
        s["solarbank_info"]["battery_discharge_power"] = json!("250");
        let states: std::collections::HashMap<String, State> = states_of(&banks_of("s1", &s)[0], &s, &mut |_, _| 0.0).into_iter().collect();
        assert_eq!((states["sb:SB2AC0001"]["power"].clone(), states["sb:SB2AC0001"]["status"].clone()), (json!(-250.0), json!("discharging")));
    }

    #[test]
    fn the_account_logs_in_and_keeps_its_session() {
        testing::reset();
        cloud();
        testing::account("anker_cloud", &json!({"email": "moi@exemple.fr", "password": "secret"}));
        Account::check().unwrap();
        let kept = host::account("anker_cloud").unwrap();
        assert_eq!((kept["auth_token"].as_str(), kept["gtoken"].as_str()), (Some("t1"), Some(md5_hex("u1").as_str())));
        assert_eq!(kept["shown"]["solarbanks"], 1);
        assert!(testing::requests().iter().all(|r| r.contains("ankerpower-api-eu.anker.com")), "France is in Europe");

        let c: AnkerCloud = testing::open(json!({"timezone": "Europe/Paris"}), None);
        let s = testing::check(&c, MANIFEST);
        assert_eq!(s["sb:SB2AC0001"]["soc"], 75);
        assert_eq!(testing::requests().iter().filter(|r| r.contains("passport/login")).count(), 1, "the session kept is used");
    }

    #[test]
    fn a_session_logged_out_logs_in_again() {
        testing::reset();
        cloud();
        testing::account("anker_cloud", &json!({"email": "moi@exemple.fr", "password": "secret", "auth_token": "old", "gtoken": "g", "token_expires_at": 4_102_444_800i64}));
        testing::respond("POST", "get_site_list", {
            let n = std::rc::Rc::new(std::cell::Cell::new(0));
            move |_, _| {
                n.set(n.get() + 1);
                let body = if n.get() == 1 { json!({"code": 26084, "msg": "token kicked out"}) } else { json!({"code": 0, "data": {"site_list": []}}) };
                host::Answer { status: 200, headers: vec![], body: body.to_string().into_bytes() }
            }
        });
        assert!(sites(chrono_tz::Europe::Paris).unwrap().is_empty());
        assert_eq!(host::account("anker_cloud").unwrap()["auth_token"], "t1");
    }

    #[test]
    fn wrong_credentials_are_said() {
        testing::reset();
        testing::answer_json("POST", "passport/login", &json!({"code": 26108, "msg": "invalid"}));
        testing::account("anker_cloud", &json!({"email": "moi@exemple.fr", "password": "faux"}));
        assert!(matches!(Account::check(), Err(Error::Invalid(m)) if m.contains("refuse")));
        testing::account("anker_cloud", &json!({"email": "moi@exemple.fr", "password": "x", "country": "France"}));
        assert!(matches!(Account::check(), Err(Error::Invalid(_))));
    }

    #[test]
    fn the_server_of_a_country() {
        assert_eq!(server_of("FR"), EU);
        assert_eq!(server_of("US"), COM);
        assert_eq!(time_zone(chrono_tz::UTC), ("GMT+00:00".into(), 0));
    }
}
