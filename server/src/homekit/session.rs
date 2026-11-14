//! A connection to an accessory: HTTP/1.1 on TCP, in clear for pairing, then encrypted once the
//! session is verified (HAP, chapter 6.5.2): frames of at most 1024 bytes, each sent as its length
//! (2 bytes, little endian, authenticated), the data encrypted with ChaCha20-Poly1305 and its tag;
//! the nonce counts the frames, one counter per direction.

use super::{tlv, HapError, Pairing};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};
use hkdf::Hkdf;
use sha2::Sha512;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use x25519_dalek::{PublicKey, StaticSecret};

const TIMEOUT: Duration = Duration::from_secs(10);
const FRAME: usize = 1024;

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

/// An answer of the accessory.
#[derive(Debug)]
pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

struct Keys {
    write: [u8; 32],
    read: [u8; 32],
    written: u64,
    read_count: u64,
}

pub struct Connection {
    stream: TcpStream,
    keys: Option<Keys>,
    /// Bytes received (decrypted) not used yet.
    pending: Vec<u8>,
    host: String,
}

fn io(e: impl std::fmt::Display) -> HapError {
    HapError::Unreachable(e.to_string())
}

impl Connection {
    pub async fn connect(addr: &str) -> Result<Self, HapError> {
        let stream = tokio::time::timeout(TIMEOUT, TcpStream::connect(addr)).await.map_err(|_| io("timed out"))?.map_err(io)?;
        stream.set_nodelay(true).ok();
        Ok(Self { stream, keys: None, pending: vec![], host: addr.to_string() })
    }

    async fn send(&mut self, data: &[u8]) -> Result<(), HapError> {
        let Some(keys) = self.keys.as_mut() else {
            return self.stream.write_all(data).await.map_err(io);
        };
        let mut out = Vec::with_capacity(data.len() + 64);
        for chunk in data.chunks(FRAME) {
            let len = (chunk.len() as u16).to_le_bytes();
            out.extend_from_slice(&len);
            out.extend(seal(&keys.write, keys.written.to_le_bytes(), &len, chunk));
            keys.written += 1;
        }
        self.stream.write_all(&out).await.map_err(io)
    }

    async fn read_raw(&mut self, n: usize) -> Result<Vec<u8>, HapError> {
        let mut buf = vec![0u8; n];
        tokio::time::timeout(TIMEOUT, self.stream.read_exact(&mut buf)).await.map_err(|_| io("timed out"))?.map_err(io)?;
        Ok(buf)
    }

    /// Reads more of the answer into `pending`.
    async fn fill(&mut self) -> Result<(), HapError> {
        if self.keys.is_none() {
            let mut buf = [0u8; 4096];
            let n = tokio::time::timeout(TIMEOUT, self.stream.read(&mut buf)).await.map_err(|_| io("timed out"))?.map_err(io)?;
            if n == 0 {
                return Err(io("connection closed by the accessory"));
            }
            self.pending.extend_from_slice(&buf[..n]);
            return Ok(());
        }
        let len_bytes = self.read_raw(2).await?;
        let len = u16::from_le_bytes([len_bytes[0], len_bytes[1]]) as usize;
        if len > FRAME {
            return Err(HapError::Protocol("frame too long".into()));
        }
        let sealed = self.read_raw(len + 16).await?;
        let keys = self.keys.as_mut().expect("encrypted");
        let plain = open(&keys.read, keys.read_count.to_le_bytes(), &len_bytes, &sealed)?;
        keys.read_count += 1;
        self.pending.extend(plain);
        Ok(())
    }

    async fn take(&mut self, n: usize) -> Result<Vec<u8>, HapError> {
        while self.pending.len() < n {
            self.fill().await?;
        }
        Ok(self.pending.drain(..n).collect())
    }

    async fn line(&mut self) -> Result<String, HapError> {
        loop {
            if let Some(i) = self.pending.windows(2).position(|w| w == b"\r\n") {
                let line: Vec<u8> = self.pending.drain(..i + 2).collect();
                return Ok(String::from_utf8_lossy(&line[..i]).into_owned());
            }
            if self.pending.len() > 64 * 1024 {
                return Err(HapError::Protocol("header too long".into()));
            }
            self.fill().await?;
        }
    }

    /// One answer; the events the accessory may send in between (EVENT/1.0) are skipped.
    async fn response(&mut self) -> Result<Response, HapError> {
        loop {
            let status_line = self.line().await?;
            let mut headers = vec![];
            loop {
                let l = self.line().await?;
                if l.is_empty() {
                    break;
                }
                if let Some((k, v)) = l.split_once(':') {
                    headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
                }
            }
            let header = |name: &str| headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str());
            let body = if header("transfer-encoding").is_some_and(|v| v.eq_ignore_ascii_case("chunked")) {
                let mut body = vec![];
                loop {
                    let size = usize::from_str_radix(self.line().await?.split(';').next().unwrap_or("0").trim(), 16).map_err(|_| HapError::Protocol("bad chunk".into()))?;
                    let chunk = self.take(size + 2).await?;
                    if size == 0 {
                        break;
                    }
                    body.extend_from_slice(&chunk[..size]);
                }
                body
            } else {
                let len = header("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
                self.take(len).await?
            };
            if status_line.starts_with("EVENT/") {
                continue;
            }
            let status = status_line.split_whitespace().nth(1).and_then(|s| s.parse().ok()).ok_or_else(|| HapError::Protocol(format!("bad answer: {status_line}")))?;
            return Ok(Response { status, body });
        }
    }

