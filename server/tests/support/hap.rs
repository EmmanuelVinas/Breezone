//! A HomeKit accessory for the tests (it pairs, serves its database on an encrypted session and
//! sends events), with the pieces of HAP it needs: TLV8, the accessory's side of SRP, the
//! encryption of the sessions.
#![allow(dead_code)]

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use hkdf::Hkdf;
use sha2::Sha512;

pub fn hkdf(salt: &str, ikm: &[u8], info: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    Hkdf::<Sha512>::new(Some(salt.as_bytes()), ikm).expand(info.as_bytes(), &mut out).expect("32 bytes");
    out
}

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

pub fn open(key: &[u8; 32], tail: [u8; 8], aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>, String> {
    let cipher = ChaCha20Poly1305::new_from_slice(key).expect("32-byte key");
    cipher.decrypt(&nonce(tail), Payload { msg: sealed, aad }).map_err(|_| "cannot decrypt".to_string())
}

pub mod tlv {
    //! TLV8, the format of the pairing messages: one byte of type, one of length (at most 255), the
    //! value; a longer value is split in fragments of the same type.

    pub const METHOD: u8 = 0x00;
    pub const IDENTIFIER: u8 = 0x01;
    pub const SALT: u8 = 0x02;
    pub const PUBLIC_KEY: u8 = 0x03;
    pub const PROOF: u8 = 0x04;
    pub const ENCRYPTED_DATA: u8 = 0x05;
    pub const STATE: u8 = 0x06;
    pub const ERROR: u8 = 0x07;
    pub const SIGNATURE: u8 = 0x0a;

    pub fn encode(items: &[(u8, &[u8])]) -> Vec<u8> {
        let mut out = vec![];
        for (kind, value) in items {
            if value.is_empty() {
                out.extend_from_slice(&[*kind, 0]);
                continue;
            }
            for chunk in value.chunks(255) {
                out.push(*kind);
                out.push(chunk.len() as u8);
                out.extend_from_slice(chunk);
            }
        }
        out
    }

    /// The items of a message, a value split in fragments put back together.
    pub fn decode(buf: &[u8]) -> Result<Vec<(u8, Vec<u8>)>, String> {
        let mut out: Vec<(u8, Vec<u8>)> = vec![];
        let mut i = 0;
        let mut last_len = 0;
        while i < buf.len() {
            let (kind, len) = (buf[i], *buf.get(i + 1).ok_or("truncated TLV")? as usize);
            let value = buf.get(i + 2..i + 2 + len).ok_or("truncated TLV")?;
            match out.last_mut() {
                Some((k, v)) if *k == kind && last_len == 255 => v.extend_from_slice(value),
                _ => out.push((kind, value.to_vec())),
            }
            last_len = len;
            i += 2 + len;
        }
        Ok(out)
    }

    pub fn get(items: &[(u8, Vec<u8>)], kind: u8) -> Option<&[u8]> {
        items.iter().find(|(k, _)| *k == kind).map(|(_, v)| v.as_slice())
    }
}

pub mod srp {
    //! SRP-6a as HomeKit uses it for pairing (HAP, chapter 5.6): the 3072-bit group of RFC 5054,
    //! g = 5, SHA-512, user "Pair-Setup", the setup code as the password. Numbers are hashed padded to
    //! the length of N, as the HomeKit accessory kits do.

    use num_bigint::BigUint;
    use sha2::{Digest, Sha512};

    const N_HEX: &str = "\
    FFFFFFFFFFFFFFFFC90FDAA22168C234C4C6628B80DC1CD129024E088A67CC74020BBEA63B139B22514A08798E3404DD\
    EF9519B3CD3A431B302B0A6DF25F14374FE1356D6D51C245E485B576625E7EC6F44C42E9A637ED6B0BFF5CB6F406B7ED\
    EE386BFB5A899FA5AE9F24117C4B1FE649286651ECE45B3DC2007CB8A163BF0598DA48361C55D39A69163FA8FD24CF5F\
    83655D23DCA3AD961C62F356208552BB9ED529077096966D670C354E4ABC9804F1746C08CA18217C32905E462E36CE3B\
    E39E772C180E86039B2783A2EC07A28FB5C55DF06F4C52C9DE2BCBF6955817183995497CEA956AE515D2261898FA0510\
    15728E5A8AAAC42DAD33170D04507A33A85521ABDF1CBA64ECFB850458DBEF0A8AEA71575D060C7DB3970F85A6E1E4C7\
    ABF5AE8CDB0933D71E8C94E04A25619DCEE3D2261AD2EE6BF12FFA06D98A0864D87602733EC86A64521F2B18177B200C\
    BBE117577A615D6C770988C0BAD946E208E24FA074E5AB3143DB5BFCE0FD108E4B82D120A93AD2CAFFFFFFFFFFFFFFFF";
    const G: u32 = 5;
    /// Bytes of N.
    pub const LEN: usize = 384;
    pub const USER: &str = "Pair-Setup";

    fn n() -> BigUint {
        BigUint::parse_bytes(N_HEX.as_bytes(), 16).expect("N")
    }

    fn hash(parts: &[&[u8]]) -> Vec<u8> {
        let mut h = Sha512::new();
        for p in parts {
            h.update(p);
        }
        h.finalize().to_vec()
    }

    /// A number as `LEN` big-endian bytes.
    pub fn pad(x: &BigUint) -> Vec<u8> {
        let bytes = x.to_bytes_be();
        let mut out = vec![0u8; LEN.saturating_sub(bytes.len())];
        out.extend(bytes);
        out
    }

    fn k() -> BigUint {
        BigUint::from_bytes_be(&hash(&[&pad(&n()), &pad(&BigUint::from(G))]))
    }

    fn x(salt: &[u8], password: &str) -> BigUint {
        let inner = hash(&[format!("{USER}:{password}").as_bytes()]);
        BigUint::from_bytes_be(&hash(&[salt, &inner]))
    }

    fn u(a_pub: &[u8], b_pub: &[u8]) -> BigUint {
        BigUint::from_bytes_be(&hash(&[a_pub, b_pub]))
    }

    /// M1 = H(H(N) xor H(g) | H(user) | salt | A | B | K).
    fn proof(salt: &[u8], a_pub: &[u8], b_pub: &[u8], key: &[u8]) -> Vec<u8> {
        let hn = hash(&[&pad(&n())]);
        let hg = hash(&[&[G as u8]]);
        let xor: Vec<u8> = hn.iter().zip(&hg).map(|(a, b)| a ^ b).collect();
        hash(&[&xor, &hash(&[USER.as_bytes()]), salt, a_pub, b_pub, key])
    }

    /// What both sides know once the accessory's proof checks out.
    pub struct Agreed {
        /// K, the session key the next steps derive their keys from.
        pub key: Vec<u8>,
        /// M1, sent to the accessory.
        pub proof: Vec<u8>,
        /// M2, expected from the accessory.
        pub expected: Vec<u8>,
    }

    /// The controller's side.
    pub struct Client {
        a: BigUint,
        /// A, padded.
        pub public: Vec<u8>,
    }

    impl Client {
        /// `secret`: random bytes (the private value a).
        pub fn new(secret: &[u8; 32]) -> Self {
            let a = BigUint::from_bytes_be(secret);
            let public = pad(&BigUint::from(G).modpow(&a, &n()));
            Self { a, public }
        }

        /// From the accessory's salt and public value B, the proof to send and the session key.
        pub fn agree(&self, salt: &[u8], b_pub: &[u8], password: &str) -> Result<Agreed, String> {
            let n = n();
            let b = BigUint::from_bytes_be(b_pub);
            if (&b % &n) == BigUint::ZERO {
                return Err("invalid public value from the accessory".into());
            }
            let b_pub = pad(&b);
            let u = u(&self.public, &b_pub);
            if u == BigUint::ZERO {
                return Err("invalid public value from the accessory".into());
            }
            let x = x(salt, password);
            let gx = (k() * BigUint::from(G).modpow(&x, &n)) % &n;
            let base = ((&b % &n) + &n - gx) % &n;
            let s = base.modpow(&(&self.a + &u * &x), &n);
            let key = hash(&[&pad(&s)]);
            let proof = proof(salt, &self.public, &b_pub, &key);
            let expected = hash(&[&self.public, &proof, &key]);
            Ok(Agreed { key, proof, expected })
        }
    }

    /// The accessory's side: only used to test the controller against a fake accessory.
    pub struct Server {
        b: BigUint,
        v: BigUint,
        pub salt: Vec<u8>,
        pub public: Vec<u8>,
    }

    impl Server {
        pub fn new(salt: &[u8], password: &str, secret: &[u8; 32]) -> Self {
            let n = n();
            let v = BigUint::from(G).modpow(&x(salt, password), &n);
            let b = BigUint::from_bytes_be(secret);
            let public = pad(&((k() * &v + BigUint::from(G).modpow(&b, &n)) % &n));
            Self { b, v, salt: salt.to_vec(), public }
        }

        /// Checks the controller's proof; gives the session key and M2.
        pub fn verify(&self, a_pub: &[u8], m1: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
            let n = n();
            let a = BigUint::from_bytes_be(a_pub);
            let a_pub = pad(&a);
            let u = u(&a_pub, &self.public);
            let s = (a * self.v.modpow(&u, &n)).modpow(&self.b, &n);
            let key = hash(&[&pad(&s)]);
            if proof(&self.salt, &a_pub, &self.public, &key) != m1 {
                return None;
            }
            let m2 = hash(&[&a_pub, m1, &key]);
            Some((key, m2))
        }
    }
}

pub mod fake {
    //! An accessory that pairs and serves its database on an encrypted session, enough to test
    //! the controller end to end (the integration tests of the modules too), not for use.

    use super::{hkdf, label, open, seal};
    use super::{srp, tlv};
    use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};
    use serde_json::{json, Value};
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use x25519_dalek::{PublicKey, StaticSecret};

    pub const CODE: &str = "031-45-154";

    #[derive(Default)]
    pub struct State {
        /// Paired controllers: id → long-term public key.
        pub controllers: Vec<(String, [u8; 32])>,
        /// (aid, iid) → value of the characteristics written.
        pub written: Vec<(u64, u64, Value)>,
        pub position: i64,
        /// Has a screen: shows a new code each time a pairing starts.
        pub screen: bool,
        pub shown: Option<String>,
        /// The characteristics a session subscribed to (`ev`).
        pub subscribed: Vec<(u64, u64)>,
    }

    pub struct Fake {
        pub port: u16,
        pub state: Arc<Mutex<State>>,
        /// Sends an event to the sessions that subscribed: `{"characteristics": [...]}`.
        pub events: tokio::sync::broadcast::Sender<Value>,
    }

    impl Fake {
        /// A press of a button of the remote (aid 5): button 1 to 4, 0 single, 1 double, 2 long.
        pub fn press(&self, button: u64, value: u64) {
            let _ = self.events.send(json!({"characteristics": [{"aid": 5, "iid": 10 * button + 1, "value": value}]}));
        }
    }

    pub async fn start() -> Fake {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let state = Arc::new(Mutex::new(State { position: 40, ..Default::default() }));
        let key = SigningKey::from_bytes(&[42; 32]);
        let st = state.clone();
        let (events, _) = tokio::sync::broadcast::channel(16);
        let tx = events.clone();
        tokio::spawn(async move {
            loop {
                let Ok((socket, _)) = listener.accept().await else { return };
                tokio::spawn(serve(socket, key.clone(), st.clone(), tx.clone()));
            }
        });
        Fake { port, state, events }
    }

    async fn read_request(socket: &mut TcpStream, buf: &mut Vec<u8>, keys: &mut Option<([u8; 32], [u8; 32], u64, u64)>) -> Option<(String, String, Vec<u8>)> {
        loop {
            if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&buf[..end]).into_owned();
                let len: usize = head.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap())).unwrap_or(0);
                if buf.len() >= end + 4 + len {
                    let body = buf[end + 4..end + 4 + len].to_vec();
                    buf.drain(..end + 4 + len);
                    let mut first = head.lines().next().unwrap().split_whitespace();
                    return Some((first.next()?.to_string(), first.next()?.to_string(), body));
                }
            }
            match keys {
                None => {
                    let mut tmp = [0u8; 4096];
                    let n = socket.read(&mut tmp).await.ok()?;
                    if n == 0 {
                        return None;
                    }
                    buf.extend_from_slice(&tmp[..n]);
                }
                Some((_, read, _, count)) => {
                    let mut len = [0u8; 2];
                    socket.read_exact(&mut len).await.ok()?;
                    let mut sealed = vec![0u8; u16::from_le_bytes(len) as usize + 16];
                    socket.read_exact(&mut sealed).await.ok()?;
                    buf.extend(open(read, count.to_le_bytes(), &len, &sealed).ok()?);
                    *count += 1;
                }
            }
        }
    }

    async fn reply(socket: &mut TcpStream, keys: &mut Option<([u8; 32], [u8; 32], u64, u64)>, status: &str, ct: &str, body: &[u8]) {
        let mut data = format!("HTTP/1.1 {status}\r\nContent-Type: {ct}\r\nContent-Length: {}\r\n\r\n", body.len()).into_bytes();
        data.extend_from_slice(body);
        let out = match keys {
            None => data,
            Some((write, _, count, _)) => {
                let mut out = vec![];
                for chunk in data.chunks(1024) {
                    let len = (chunk.len() as u16).to_le_bytes();
                    out.extend_from_slice(&len);
                    out.extend(seal(write, count.to_le_bytes(), &len, chunk));
                    *count += 1;
                }
                out
            }
        };
        socket.write_all(&out).await.unwrap();
    }

    fn database(position: i64) -> Value {
        json!({"accessories": [
            {"aid": 1, "services": [
                {"iid": 1, "type": "3E", "characteristics": [
                    {"iid": 2, "type": "23", "value": "Passerelle", "perms": ["pr"], "format": "string"},
                    {"iid": 3, "type": "20", "value": "Legrand", "perms": ["pr"], "format": "string"},
                    {"iid": 4, "type": "21", "value": "Gateway", "perms": ["pr"], "format": "string"}]}]},
            {"aid": 2, "services": [
                {"iid": 1, "type": "0000003E-0000-1000-8000-0026BB765291", "characteristics": [
                    {"iid": 2, "type": "00000023-0000-1000-8000-0026BB765291", "value": "Volet salon", "perms": ["pr"], "format": "string"}]},
                {"iid": 8, "type": "0000008C-0000-1000-8000-0026BB765291", "characteristics": [
                    {"iid": 9, "type": "0000006D-0000-1000-8000-0026BB765291", "value": position, "perms": ["pr", "ev"], "format": "uint8"},
                    {"iid": 10, "type": "0000007C-0000-1000-8000-0026BB765291", "value": position, "perms": ["pr", "pw", "ev"], "format": "uint8"},
                    {"iid": 11, "type": "00000072-0000-1000-8000-0026BB765291", "value": 2, "perms": ["pr", "ev"], "format": "uint8"}]}]},
            // A remote with 4 buttons, labelled 1 to 4.
            {"aid": 5, "services": [
                {"iid": 1, "type": "3E", "characteristics": [{"iid": 2, "type": "23", "value": "Télécommande", "perms": ["pr"], "format": "string"}]},
                {"iid": 10, "type": "89", "characteristics": [{"iid": 11, "type": "73", "value": null, "perms": ["pr", "ev"]}, {"iid": 12, "type": "CB", "value": 1}]},
                {"iid": 20, "type": "89", "characteristics": [{"iid": 21, "type": "73", "value": null, "perms": ["pr", "ev"]}, {"iid": 22, "type": "CB", "value": 2}]},
                {"iid": 30, "type": "89", "characteristics": [{"iid": 31, "type": "73", "value": null, "perms": ["pr", "ev"]}, {"iid": 32, "type": "CB", "value": 3}]},
                {"iid": 40, "type": "89", "characteristics": [{"iid": 41, "type": "73", "value": null, "perms": ["pr", "ev"]}, {"iid": 42, "type": "CB", "value": 4}]}]}
        ]})
    }

    async fn serve(mut socket: TcpStream, key: SigningKey, state: Arc<Mutex<State>>, events: tokio::sync::broadcast::Sender<Value>) {
        let mut buf = vec![];
        let mut keys = None;
        let mut srp: Option<srp::Server> = None;
        let mut session_key = vec![];
        let mut verify: Option<(StaticSecret, [u8; 32], [u8; 32])> = None;
        while let Some((method, path, body)) = read_request(&mut socket, &mut buf, &mut keys).await {
            let items = tlv::decode(&body).unwrap_or_default();
            let st = tlv::get(&items, tlv::STATE).and_then(|s| s.first().copied());
            let answer: Vec<u8> = match (method.as_str(), path.as_str(), st) {
                ("POST", "/pair-setup", Some(1)) => {
                    let paired = !state.lock().unwrap().controllers.is_empty();
                    if paired {
                        tlv::encode(&[(tlv::STATE, &[2]), (tlv::ERROR, &[6])])
                    } else {
                        let code = {
                            let mut st = state.lock().unwrap();
                            if st.screen {
                                let n = st.shown.as_deref().map_or(0, |c| c.replace('-', "").parse::<u32>().unwrap() + 1);
                                let digits = format!("{:08}", 20_000_000 + n);
                                st.shown = Some(format!("{}-{}-{}", &digits[..3], &digits[3..5], &digits[5..]));
                            }
                            st.shown.clone().unwrap_or_else(|| CODE.to_string())
                        };
                        let server = srp::Server::new(&[5; 16], &code, &[7; 32]);
                        let a = tlv::encode(&[(tlv::STATE, &[2]), (tlv::SALT, &server.salt), (tlv::PUBLIC_KEY, &server.public)]);
                        srp = Some(server);
                        a
                    }
                }
                ("POST", "/pair-setup", Some(3)) => {
                    let a_pub = tlv::get(&items, tlv::PUBLIC_KEY).unwrap();
                    match srp.as_ref().unwrap().verify(a_pub, tlv::get(&items, tlv::PROOF).unwrap()) {
                        Some((k, m2)) => {
                            session_key = k;
                            tlv::encode(&[(tlv::STATE, &[4]), (tlv::PROOF, &m2)])
                        }
                        None => tlv::encode(&[(tlv::STATE, &[4]), (tlv::ERROR, &[2])]),
                    }
                }
                ("POST", "/pair-setup", Some(5)) => {
                    let k = hkdf("Pair-Setup-Encrypt-Salt", &session_key, "Pair-Setup-Encrypt-Info");
                    let inner = tlv::decode(&open(&k, label("PS-Msg05"), &[], tlv::get(&items, tlv::ENCRYPTED_DATA).unwrap()).unwrap()).unwrap();
                    let id = tlv::get(&inner, tlv::IDENTIFIER).unwrap();
                    let ltpk: [u8; 32] = tlv::get(&inner, tlv::PUBLIC_KEY).unwrap().try_into().unwrap();
                    let x = hkdf("Pair-Setup-Controller-Sign-Salt", &session_key, "Pair-Setup-Controller-Sign-Info");
                    let mut info = x.to_vec();
                    info.extend_from_slice(id);
                    info.extend_from_slice(&ltpk);
                    let sig = ed25519_dalek::Signature::from_slice(tlv::get(&inner, tlv::SIGNATURE).unwrap()).unwrap();
                    VerifyingKey::from_bytes(&ltpk).unwrap().verify(&info, &sig).unwrap();
                    state.lock().unwrap().controllers.push((String::from_utf8_lossy(id).into_owned(), ltpk));
                    let x = hkdf("Pair-Setup-Accessory-Sign-Salt", &session_key, "Pair-Setup-Accessory-Sign-Info");
                    let mut info = x.to_vec();
                    info.extend_from_slice(b"AA:BB:CC:DD:EE:FF");
                    info.extend_from_slice(key.verifying_key().as_bytes());
                    let mine =
                        tlv::encode(&[(tlv::IDENTIFIER, b"AA:BB:CC:DD:EE:FF"), (tlv::PUBLIC_KEY, key.verifying_key().as_bytes()), (tlv::SIGNATURE, &key.sign(&info).to_bytes())]);
                    tlv::encode(&[(tlv::STATE, &[6]), (tlv::ENCRYPTED_DATA, &seal(&k, label("PS-Msg06"), &[], &mine))])
                }
                ("POST", "/pair-verify", Some(1)) => {
                    let theirs: [u8; 32] = tlv::get(&items, tlv::PUBLIC_KEY).unwrap().try_into().unwrap();
                    let secret = StaticSecret::from([11; 32]);
                    let public = PublicKey::from(&secret);
                    let shared = secret.diffie_hellman(&PublicKey::from(theirs));
                    let k = hkdf("Pair-Verify-Encrypt-Salt", shared.as_bytes(), "Pair-Verify-Encrypt-Info");
                    let mut info = public.as_bytes().to_vec();
                    info.extend_from_slice(b"AA:BB:CC:DD:EE:FF");
                    info.extend_from_slice(&theirs);
                    let inner = tlv::encode(&[(tlv::IDENTIFIER, b"AA:BB:CC:DD:EE:FF"), (tlv::SIGNATURE, &key.sign(&info).to_bytes())]);
                    let a = tlv::encode(&[(tlv::STATE, &[2]), (tlv::PUBLIC_KEY, public.as_bytes()), (tlv::ENCRYPTED_DATA, &seal(&k, label("PV-Msg02"), &[], &inner))]);
                    verify = Some((secret, theirs, *shared.as_bytes()));
                    a
                }
                ("POST", "/pair-verify", Some(3)) => {
                    let (secret, theirs, shared) = verify.take().unwrap();
                    let k = hkdf("Pair-Verify-Encrypt-Salt", &shared, "Pair-Verify-Encrypt-Info");
                    let inner = tlv::decode(&open(&k, label("PV-Msg03"), &[], tlv::get(&items, tlv::ENCRYPTED_DATA).unwrap()).unwrap()).unwrap();
                    let id = String::from_utf8_lossy(tlv::get(&inner, tlv::IDENTIFIER).unwrap()).into_owned();
                    let known = state.lock().unwrap().controllers.iter().find(|(c, _)| *c == id).map(|(_, k)| *k);
                    let Some(ltpk) = known else {
                        reply(&mut socket, &mut keys, "200 OK", "application/pairing+tlv8", &tlv::encode(&[(tlv::STATE, &[4]), (tlv::ERROR, &[2])])).await;
                        continue;
                    };
                    let mut info = theirs.to_vec();
                    info.extend_from_slice(id.as_bytes());
                    info.extend_from_slice(PublicKey::from(&secret).as_bytes());
                    let sig = ed25519_dalek::Signature::from_slice(tlv::get(&inner, tlv::SIGNATURE).unwrap()).unwrap();
                    VerifyingKey::from_bytes(&ltpk).unwrap().verify(&info, &sig).unwrap();
                    reply(&mut socket, &mut keys, "200 OK", "application/pairing+tlv8", &tlv::encode(&[(tlv::STATE, &[4])])).await;
                    // From the accessory's side, its write key is the controller's read key.
                    keys = Some((hkdf("Control-Salt", &shared, "Control-Read-Encryption-Key"), hkdf("Control-Salt", &shared, "Control-Write-Encryption-Key"), 0, 0));
                    continue;
                }
                ("POST", "/pairings", Some(1)) => {
                    let id = String::from_utf8_lossy(tlv::get(&items, tlv::IDENTIFIER).unwrap()).into_owned();
                    state.lock().unwrap().controllers.retain(|(c, _)| *c != id);
                    tlv::encode(&[(tlv::STATE, &[2])])
                }
                ("GET", "/accessories", _) => {
                    let position = state.lock().unwrap().position;
                    let body = database(position).to_string();
                    reply(&mut socket, &mut keys, "200 OK", "application/hap+json", body.as_bytes()).await;
                    continue;
                }
                ("GET", p, _) if p.starts_with("/characteristics?id=") => {
                    let position = state.lock().unwrap().position;
                    let body = json!({"characteristics": [{"aid": 2, "iid": 9, "value": position}, {"aid": 2, "iid": 10, "value": position}, {"aid": 2, "iid": 11, "value": 2}]})
                        .to_string();
                    reply(&mut socket, &mut keys, "200 OK", "application/hap+json", body.as_bytes()).await;
                    continue;
                }
                ("PUT", "/characteristics", _) => {
                    let v: Value = serde_json::from_slice(&body).unwrap();
                    let cs = v["characteristics"].as_array().unwrap();
                    if cs.iter().all(|c| c["ev"] == true) {
                        // A subscription: this session only gets events from now on.
                        let mut rx = events.subscribe();
                        state.lock().unwrap().subscribed.extend(cs.iter().map(|c| (c["aid"].as_u64().unwrap(), c["iid"].as_u64().unwrap())));
                        reply(&mut socket, &mut keys, "204 No Content", "application/hap+json", b"").await;
                        while let Ok(event) = rx.recv().await {
                            let body = event.to_string();
                            let mut data = format!("EVENT/1.0 200 OK\r\nContent-Type: application/hap+json\r\nContent-Length: {}\r\n\r\n", body.len()).into_bytes();
                            data.extend_from_slice(body.as_bytes());
                            let Some((write, _, count, _)) = keys.as_mut() else { return };
                            let mut out = vec![];
                            for chunk in data.chunks(1024) {
                                let len = (chunk.len() as u16).to_le_bytes();
                                out.extend_from_slice(&len);
                                out.extend(seal(write, count.to_le_bytes(), &len, chunk));
                                *count += 1;
                            }
                            if socket.write_all(&out).await.is_err() {
                                return;
                            }
                        }
                        return;
                    }
                    {
                        let mut st = state.lock().unwrap();
                        for c in cs {
                            st.written.push((c["aid"].as_u64().unwrap(), c["iid"].as_u64().unwrap(), c["value"].clone()));
                            if c["iid"] == 10 {
                                st.position = c["value"].as_i64().unwrap();
                            }
                        }
                    }
                    reply(&mut socket, &mut keys, "204 No Content", "application/hap+json", b"").await;
                    continue;
                }
                _ => {
                    reply(&mut socket, &mut keys, "404 Not Found", "text/plain", b"").await;
                    continue;
                }
            };
            reply(&mut socket, &mut keys, "200 OK", "application/pairing+tlv8", &answer).await;
        }
    }
}
