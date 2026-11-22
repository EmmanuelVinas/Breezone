//! A module's package (docs/roadmap/modules.md § 4.3): `<id>-<version>.bzm`, a `.tar.gz` holding
//! `manifest.json`, `module.wasm` (unless the module is only data), `i18n/{fr,en,es}.json` and
//! `icon.svg`, signed like the releases of Breezone, with the same key but a message of its own:
//! `breezone-module <id> <version>\n<sha256>\n`. A signature of a module never passes for one of a
//! release, nor the other way round.

use super::manifest::Manifest;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Largest package accepted, and largest file in it.
pub const MAX_PACKAGE: usize = 16 << 20;
const MAX_FILE: u64 = 16 << 20;
const MAX_ENTRIES: usize = 16;

/// What the publishing workflow signs for a module.
pub fn signed_message(id: &str, version: &str, package: &[u8]) -> Vec<u8> {
    format!("breezone-module {id} {version}\n{}\n", hex::encode(Sha256::digest(package))).into_bytes()
}

/// Checks an Ed25519 signature (base64, as published) of `message` with `public_key` (base64).
pub fn verify_signature(public_key: &str, message: &[u8], signature: &str) -> bool {
    let Some(key) = STANDARD.decode(public_key.trim()).ok().and_then(|k| <[u8; 32]>::try_from(k).ok()) else { return false };
    let Ok(key) = VerifyingKey::from_bytes(&key) else { return false };
    let Some(sig) = STANDARD.decode(signature.trim()).ok().and_then(|s| <[u8; 64]>::try_from(s).ok()) else { return false };
    key.verify_strict(message, &Signature::from_bytes(&sig)).is_ok()
}

/// A package read and checked, in memory.
#[derive(Debug, Clone)]
pub struct Package {
    pub manifest: Manifest,
    /// The component, absent for a module that is only data.
    pub wasm: Option<Vec<u8>>,
    /// Its words, by language.
    pub i18n: BTreeMap<String, Vec<u8>>,
    pub icon: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum PackageError {
    #[error("the signature does not match this package")]
    Signature,
    #[error("the package is not readable: {0}")]
    Unreadable(String),
    #[error("the package holds {0}, which a module may not")]
    Unexpected(String),
    #[error("the manifest is wrong: {0}")]
    Manifest(String),
    #[error("the package is {0}, not the one asked")]
    Other(String),
}

impl Package {
    /// Reads `bytes`, signed by `signature` with `public_key`, as the package of module `id` at
    /// `version` (when they are known beforehand, from the library).
    pub fn open(bytes: &[u8], signature: &str, public_key: &str, expected: Option<(&str, &str)>) -> Result<Package, PackageError> {
        if bytes.len() > MAX_PACKAGE {
            return Err(PackageError::Unreadable("too large".into()));
        }
        let package = Package::read(bytes)?;
        let (id, version) = (package.manifest.id.as_str(), package.manifest.version.as_str());
        if let Some((want_id, want_version)) = expected {
            if (id, version) != (want_id, want_version) {
                return Err(PackageError::Other(format!("{id} {version}")));
            }
        }
        if !verify_signature(public_key, &signed_message(id, version, bytes), signature) {
            return Err(PackageError::Signature);
        }
        Ok(package)
    }

    /// Reads a package that comes with Breezone (beside it in the image, or in its archive, signed
    /// as a whole): same checks as [`Package::open`] but the signature.
    pub fn trusted(bytes: &[u8]) -> Result<Package, PackageError> {
        if bytes.len() > MAX_PACKAGE {
            return Err(PackageError::Unreadable("too large".into()));
        }
        Package::read(bytes)
    }

    /// Reads the files of a package (its signature is checked by [`Package::open`]).
    fn read(bytes: &[u8]) -> Result<Package, PackageError> {
        let unreadable = |e: std::io::Error| PackageError::Unreadable(e.to_string());
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(bytes));
        let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        for (n, entry) in tar.entries().map_err(unreadable)?.enumerate() {
            let mut entry = entry.map_err(unreadable)?;
            let path = entry.path().map_err(unreadable)?.to_string_lossy().trim_start_matches("./").to_string();
            let kind = entry.header().entry_type();
            if kind.is_dir() {
                continue;
            }
            let allowed =
                matches!(path.as_str(), "manifest.json" | "module.wasm" | "icon.svg") || path.strip_prefix("i18n/").is_some_and(|l| matches!(l, "fr.json" | "en.json" | "es.json"));
            if n >= MAX_ENTRIES || !kind.is_file() || !allowed || entry.size() > MAX_FILE {
                return Err(PackageError::Unexpected(path));
            }
            let mut data = Vec::new();
            entry.by_ref().take(MAX_FILE).read_to_end(&mut data).map_err(unreadable)?;
            files.insert(path, data);
        }
        let manifest = files.remove("manifest.json").ok_or_else(|| PackageError::Manifest("no manifest.json".into()))?;
        let manifest = Manifest::parse(&String::from_utf8_lossy(&manifest)).map_err(|e| PackageError::Manifest(e.join("; ")))?;
        let wasm = files.remove("module.wasm");
        if wasm.as_ref().is_some_and(|w| !w.starts_with(b"\0asm")) {
            return Err(PackageError::Manifest("module.wasm is not WebAssembly".into()));
        }
        let icon = files.remove("icon.svg");
        let i18n = files.into_iter().filter_map(|(path, data)| Some((path.strip_prefix("i18n/")?.trim_end_matches(".json").to_string(), data))).collect();
        Ok(Package { manifest, wasm, i18n, icon })
    }

