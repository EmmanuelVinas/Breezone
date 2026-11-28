//! Certificates of other Breezone nodes.
//!
//! By default a node is reached with the usual HTTPS checks (trusted authority, matching name,
//! validity dates). When an administrator decides to trust a certificate that fails them (e.g. a
//! NAS certificate that does not cover the reverse proxy name, or a self-signed one), that exact
//! certificate is pinned by its SHA-256 fingerprint: it is accepted whatever its name or issuer,
//! and any other certificate is refused, so that trusting it does not open the door to someone
//! impersonating the node.

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Message of the error raised when a pinned node presents another certificate.
pub const PIN_MISMATCH: &str = "pinned certificate mismatch";

/// SHA-256 of a certificate (DER), lowercase hex.
pub fn fingerprint(der: &[u8]) -> String {
    hex::encode(Sha256::digest(der))
}

/// Normalises a fingerprint typed or copied as `AB:CD:…`.
pub fn normalize_fingerprint(s: &str) -> Option<String> {
    let hex: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect::<String>().to_lowercase();
    (hex.len() == 64).then_some(hex)
}

fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// What an administrator sees before trusting a certificate.
#[derive(Debug, Clone, Serialize)]
pub struct CertificateInfo {
    /// SHA-256, lowercase hex.
    pub fingerprint: String,
    pub subject: Option<String>,
    /// Names the certificate is valid for (DNS names and IP addresses).
    pub names: Vec<String>,
    pub issuer: Option<String>,
    pub not_before: Option<i64>,
    pub not_after: Option<i64>,
    pub self_signed: bool,
}

