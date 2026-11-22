//! A connection to an accessory, kept open by Breezone (`host::subscribe`): HTTP/1.1, in clear for
//! pairing, then encrypted once the session is verified (HAP, chapter 6.5.2): frames of at most
//! 1024 bytes, each sent as its length (2 bytes, little endian, authenticated), the data encrypted
//! with ChaCha20-Poly1305 and its tag; the nonce counts the frames, one counter per direction.
//!
//! Answers are read during the call (`host::receive`); the events the accessory sends (EVENT/1.0)
//! are kept to be handled, whether they come during a request or between calls (`on_message`).

use crate::{tlv, HapError, Pairing};
use breezone_module_sdk::{host, Value};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use ed25519_dalek::{Signer, Verifier};
use hkdf::Hkdf;
use sha2::Sha512;
use x25519_dalek::{PublicKey, StaticSecret};

const FRAME: usize = 1024;
/// How long an answer is waited for.
const ANSWER_MS: u32 = 10_000;

pub fn hkdf(salt: &str, ikm: &[u8], info: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    Hkdf::<Sha512>::new(Some(salt.as_bytes()), ikm).expand(info.as_bytes(), &mut out).expect("32 bytes");
    out
}

/// The nonce of a pairing message ("PS-Msg05") or of a frame (its counter): 4 zero bytes first.
fn nonce(tail: [u8; 8]) -> Nonce {
    let mut n = [0u8; 12];
    n[4..].copy_from_slice(&tail);
    Nonce::from(n)
}

pub fn label(text: &str) -> [u8; 8] {
    let mut out = [0u8; 8];
    out.copy_from_slice(&text.as_bytes()[..8]);
    out
}

pub fn seal(key: &[u8; 32], tail: [u8; 8], aad: &[u8], plain: &[u8]) -> Vec<u8> {
    let cipher = ChaCha20Poly1305::new_from_slice(key).expect("32-byte key");
    cipher.encrypt(&nonce(tail), Payload { msg: plain, aad }).expect("encryption")
}

pub fn open(key: &[u8; 32], tail: [u8; 8], aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>, HapError> {
    let cipher = ChaCha20Poly1305::new_from_slice(key).expect("32-byte key");
    cipher.decrypt(&nonce(tail), Payload { msg: sealed, aad }).map_err(|_| HapError::Protocol("cannot decrypt the accessory's answer".into()))
}

pub fn random<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).expect("random bytes");
    b
}

struct Keys {
    write: [u8; 32],
    read: [u8; 32],
    written: u64,
    read_count: u64,
}

/// An answer of the accessory.
#[derive(Debug)]
pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

/// One HTTP message at the start of `pending`: its status line, its body, and how many bytes it
/// takes; `None` while not all of it is there.
pub fn parse_message(pending: &[u8]) -> Result<Option<(String, Vec<u8>, usize)>, HapError> {
    let Some(end) = pending.windows(4).position(|w| w == b"\r\n\r\n") else {
        if pending.len() > 64 * 1024 {
            return Err(HapError::Protocol("header too long".into()));
        }
        return Ok(None);
    };
    let head = String::from_utf8_lossy(&pending[..end]).into_owned();
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap_or_default().to_string();
    let headers: Vec<(String, String)> = lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string())).collect();
    let header = |name: &str| headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str());
    let mut at = end + 4;
    if header("transfer-encoding").is_some_and(|v| v.eq_ignore_ascii_case("chunked")) {
        let mut body = vec![];
        loop {
            let Some(line_end) = pending[at..].windows(2).position(|w| w == b"\r\n") else { return Ok(None) };
            let size_text = String::from_utf8_lossy(&pending[at..at + line_end]).into_owned();
            let size = usize::from_str_radix(size_text.split(';').next().unwrap_or("0").trim(), 16).map_err(|_| HapError::Protocol("bad chunk".into()))?;
            let start = at + line_end + 2;
            if pending.len() < start + size + 2 {
                return Ok(None);
            }
            body.extend_from_slice(&pending[start..start + size]);
            at = start + size + 2;
            if size == 0 {
                return Ok(Some((status_line, body, at)));
            }
        }
    }
    let len: usize = header("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
    if pending.len() < at + len {
        return Ok(None);
    }
    Ok(Some((status_line, pending[at..at + len].to_vec(), at + len)))
}

pub struct Conn {
    pub subscription: u32,
    host: String,
    /// Bytes received, not decrypted yet.
    raw: Vec<u8>,
    keys: Option<Keys>,
    /// Bytes decrypted, not read yet.
    pending: Vec<u8>,
    /// The events received, to be handled.
    pub events: Vec<Value>,
}

impl Drop for Conn {
    fn drop(&mut self) {
        host::unsubscribe(self.subscription);
    }
}