    /// Writes it in `<modules>/<id>/<version>/`, through a folder that is renamed once complete.
    pub fn unpack(&self, modules_dir: &Path) -> std::io::Result<PathBuf> {
        let (id, version) = (&self.manifest.id, &self.manifest.version);
        let dir = modules_dir.join(id).join(version);
        let partial = modules_dir.join(id).join(format!("{version}.partial"));
        let _ = std::fs::remove_dir_all(&partial);
        std::fs::create_dir_all(partial.join("i18n"))?;
        std::fs::write(partial.join("manifest.json"), serde_json::to_vec_pretty(&self.manifest).map_err(std::io::Error::other)?)?;
        if let Some(w) = &self.wasm {
            std::fs::write(partial.join("module.wasm"), w)?;
        }
        if let Some(i) = &self.icon {
            std::fs::write(partial.join("icon.svg"), i)?;
        }
        for (lang, words) in &self.i18n {
            std::fs::write(partial.join("i18n").join(format!("{lang}.json")), words)?;
        }
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::rename(&partial, &dir)?;
        Ok(dir)
    }
}

/// Builds a package from its files (the publishing tool, and the tests).
pub fn build(manifest: &str, wasm: Option<&[u8]>, extra: &[(&str, &[u8])]) -> std::io::Result<Vec<u8>> {
    let mut tar = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best()));
    let mut add = |path: &str, data: &[u8]| {
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_mtime(0);
        header.set_cksum();
        tar.append_data(&mut header, path, data)
    };
    add("manifest.json", manifest.as_bytes())?;
    if let Some(w) = wasm {
        add("module.wasm", w)?;
    }
    for (path, data) in extra {
        add(path, data)?;
    }
    tar.into_inner()?.finish()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    /// A key of the tests, and its public half.
    pub(crate) fn keys() -> (SigningKey, String) {
        let key = SigningKey::from_bytes(&[7; 32]);
        let public = STANDARD.encode(key.verifying_key().to_bytes());
        (key, public)
    }

    pub(crate) fn sign(key: &SigningKey, message: &[u8]) -> String {
        STANDARD.encode(key.sign(message).to_bytes())
    }

    fn manifest() -> String {
        include_str!("../../contracts/module.example.json").to_string()
    }

    #[test]
    fn a_signed_package_is_read() {
        let (key, public) = keys();
        let bytes = build(&manifest(), Some(b"\0asm\x01\0\0\0"), &[("i18n/fr.json", b"{}"), ("icon.svg", b"<svg/>")]).unwrap();
        let sig = sign(&key, &signed_message("shelly", "1.4.0", &bytes));
        let p = Package::open(&bytes, &sig, &public, Some(("shelly", "1.4.0"))).unwrap();
        assert_eq!(p.manifest.id, "shelly");
        assert!(p.wasm.is_some() && p.icon.is_some());
        assert_eq!(p.i18n.keys().collect::<Vec<_>>(), ["fr"]);

        let dir = std::env::temp_dir().join(format!("breezone-package-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let at = p.unpack(&dir).unwrap();
        assert!(at.join("module.wasm").is_file() && at.join("i18n/fr.json").is_file());
        assert!(!dir.join("shelly").join("1.4.0.partial").exists());
    }

    #[test]
    fn what_is_not_signed_or_not_a_module_is_refused() {
        let (key, public) = keys();
        let bytes = build(&manifest(), None, &[]).unwrap();
        // The signature of a release, of another version, or by another key.
        let release = format!("breezone 1.4.0 linux-amd64\n{}\n", hex::encode(Sha256::digest(&bytes)));
        for message in [release.into_bytes(), signed_message("shelly", "1.4.1", &bytes)] {
            assert_eq!(Package::open(&bytes, &sign(&key, &message), &public, None).unwrap_err(), PackageError::Signature);
        }
        let other = SigningKey::from_bytes(&[9; 32]);
        assert_eq!(Package::open(&bytes, &sign(&other, &signed_message("shelly", "1.4.0", &bytes)), &public, None).unwrap_err(), PackageError::Signature);
        // Signed, but not what was asked.
        let sig = sign(&key, &signed_message("shelly", "1.4.0", &bytes));
        assert!(matches!(Package::open(&bytes, &sig, &public, Some(("shelly", "1.3.0"))), Err(PackageError::Other(_))));
        // A file a module may not hold, a path out of the folder.
        for path in ["run.sh", "../manifest.json", "i18n/de.json"] {
            let bytes = build(&manifest(), None, &[(path, b"x")]);
            let Ok(bytes) = bytes else { continue };
            let sig = sign(&key, &signed_message("shelly", "1.4.0", &bytes));
            assert!(matches!(Package::open(&bytes, &sig, &public, None), Err(PackageError::Unexpected(_))), "{path}");
        }
        // A component that is not WebAssembly.
        let bytes = build(&manifest(), Some(b"#!/bin/sh"), &[]).unwrap();
        let sig = sign(&key, &signed_message("shelly", "1.4.0", &bytes));
        assert!(matches!(Package::open(&bytes, &sig, &public, None), Err(PackageError::Manifest(_))));
    }
}
