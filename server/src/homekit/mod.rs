//! A HomeKit controller (HAP over IP): Breezone pairs with an accessory (a Legrand gateway, a VELUX
//! gateway…) with its setup code, then reads and drives it on the local network, through encrypted
//! sessions. No Apple device or account is involved.
//!
//! Pairing with Breezone changes nothing else on the accessory: the devices its own app set up stay
//! as they are. Breezone never resets an accessory: the only pairing it removes is its own.

pub mod discovery;
pub mod session;
mod srp;
mod tlv;

use ed25519_dalek::SigningKey;
use serde::{Deserialize, Serialize};
pub use session::Connection;
use session::{hkdf, label, open, seal};

/// What an accessory and Breezone keep of their pairing: the accessory's identity and long-term
/// public key, Breezone's identity and long-term secret key (never shown, stored encrypted).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pairing {
    pub accessory_id: String,
    /// Hex.
    pub accessory_ltpk: String,
    pub controller_id: String,
    /// Hex.
    pub controller_ltsk: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HapError {
    Unreachable(String),
    Protocol(String),
    /// An error code of the accessory (HAP, table 5-5).
    Accessory(u8),
}

impl HapError {
    /// What it means for the person pairing.
    pub fn explain(&self) -> crate::i18n::Text {
        match self {
            HapError::Accessory(2) => crate::text!(
                "Code incorrect : vérifiez le code HomeKit à 8 chiffres de l'accessoire.",
                "Wrong code: check the 8-digit HomeKit code of the accessory.",
                "Código incorrecto: compruebe el código HomeKit de 8 cifras del accesorio."
            ),
            HapError::Accessory(3) | HapError::Accessory(5) => crate::text!(
                "Trop d'essais : l'accessoire refuse l'appairage pour l'instant. Réessayez plus tard.",
                "Too many tries: the accessory refuses pairing for now. Try again later.",
                "Demasiados intentos: el accesorio rechaza el emparejamiento por ahora. Inténtelo más tarde."
            ),
            HapError::Accessory(4) => {
                crate::text!("L'accessoire n'accepte plus d'autre contrôleur.", "The accessory accepts no other controller.", "El accesorio no acepta más controladores.")
            }
            HapError::Accessory(6) => crate::text!(
                "L'accessoire est déjà appairé à un autre contrôleur HomeKit.",
                "The accessory is already paired with another HomeKit controller.",
                "El accesorio ya está emparejado con otro controlador HomeKit."
            ),
            HapError::Accessory(7) => crate::text!(
                "L'accessoire est occupé par un autre appairage. Réessayez dans un instant.",
                "The accessory is busy with another pairing. Try again in a moment.",
                "El accesorio está ocupado con otro emparejamiento. Inténtelo de nuevo en un momento."
            ),
            HapError::Accessory(code) => crate::text!("L'accessoire refuse (erreur {0}).", "The accessory refuses (error {0}).", "El accesorio rechaza (error {0}).", code),
            HapError::Unreachable(e) => crate::text!("Accessoire injoignable : {0}", "Accessory unreachable: {0}", "Accesorio inaccesible: {0}", e),
            HapError::Protocol(e) => {
                crate::text!("Réponse inattendue de l'accessoire : {0}", "Unexpected answer from the accessory: {0}", "Respuesta inesperada del accesorio: {0}", e)
            }
        }
    }
}

impl std::fmt::Display for HapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HapError::Unreachable(e) => write!(f, "unreachable: {e}"),
            HapError::Protocol(e) => write!(f, "{e}"),
            HapError::Accessory(code) => write!(f, "the accessory refuses (error {code})"),
        }
    }
}

/// "123-45-678", from the code as typed ("12345678", "123 45 678"…).
pub fn setup_code(typed: &str) -> Option<String> {
    let digits: String = typed.chars().filter(char::is_ascii_digit).collect();
    if digits.len() != 8 || typed.chars().any(|c| !(c.is_ascii_digit() || c == '-' || c == ' ')) {
        return None;
    }
    Some(format!("{}-{}-{}", &digits[..3], &digits[3..5], &digits[5..]))
}