impl Conn {
    pub fn connect(host_: &str) -> Result<Self, HapError> {
        let subscription = host::subscribe(&format!("tcp://{host_}")).map_err(|e| HapError::Unreachable(format!("{e:?}")))?;
        Ok(Self { subscription, host: host_.to_string(), raw: vec![], keys: None, pending: vec![], events: vec![] })
    }

    fn send(&mut self, data: &[u8]) -> Result<(), HapError> {
        let out = match self.keys.as_mut() {
            None => data.to_vec(),
            Some(keys) => {
                let mut out = Vec::with_capacity(data.len() + 64);
                for chunk in data.chunks(FRAME) {
                    let len = (chunk.len() as u16).to_le_bytes();
                    out.extend_from_slice(&len);
                    out.extend(seal(&keys.write, keys.written.to_le_bytes(), &len, chunk));
                    keys.written += 1;
                }
                out
            }
        };
        host::send(self.subscription, &out).map_err(|_| HapError::Unreachable("connection closed by the accessory".into()))
    }

    /// What came on the connection (between calls).
    pub fn received(&mut self, data: &[u8]) {
        self.raw.extend_from_slice(data);
    }

    /// Moves what can be read of `raw` to `pending` (decrypting whole frames).
    fn decrypt(&mut self) -> Result<(), HapError> {
        let Some(keys) = self.keys.as_mut() else {
            self.pending.append(&mut self.raw);
            return Ok(());
        };
        while self.raw.len() >= 2 {
            let len = usize::from(u16::from_le_bytes([self.raw[0], self.raw[1]]));
            if len > FRAME {
                return Err(HapError::Protocol("frame too long".into()));
            }
            if self.raw.len() < 2 + len + 16 {
                break;
            }
            let plain = open(&keys.read, keys.read_count.to_le_bytes(), &self.raw[..2], &self.raw[2..2 + len + 16])?;
            keys.read_count += 1;
            self.raw.drain(..2 + len + 16);
            self.pending.extend(plain);
        }
        Ok(())
    }

    /// The next whole message already received, if any (an event kept aside).
    pub fn try_message(&mut self) -> Result<Option<(String, Vec<u8>)>, HapError> {
        self.decrypt()?;
        let Some((status, body, used)) = parse_message(&self.pending)? else { return Ok(None) };
        self.pending.drain(..used);
        Ok(Some((status, body)))
    }

    /// The next answer, waited for during the call; events met on the way are kept.
    fn response(&mut self) -> Result<Response, HapError> {
        loop {
            match self.try_message()? {
                Some((status_line, body)) if status_line.starts_with("EVENT/") => {
                    if let Ok(v) = breezone_module_sdk::serde_json::from_slice(&body) {
                        self.events.push(v);
                    }
                }
                Some((status_line, body)) => {
                    let status = status_line.split_whitespace().nth(1).and_then(|s| s.parse().ok()).ok_or_else(|| HapError::Protocol(format!("bad answer: {status_line}")))?;
                    return Ok(Response { status, body });
                }
                None => {
                    let more = host::receive(self.subscription, ANSWER_MS).map_err(|e| HapError::Unreachable(format!("{e:?}")))?;
                    self.raw.extend_from_slice(&more);
                }
            }
        }
    }

    pub fn request(&mut self, method: &str, path: &str, content_type: Option<&str>, body: &[u8]) -> Result<Response, HapError> {
        // Without the port, as the reference controllers do: some accessories reject it.
        let host_ = self.host.rsplit_once(':').map_or(self.host.as_str(), |(h, _)| h);
        let mut head = format!("{method} {path} HTTP/1.1\r\nHost: {host_}\r\n");
        if let Some(ct) = content_type {
            head += &format!("Content-Type: {ct}\r\n");
        }
        if method != "GET" || !body.is_empty() {
            head += &format!("Content-Length: {}\r\n", body.len());
        }
        head += "\r\n";
        let mut data = head.into_bytes();
        data.extend_from_slice(body);
        self.send(&data)?;
        self.response()
    }

    /// A pairing step: TLV in, TLV out.
    pub fn tlv(&mut self, path: &str, items: &[(u8, &[u8])]) -> Result<Vec<(u8, Vec<u8>)>, HapError> {
        let resp = self.request("POST", path, Some("application/pairing+tlv8"), &tlv::encode(items))?;
        let items = tlv::decode(&resp.body).unwrap_or_default();
        if !(200..300).contains(&resp.status) {
            if let Some(code) = tlv::get(&items, tlv::ERROR).and_then(|c| c.first()) {
                return Err(HapError::Accessory(*code));
            }
            // 429: a pairing with another controller is under way.
            if resp.status == 429 {
                return Err(HapError::Accessory(7));
            }
            return Err(HapError::Protocol(format!("{path}: HTTP {}", resp.status)));
        }
        if let Some(code) = tlv::get(&items, tlv::ERROR) {
            return Err(HapError::Accessory(code.first().copied().unwrap_or(1)));
        }
        Ok(items)
    }

