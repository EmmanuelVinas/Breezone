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
#[cfg(test)]
pub struct Server {
    b: BigUint,
    v: BigUint,
    pub salt: Vec<u8>,
    pub public: Vec<u8>,
}

#[cfg(test)]
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Computed apart with Python integers from the formulas of RFC 5054 and of the reference
    /// HomeKit controller: the arithmetic and the hashed fields must match exactly.
    #[test]
    fn matches_an_independent_computation() {
        let server = Server::new(&[5; 16], "031-45-154", &[7; 32]);
        assert_eq!(hex::encode(&server.public), "a852b0ea39361ffb5f7c21699edcc4e3abdfbc23a8586d851b65ffac62b13ea2394ae5685e990a98109d726368591f40e5e36248db0a20de758025f44c43c3d93540e77068150c5e0284f70b174390e2a72260c8be910f6dcd972a58516f80c223740b4c8f4cc17da6d08f4d87fe3bdd2c1a4167f89bb2738224f98641fad1e315e10269dc551b840c076d6b2a937d52fdf023de9249ec59f16dec36dbf604ac215e1535ea6430a5022aa5edfa97c1ea0f62b0adccb7846bd295a74f7ea7981d3936cec6bf854d822f5d1629396480ff885f398f59c6338dbb38fe59aba867b2fe87410e5a80476203b9b4d71cc6d41feb5e32e19b064c95163c2458594b437bb4ee79aa39f409ef2ca5da2bc49109500f6e7d015729d6bfcec7a4e7c869ae54bff8af37c18ccf46a72ede8242490145cbf927f030320395b66a59b5adab1f75d03d15ec89b712af3e4625eca263e639ca298ca7d46a1cf76abcf6f87302b571f32c67468817ce7be1f4cc083dabddc259909d597f18c93ddd14b88ff8161eb3");
        let agreed = Client::new(&[4; 32]).agree(&[5; 16], &server.public, "031-45-154").unwrap();
        assert_eq!(hex::encode(&agreed.key), "a846ba7f39863fa02db377f92e1a016f7d23a24cadb8c6c65043827cb91d0aba105f9669a15b584f06216d67a99c5d44e9b92a107088b1008585d1f620c14c27");
        assert_eq!(hex::encode(&agreed.proof), "f596b6bb7cd392677b146f4985c8220dee268f22d48e800ee93876ac6c3ddee58c8e8667d0ddaa65d039409e4d4149009998d6bc3c082cfa37bc864147571a6d");
        assert_eq!(
            hex::encode(&agreed.expected),
            "6cf1116738fcae66fffcef7673e5a569f79318ec29e6ed22f8e79e4527675f13c789f1d5f52ab74a78eb13f3578f88dabb15ac45d5596d85eda6160021e033b4"
        );
    }

    #[test]
    fn controller_and_accessory_agree_with_the_right_code_only() {
        let server = Server::new(&[3; 16], "111-22-333", &[9; 32]);
        let client = Client::new(&[4; 32]);
        let agreed = client.agree(&server.salt, &server.public, "111-22-333").unwrap();
        let (key, m2) = server.verify(&client.public, &agreed.proof).expect("right code");
        assert_eq!(key, agreed.key);
        assert_eq!(m2, agreed.expected);
        let wrong = client.agree(&server.salt, &server.public, "111-22-334").unwrap();
        assert!(server.verify(&client.public, &wrong.proof).is_none());
    }
}