/// Pair Setup (HAP, chapter 5.6), with the accessory's setup code. `with_auth`: the accessory
/// announces Apple authentication (feature flag 1), as most certified ones do.
pub async fn pair_setup(addr: &str, code: &str, with_auth: bool) -> Result<Pairing, HapError> {
    let mut conn = Connection::connect(addr).await?;
    let method = [if with_auth { 1u8 } else { 0 }];
    let m2 = conn.tlv("/pair-setup", &[(tlv::STATE, &[1]), (tlv::METHOD, &method)]).await?;
    let salt = tlv::get(&m2, tlv::SALT).ok_or_else(|| HapError::Protocol("pair-setup: no salt".into()))?.to_vec();
    let b_pub = tlv::get(&m2, tlv::PUBLIC_KEY).ok_or_else(|| HapError::Protocol("pair-setup: no public key".into()))?.to_vec();

    let client = srp::Client::new(&crate::auth::random_bytes::<32>());
    let agreed = client.agree(&salt, &b_pub, code).map_err(HapError::Protocol)?;
    let m4 = conn.tlv("/pair-setup", &[(tlv::STATE, &[3]), (tlv::PUBLIC_KEY, &client.public), (tlv::PROOF, &agreed.proof)]).await?;
    if tlv::get(&m4, tlv::PROOF) != Some(agreed.expected.as_slice()) {
        return Err(HapError::Protocol("the accessory's proof does not match".into()));
    }

    // Breezone's long-term identity for this accessory.
    let controller_id = format!("breezone-{}", hex::encode(crate::auth::random_bytes::<6>()));
    let me = SigningKey::from_bytes(&crate::auth::random_bytes::<32>());
    let ltpk = me.verifying_key().to_bytes();
    let x = hkdf("Pair-Setup-Controller-Sign-Salt", &agreed.key, "Pair-Setup-Controller-Sign-Info");
    let mut info = x.to_vec();
    info.extend_from_slice(controller_id.as_bytes());
    info.extend_from_slice(&ltpk);
    use ed25519_dalek::Signer;
    let signature = me.sign(&info).to_bytes();
    let inner = tlv::encode(&[(tlv::IDENTIFIER, controller_id.as_bytes()), (tlv::PUBLIC_KEY, &ltpk), (tlv::SIGNATURE, &signature)]);
    let key = hkdf("Pair-Setup-Encrypt-Salt", &agreed.key, "Pair-Setup-Encrypt-Info");
    let sealed = seal(&key, label("PS-Msg05"), &[], &inner);
    let m6 = conn.tlv("/pair-setup", &[(tlv::STATE, &[5]), (tlv::ENCRYPTED_DATA, &sealed)]).await?;

    let sealed = tlv::get(&m6, tlv::ENCRYPTED_DATA).ok_or_else(|| HapError::Protocol("pair-setup: no data".into()))?;
    let inner = tlv::decode(&open(&key, label("PS-Msg06"), &[], sealed)?).map_err(HapError::Protocol)?;
    let accessory_id = tlv::get(&inner, tlv::IDENTIFIER).ok_or_else(|| HapError::Protocol("pair-setup: no identifier".into()))?;
    let accessory_ltpk = tlv::get(&inner, tlv::PUBLIC_KEY).ok_or_else(|| HapError::Protocol("pair-setup: no key".into()))?;
    let pairing = Pairing {
        accessory_id: String::from_utf8_lossy(accessory_id).into_owned(),
        accessory_ltpk: hex::encode(accessory_ltpk),
        controller_id,
        controller_ltsk: hex::encode(me.to_bytes()),
    };
    let signature =
        tlv::get(&inner, tlv::SIGNATURE).and_then(|s| ed25519_dalek::Signature::from_slice(s).ok()).ok_or_else(|| HapError::Protocol("pair-setup: no signature".into()))?;
    let x = hkdf("Pair-Setup-Accessory-Sign-Salt", &agreed.key, "Pair-Setup-Accessory-Sign-Info");
    let mut info = x.to_vec();
    info.extend_from_slice(accessory_id);
    info.extend_from_slice(accessory_ltpk);
    use ed25519_dalek::Verifier;
    pairing.accessory_key()?.verify(&info, &signature).map_err(|_| HapError::Protocol("the accessory's signature does not match".into()))?;
    Ok(pairing)
}

