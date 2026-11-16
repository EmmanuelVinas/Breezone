//! Encryption of the secrets kept in the database (Airzone Cloud password, Hue keys, HomeKit
//! pairings, this node's federation key), with a key of its own: `<data_dir>/data.key`.
//!
//! The key used to be derived from the JWT secret, so changing that secret (to sign everyone
//! out) made every stored secret unreadable. A server that already holds encrypted values keeps
//! the key it had (written to `data.key` once): nothing is re-encrypted, and an older version
//! started again still reads them.

use crate::auth::random_bytes;
use crate::db::Db;
use anyhow::{bail, Context};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use sha2::{Digest, Sha256};
use std::path::Path;

pub type Key = [u8; 32];

const FILE: &str = "data.key";

/// The key of the versions before `data.key`.
fn legacy_key(jwt_secret: &[u8]) -> Key {
    let mut h = Sha256::new();
    h.update(b"breezone/airzone-cloud-credentials/");
    h.update(jwt_secret);
    h.finalize().into()
}

async fn holds_secrets(db: &Db) -> sqlx::Result<bool> {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM settings WHERE key IN ('cloud.password', 'node.signing_key')) OR EXISTS (SELECT 1 FROM sources WHERE secret IS NOT NULL)")
        .fetch_one(db)
        .await
}

/// Reads `data.key`, or creates it: with the key of the JWT secret when the database already holds
/// secrets encrypted with it, a random one otherwise. A damaged file stops the server instead of
/// being replaced, as replacing it would lose every stored secret.
pub async fn load_key(data_dir: &Path, db: &Db, jwt_secret: &[u8]) -> anyhow::Result<Key> {
    let path = data_dir.join(FILE);
    if path.exists() {
        let text = std::fs::read_to_string(&path).with_context(|| format!("cannot read {}", path.display()))?;
        let Some(key) = hex::decode(text.trim()).ok().and_then(|b| Key::try_from(b).ok()) else {
            bail!("{} is damaged (64 hexadecimal characters expected): restore it, or remove it and enter the stored passwords and pairings again", path.display());
        };
        return Ok(key);
    }
    let key = if holds_secrets(db).await? {
        tracing::info!("encryption key moved to {} (it no longer depends on the JWT secret)", path.display());
        legacy_key(jwt_secret)
    } else {
        random_bytes::<32>()
    };
    std::fs::create_dir_all(data_dir)?;
    std::fs::write(&path, hex::encode(key)).with_context(|| format!("cannot write {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(key)
}

pub fn encrypt(key: &Key, plain: &str) -> String {
    let cipher = ChaCha20Poly1305::new_from_slice(key).expect("32-byte key");
    let nonce_bytes = random_bytes::<12>();
    let ct = cipher.encrypt(&Nonce::from(nonce_bytes), plain.as_bytes()).expect("encryption");
    let mut out = nonce_bytes.to_vec();
    out.extend(ct);
    STANDARD.encode(out)
}

/// `None` when the value was encrypted with another key (or is not an encrypted value).
pub fn decrypt(key: &Key, encoded: &str) -> Option<String> {
    let raw = STANDARD.decode(encoded).ok()?;
    if raw.len() < 13 {
        return None;
    }
    let cipher = ChaCha20Poly1305::new_from_slice(key).ok()?;
    let nonce_bytes: [u8; 12] = raw[..12].try_into().ok()?;
    let plain = cipher.decrypt(&Nonce::from(nonce_bytes), &raw[12..]).ok()?;
    String::from_utf8(plain).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encryption_roundtrip() {
        let key = legacy_key(b"server-secret");
        let enc = encrypt(&key, "mon mot de passe");
        assert_ne!(enc, "mon mot de passe");
        assert_eq!(decrypt(&key, &enc).as_deref(), Some("mon mot de passe"));
        assert_eq!(decrypt(&legacy_key(b"other"), &enc), None);
        assert_ne!(encrypt(&key, "x"), encrypt(&key, "x"), "random nonce");
    }

    async fn db_in(dir: &Path) -> Db {
        crate::db::connect(&dir.join("breezone.db")).await.unwrap()
    }

    #[tokio::test]
    async fn an_existing_server_keeps_the_key_of_its_jwt_secret() {
        let dir = tempdir();
        let db = db_in(&dir).await;
        let old = encrypt(&legacy_key(b"jwt-1"), "hue key");
        sqlx::query("INSERT INTO settings (key, value) VALUES ('cloud.password', ?)").bind(&old).execute(&db).await.unwrap();

        let key = load_key(&dir, &db, b"jwt-1").await.unwrap();
        assert_eq!(decrypt(&key, &old).as_deref(), Some("hue key"));
        // The JWT secret changes: the stored secrets stay readable.
        assert_eq!(load_key(&dir, &db, b"jwt-2").await.unwrap(), key);
    }

    #[tokio::test]
    async fn a_new_server_gets_a_random_key() {
        let dir = tempdir();
        let db = db_in(&dir).await;
        let key = load_key(&dir, &db, b"jwt-1").await.unwrap();
        assert_ne!(key, legacy_key(b"jwt-1"));
        assert_eq!(load_key(&dir, &db, b"jwt-1").await.unwrap(), key);
    }

    #[tokio::test]
    async fn a_damaged_key_file_is_not_replaced() {
        let dir = tempdir();
        let db = db_in(&dir).await;
        std::fs::write(dir.join(FILE), "abc").unwrap();
        assert!(load_key(&dir, &db, b"jwt-1").await.is_err());
        assert_eq!(std::fs::read_to_string(dir.join(FILE)).unwrap(), "abc");
    }

    fn tempdir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("breezone-crypto-{}", hex::encode(random_bytes::<6>())));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