    /// Pair Verify (HAP, chapter 5.7): both sides prove who they are with their long-term keys,
    /// and agree on the keys of the encrypted session.
    pub fn verify(host_: &str, pairing: &Pairing) -> Result<Self, HapError> {
        let mut conn = Self::connect(host_)?;
        let secret = StaticSecret::from(random::<32>());
        let public = PublicKey::from(&secret);
        let m2 = conn.tlv("/pair-verify", &[(tlv::STATE, &[1]), (tlv::PUBLIC_KEY, public.as_bytes())]).map_err(|e| match e {
            // Nothing of Breezone's pairing was shown yet: the accessory is busy or restarting.
            HapError::Unreachable(m) => HapError::Unreachable(format!("{m}, before the session was set up (the accessory may be restarting, or have too many connections open)")),
            other => other,
        })?;
        let their: [u8; 32] = tlv::get(&m2, tlv::PUBLIC_KEY).and_then(|k| k.try_into().ok()).ok_or_else(|| HapError::Protocol("pair-verify: no public key".into()))?;
        let shared = secret.diffie_hellman(&PublicKey::from(their));
        let key = hkdf("Pair-Verify-Encrypt-Salt", shared.as_bytes(), "Pair-Verify-Encrypt-Info");
        let sealed = tlv::get(&m2, tlv::ENCRYPTED_DATA).ok_or_else(|| HapError::Protocol("pair-verify: no data".into()))?;
        let inner = tlv::decode(&open(&key, label("PV-Msg02"), &[], sealed)?).map_err(HapError::Protocol)?;
        let id = tlv::get(&inner, tlv::IDENTIFIER).unwrap_or_default();
        if id != pairing.accessory_id.as_bytes() {
            return Err(HapError::Protocol("another accessory answers at this address".into()));
        }
        let signature =
            tlv::get(&inner, tlv::SIGNATURE).and_then(|s| ed25519_dalek::Signature::from_slice(s).ok()).ok_or_else(|| HapError::Protocol("pair-verify: no signature".into()))?;
        let mut info = their.to_vec();
        info.extend_from_slice(id);
        info.extend_from_slice(public.as_bytes());
        pairing.accessory_key()?.verify(&info, &signature).map_err(|_| HapError::Protocol("the accessory's signature does not match its pairing".into()))?;

        let me = pairing.controller_key()?;
        let mut info = public.as_bytes().to_vec();
        info.extend_from_slice(pairing.controller_id.as_bytes());
        info.extend_from_slice(&their);
        let mine = tlv::encode(&[(tlv::IDENTIFIER, pairing.controller_id.as_bytes()), (tlv::SIGNATURE, &me.sign(&info).to_bytes())]);
        let sealed = seal(&key, label("PV-Msg03"), &[], &mine);
        // Breezone's key was shown: a refusal, or a connection closed now, means the accessory does
        // not know it any more (some, like Legrand's, close the connection instead of answering).
        conn.tlv("/pair-verify", &[(tlv::STATE, &[3]), (tlv::ENCRYPTED_DATA, &sealed)]).map_err(|e| match e {
            HapError::Accessory(2) => HapError::Forgotten,
            HapError::Unreachable(m) if m.contains("closed") || m.contains("timeout") => HapError::Forgotten,
            other => other,
        })?;
        conn.keys = Some(Keys {
            write: hkdf("Control-Salt", shared.as_bytes(), "Control-Write-Encryption-Key"),
            read: hkdf("Control-Salt", shared.as_bytes(), "Control-Read-Encryption-Key"),
            written: 0,
            read_count: 0,
        });
        Ok(conn)
    }

    /// GET a JSON resource of the accessory (`/accessories`, `/characteristics?id=…`).
    pub fn get_json(&mut self, path: &str) -> Result<Value, HapError> {
        let resp = self.request("GET", path, None, &[])?;
        if !(200..300).contains(&resp.status) && resp.status != 207 {
            return Err(HapError::Protocol(format!("GET {path}: HTTP {}", resp.status)));
        }
        breezone_module_sdk::serde_json::from_slice(&resp.body).map_err(|e| HapError::Protocol(format!("GET {path}: {e}")))
    }

    pub fn put_json(&mut self, path: &str, body: &Value) -> Result<(), HapError> {
        let resp = self.request("PUT", path, Some("application/hap+json"), body.to_string().as_bytes())?;
        match resp.status {
            200..=299 => Ok(()),
            s => Err(HapError::Protocol(format!("PUT {path}: HTTP {s} {}", String::from_utf8_lossy(&resp.body)))),
        }
    }
}