pub fn describe_certificate(der: &[u8]) -> CertificateInfo {
    let mut info = CertificateInfo { fingerprint: fingerprint(der), subject: None, names: vec![], issuer: None, not_before: None, not_after: None, self_signed: false };
    if let Ok((_, cert)) = x509_parser::parse_x509_certificate(der) {
        let cn = |name: &x509_parser::x509::X509Name| name.iter_common_name().next().and_then(|a| a.as_str().ok()).map(str::to_string);
        info.subject = cn(cert.subject()).or_else(|| Some(cert.subject().to_string()).filter(|s| !s.is_empty()));
        info.issuer = cn(cert.issuer()).or_else(|| Some(cert.issuer().to_string()).filter(|s| !s.is_empty()));
        info.self_signed = cert.subject() == cert.issuer();
        info.not_before = Some(cert.validity().not_before.timestamp());
        info.not_after = Some(cert.validity().not_after.timestamp());
        if let Ok(Some(san)) = cert.subject_alternative_name() {
            for name in &san.value.general_names {
                match name {
                    x509_parser::extensions::GeneralName::DNSName(d) => info.names.push(d.to_string()),
                    x509_parser::extensions::GeneralName::IPAddress(ip) => {
                        if let Ok(a) = <[u8; 4]>::try_from(*ip) {
                            info.names.push(std::net::Ipv4Addr::from(a).to_string());
                        } else if let Ok(a) = <[u8; 16]>::try_from(*ip) {
                            info.names.push(std::net::Ipv6Addr::from(a).to_string());
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    info
}

/// Accepts one certificate only (by fingerprint), whatever its name, issuer or dates; the
/// handshake signatures are still checked, so the node must hold the matching private key.
#[derive(Debug)]
struct Pinned {
    fingerprint: String,
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for Pinned {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        if fingerprint(end_entity) == self.fingerprint {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General(PIN_MISMATCH.into()))
        }
    }
    fn verify_tls12_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }
    fn verify_tls13_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

/// Records the certificate presented, without judging it (only to show it to an administrator).
#[derive(Debug)]
struct Recorder {
    seen: Arc<Mutex<Option<Vec<u8>>>>,
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for Recorder {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        *self.seen.lock().unwrap_or_else(|e| e.into_inner()) = Some(end_entity.to_vec());
        // Stop here: nothing is sent to a server whose certificate is not trusted.
        Err(rustls::Error::General("certificate recorded".into()))
    }
    fn verify_tls12_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }
    fn verify_tls13_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

fn builder() -> reqwest::ClientBuilder {
    reqwest::Client::builder().connect_timeout(Duration::from_secs(4)).timeout(Duration::from_secs(10)).user_agent(concat!("Breezone/", env!("CARGO_PKG_VERSION")))
}

fn custom_tls(verifier: Arc<dyn ServerCertVerifier>) -> Option<rustls::ClientConfig> {
    let config = rustls::ClientConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .ok()?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    Some(config)
}

/// HTTP client towards a node: usual checks, or only the pinned certificate.
pub fn node_http_client(pinned: Option<&str>) -> reqwest::Client {
    match pinned.and_then(|fp| custom_tls(Arc::new(Pinned { fingerprint: fp.to_string(), provider: provider() }))) {
        Some(tls) => builder().use_preconfigured_tls(tls).build().unwrap_or_default(),
        None => builder().build().unwrap_or_default(),
    }
}

/// Reads the certificate an HTTPS address presents (nothing is sent to it).
pub async fn fetch_certificate(url: &str) -> Option<CertificateInfo> {
    if !url.starts_with("https://") {
        return None;
    }
    let seen = Arc::new(Mutex::new(None));
    let tls = custom_tls(Arc::new(Recorder { seen: seen.clone(), provider: provider() }))?;
    let client = builder().use_preconfigured_tls(tls).build().ok()?;
    let _ = client.get(format!("{}/health", url.trim_end_matches('/'))).send().await;
    let der = seen.lock().unwrap_or_else(|e| e.into_inner()).take()?;
    Some(describe_certificate(&der))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprints_are_normalised() {
        let fp = "AB:".repeat(31) + "AB";
        assert_eq!(normalize_fingerprint(&fp), Some("ab".repeat(32)));
        assert_eq!(normalize_fingerprint("abc"), None);
    }
}

/// Accepts the certificate a device makes for itself (its handshake signatures still checked): for
/// hardware on the local network only. Old devices sign with short RSA keys, which ring refuses
/// below 2048 bits, 1024 in its legacy algorithms ("invalid peer certificate: BadSignature"): the
/// Delta Dore Tydom 1.0 has a key of 512 bits (seen on a real one, 2026-11-28). Those signatures
/// are checked by `rsa`, whatever the size of the key. The certificate itself is not trusted
/// anyway, so a short key weakens nothing here.
#[cfg(feature = "wasm")]
#[derive(Debug)]
struct Device {
    provider: Arc<CryptoProvider>,
}

#[cfg(feature = "wasm")]
impl ServerCertVerifier for Device {
    fn verify_server_cert(&self, _: &CertificateDer<'_>, _: &[CertificateDer<'_>], _: &ServerName<'_>, _: &[u8], _: UnixTime) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms).or_else(|e| {
            if legacy_rsa(message, cert, dss.scheme, dss.signature()) {
                Ok(HandshakeSignatureValid::assertion())
            } else {
                Err(e)
            }
        })
    }
    fn verify_tls13_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        // PKCS#1 first, which a short key can be checked with, then SHA-1 for the oldest devices.
        let mut schemes = self.provider.signature_verification_algorithms.supported_schemes();
        schemes.sort_by_key(|s| !LEGACY_RSA.contains(s));
        schemes.push(SignatureScheme::RSA_PKCS1_SHA1);
        schemes
    }
}

#[cfg(feature = "wasm")]
const LEGACY_RSA: [SignatureScheme; 4] = [SignatureScheme::RSA_PKCS1_SHA512, SignatureScheme::RSA_PKCS1_SHA384, SignatureScheme::RSA_PKCS1_SHA256, SignatureScheme::RSA_PKCS1_SHA1];

/// A TLS 1.2 signature by an RSA key of any size (PKCS#1 v1.5), checked by the device's certificate.
#[cfg(feature = "wasm")]
fn legacy_rsa(message: &[u8], cert: &CertificateDer<'_>, scheme: SignatureScheme, sig: &[u8]) -> bool {
    use ring::digest;
    use x509_parser::public_key::PublicKey;
    // The hash, and the DigestInfo that names it in the signature (RFC 8017 § 9.2, note 1).
    let (algorithm, prefix): (&digest::Algorithm, &[u8]) = match scheme {
        SignatureScheme::RSA_PKCS1_SHA1 => (&digest::SHA1_FOR_LEGACY_USE_ONLY, &[0x30, 0x21, 0x30, 0x09, 0x06, 0x05, 0x2b, 0x0e, 0x03, 0x02, 0x1a, 0x05, 0x00, 0x04, 0x14]),
        SignatureScheme::RSA_PKCS1_SHA256 => (&digest::SHA256, &[0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01, 0x05, 0x00, 0x04, 0x20]),
        SignatureScheme::RSA_PKCS1_SHA384 => (&digest::SHA384, &[0x30, 0x41, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x02, 0x05, 0x00, 0x04, 0x30]),
        SignatureScheme::RSA_PKCS1_SHA512 => (&digest::SHA512, &[0x30, 0x51, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x03, 0x05, 0x00, 0x04, 0x40]),
        _ => return false,
    };
    let Ok((_, parsed)) = x509_parser::parse_x509_certificate(cert) else { return false };
    let Ok(PublicKey::RSA(key)) = parsed.public_key().parsed() else { return false };
    let Ok(key) = rsa::RsaPublicKey::new(rsa::BigUint::from_bytes_be(key.modulus), rsa::BigUint::from_bytes_be(key.exponent)) else { return false };
    let hashed = digest::digest(algorithm, message);
    key.verify(rsa::Pkcs1v15Sign { hash_len: Some(hashed.as_ref().len()), prefix: prefix.into() }, hashed.as_ref(), sig).is_ok()
}

/// TLS towards hardware or a service: the usual checks, or (`device`) the device's own certificate.
#[cfg(feature = "wasm")]
pub fn client_config(device: bool) -> Option<rustls::ClientConfig> {
    if device {
        return custom_tls(Arc::new(Device { provider: provider() }));
    }
    let roots = rustls::RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
    Some(rustls::ClientConfig::builder_with_provider(provider()).with_safe_default_protocol_versions().ok()?.with_root_certificates(roots).with_no_client_auth())
}

#[cfg(all(test, feature = "wasm"))]
mod device_tests {
    use super::*;

    /// Device certificates with RSA keys of 512 (as the Tydom 1.0) and 1024 bits, and "handshake"
    /// signed by each (openssl).
    const CERT_512: &[u8] = include_bytes!("testdata/rsa512.der");
    const CERT_1024: &[u8] = include_bytes!("testdata/rsa1024.der");

    /// What ring says of a signature, with its usual algorithm or its legacy one.
    fn ring_accepts(algorithm: &'static dyn ring::signature::VerificationAlgorithm, cert: &[u8], sig: &[u8]) -> bool {
        let key = x509_parser::parse_x509_certificate(cert).unwrap().1.public_key().subject_public_key.data.to_vec();
        ring::signature::UnparsedPublicKey::new(algorithm, key).verify(b"handshake", sig).is_ok()
    }

    #[test]
    fn an_old_device_signs_with_a_short_rsa_key() {
        for (der, sha256, sha1) in [
            (CERT_512, &include_bytes!("testdata/rsa512.sha256.sig")[..], &include_bytes!("testdata/rsa512.sha1.sig")[..]),
            (CERT_1024, &include_bytes!("testdata/rsa1024.sha256.sig")[..], &include_bytes!("testdata/rsa1024.sha1.sig")[..]),
        ] {
            let cert = CertificateDer::from(der);
            assert!(!ring_accepts(&ring::signature::RSA_PKCS1_2048_8192_SHA256, der, sha256), "too short for ring: the error behind BadSignature");
            assert!(legacy_rsa(b"handshake", &cert, SignatureScheme::RSA_PKCS1_SHA256, sha256));
            assert!(legacy_rsa(b"handshake", &cert, SignatureScheme::RSA_PKCS1_SHA1, sha1));
            assert!(!legacy_rsa(b"something else", &cert, SignatureScheme::RSA_PKCS1_SHA256, sha256), "a wrong signature stays refused");
            assert!(!legacy_rsa(b"handshake", &cert, SignatureScheme::RSA_PKCS1_SHA512, sha256), "another hash stays refused");
            assert!(!legacy_rsa(b"handshake", &cert, SignatureScheme::RSA_PSS_SHA256, sha256));
        }
        // 512 bits: even ring's legacy algorithms refuse it (the Tydom 1.0, beta.12).
        assert!(!ring_accepts(&ring::signature::RSA_PKCS1_1024_8192_SHA256_FOR_LEGACY_USE_ONLY, CERT_512, include_bytes!("testdata/rsa512.sha256.sig")));
        let schemes = Device { provider: provider() }.supported_verify_schemes();
        assert_eq!((schemes.first(), schemes.last()), (Some(&SignatureScheme::RSA_PKCS1_SHA512), Some(&SignatureScheme::RSA_PKCS1_SHA1)));
    }
}