    pub async fn request(&mut self, method: &str, path: &str, content_type: Option<&str>, body: &[u8]) -> Result<Response, HapError> {
        // Without the port, as the reference controllers do: some accessories reject it.
        let host = self.host.rsplit_once(':').map_or(self.host.as_str(), |(h, _)| h);
        let mut head = format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\n");
        if let Some(ct) = content_type {
            head += &format!("Content-Type: {ct}\r\n");
        }
        if method != "GET" || !body.is_empty() {
            head += &format!("Content-Length: {}\r\n", body.len());
        }
        head += "\r\n";
        let mut data = head.into_bytes();
        data.extend_from_slice(body);
        self.send(&data).await?;
        self.response().await
    }

    /// A pairing step: TLV in, TLV out.
    pub async fn tlv(&mut self, path: &str, items: &[(u8, &[u8])]) -> Result<Vec<(u8, Vec<u8>)>, HapError> {
        let resp = self.request("POST", path, Some("application/pairing+tlv8"), &tlv::encode(items)).await?;
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
        let items = tlv::decode(&resp.body).map_err(HapError::Protocol)?;
        if let Some(code) = tlv::get(&items, tlv::ERROR) {
            return Err(HapError::Accessory(code.first().copied().unwrap_or(1)));
        }
        Ok(items)
    }

    /// Pair Verify (HAP, chapter 5.7): both sides prove who they are with their long-term keys,
    /// and agree on the keys of the encrypted session.
    pub async fn verify(addr: &str, pairing: &Pairing) -> Result<Self, HapError> {
        let mut conn = Self::connect(addr).await?;
        let secret = StaticSecret::from(crate::auth::random_bytes::<32>());
        let public = PublicKey::from(&secret);
        let m2 = conn.tlv("/pair-verify", &[(tlv::STATE, &[1]), (tlv::PUBLIC_KEY, public.as_bytes())]).await?;
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
        let accessory = pairing.accessory_key()?;
        let mut info = their.to_vec();
        info.extend_from_slice(id);
        info.extend_from_slice(public.as_bytes());
        accessory.verify(&info, &signature).map_err(|_| HapError::Protocol("the accessory's signature does not match its pairing".into()))?;

        let me = pairing.controller_key()?;
        let mut info = public.as_bytes().to_vec();
        info.extend_from_slice(pairing.controller_id.as_bytes());
        info.extend_from_slice(&their);
        let mine = tlv::encode(&[(tlv::IDENTIFIER, pairing.controller_id.as_bytes()), (tlv::SIGNATURE, &me.sign(&info).to_bytes())]);
        let sealed = seal(&key, label("PV-Msg03"), &[], &mine);
        conn.tlv("/pair-verify", &[(tlv::STATE, &[3]), (tlv::ENCRYPTED_DATA, &sealed)]).await?;

        conn.keys = Some(Keys {
            write: hkdf("Control-Salt", shared.as_bytes(), "Control-Write-Encryption-Key"),
            read: hkdf("Control-Salt", shared.as_bytes(), "Control-Read-Encryption-Key"),
            written: 0,
            read_count: 0,
        });
        Ok(conn)
    }

    /// GET a JSON resource of the accessory (`/accessories`, `/characteristics?id=…`).
    pub async fn get_json(&mut self, path: &str) -> Result<serde_json::Value, HapError> {
        let resp = self.request("GET", path, None, &[]).await?;
        if !(200..300).contains(&resp.status) && resp.status != 207 {
            return Err(HapError::Protocol(format!("GET {path}: HTTP {}", resp.status)));
        }
        serde_json::from_slice(&resp.body).map_err(|e| HapError::Protocol(format!("GET {path}: {e}")))
    }

    pub async fn put_json(&mut self, path: &str, body: &serde_json::Value) -> Result<(), HapError> {
        let resp = self.request("PUT", path, Some("application/hap+json"), body.to_string().as_bytes()).await?;
        match resp.status {
            200..=299 => Ok(()),
            s => Err(HapError::Protocol(format!("PUT {path}: HTTP {s} {}", String::from_utf8_lossy(&resp.body)))),
        }
    }
}

/// The keys of a pairing, from the hex stored with it.
impl Pairing {
    pub fn accessory_key(&self) -> Result<VerifyingKey, HapError> {
        let bytes: [u8; 32] = hex::decode(&self.accessory_ltpk).ok().and_then(|b| b.try_into().ok()).ok_or_else(|| HapError::Protocol("bad accessory key".into()))?;
        VerifyingKey::from_bytes(&bytes).map_err(|_| HapError::Protocol("bad accessory key".into()))
    }

    pub fn controller_key(&self) -> Result<SigningKey, HapError> {
        let bytes: [u8; 32] = hex::decode(&self.controller_ltsk).ok().and_then(|b| b.try_into().ok()).ok_or_else(|| HapError::Protocol("bad controller key".into()))?;
        Ok(SigningKey::from_bytes(&bytes))
    }
}