/// Removes Breezone's own pairing from the accessory (and nothing else): the accessory forgets
/// Breezone, and can be paired again.
pub async fn remove_own_pairing(addr: &str, pairing: &Pairing) -> Result<(), HapError> {
    let mut conn = Connection::verify(addr, pairing).await?;
    conn.tlv("/pairings", &[(tlv::STATE, &[1]), (tlv::METHOD, &[4]), (tlv::IDENTIFIER, pairing.controller_id.as_bytes())]).await?;
    Ok(())
}

#[cfg(test)]
pub(crate) mod fake {
    //! An accessory that pairs and serves its database on an encrypted session, enough to test
    //! the controller end to end.

    use super::session::{hkdf, label, open, seal};
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
    }

    pub struct Fake {
        pub port: u16,
        pub state: Arc<Mutex<State>>,
    }

    pub async fn start() -> Fake {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let state = Arc::new(Mutex::new(State { position: 40, ..Default::default() }));
        let key = SigningKey::from_bytes(&[42; 32]);
        let st = state.clone();
        tokio::spawn(async move {
            loop {
                let Ok((socket, _)) = listener.accept().await else { return };
                tokio::spawn(serve(socket, key.clone(), st.clone()));
            }
        });
        Fake { port, state }
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
                    {"iid": 11, "type": "00000072-0000-1000-8000-0026BB765291", "value": 2, "perms": ["pr", "ev"], "format": "uint8"}]}]}
        ]})
    }

    async fn serve(mut socket: TcpStream, key: SigningKey, state: Arc<Mutex<State>>) {
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
                        let server = srp::Server::new(&[5; 16], CODE, &[7; 32]);
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
                    {
                        let mut st = state.lock().unwrap();
                        for c in v["characteristics"].as_array().unwrap() {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_codes_as_typed() {
        assert_eq!(setup_code("03145154").as_deref(), Some("031-45-154"));
        assert_eq!(setup_code("031-45-154").as_deref(), Some("031-45-154"));
        assert_eq!(setup_code("031 45 154").as_deref(), Some("031-45-154"));
        assert_eq!(setup_code("0314515"), None);
        assert_eq!(setup_code("03145154a"), None);
    }

    #[tokio::test]
    async fn pairs_reads_writes_and_removes_its_own_pairing() {
        let fake = fake::start().await;
        let addr = format!("127.0.0.1:{}", fake.port);

        // A wrong code is refused and pairs nothing.
        assert_eq!(pair_setup(&addr, "111-11-111", true).await, Err(HapError::Accessory(2)));
        let pairing = pair_setup(&addr, fake::CODE, true).await.expect("pairs");
        assert_eq!(pairing.accessory_id, "AA:BB:CC:DD:EE:FF");
        assert_eq!(fake.state.lock().unwrap().controllers.len(), 1);
        // Already paired: another controller cannot take it.
        assert_eq!(pair_setup(&addr, fake::CODE, true).await, Err(HapError::Accessory(6)));

        // An encrypted session: the database, a read, a write.
        let mut conn = Connection::verify(&addr, &pairing).await.expect("verifies");
        let db = conn.get_json("/accessories").await.unwrap();
        assert_eq!(db["accessories"][1]["services"][1]["characteristics"][0]["value"], 40);
        conn.put_json("/characteristics", &serde_json::json!({"characteristics": [{"aid": 2, "iid": 10, "value": 75}]})).await.unwrap();
        let values = conn.get_json("/characteristics?id=2.9,2.10,2.11").await.unwrap();
        assert_eq!(values["characteristics"][0]["value"], 75);

        // A pairing that is not the accessory's is refused.
        let mut other = pairing.clone();
        other.controller_id = "someone-else".into();
        assert!(Connection::verify(&addr, &other).await.is_err());

        // Breezone removes its own pairing, and only it.
        remove_own_pairing(&addr, &pairing).await.unwrap();
        assert!(fake.state.lock().unwrap().controllers.is_empty());
    }
}
