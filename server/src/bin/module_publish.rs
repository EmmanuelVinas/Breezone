//! Publishing modules (docs/roadmap/modules.md § 6), for the workflows; the signatures are made by
//! `openssl` with the key of the releases, from the messages this tool writes.
//!
//!   module-publish pack <manifest.json> [module.wasm] [i18n dir] [icon.svg] <out dir>
//!       → <out>/<id>-<version>.bzm and <id>-<version>.bzm.message (to sign into `.bzm.sig`)
//!   module-publish index <index.json or -> <out dir> <package.bzm>…
//!       → <out>/index.json (seq + 1, the versions added newest first) and index.json.message
//!   module-publish host
//!       → what this build hosts for modules (`host.json` of a release)

use breezone_server::modules::library::{Entry, Index, Published};
use breezone_server::modules::manifest::Manifest;
use breezone_server::modules::package;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("pack") => pack(&args[1..]),
        Some("index") => index(&args[1..]),
        Some("host") => serde_json::to_string_pretty(&breezone_server::capabilities::Host::local()).map(|h| println!("{h}")).map_err(|e| e.to_string()),
        _ => Err("usage: module-publish pack|index|host …".into()),
    };
    if let Err(e) = result {
        eprintln!("module-publish: {e}");
        std::process::exit(1);
    }
}

fn read(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn pack(args: &[String]) -> Result<(), String> {
    let (out, inputs) = args.split_last().ok_or("pack: no output folder")?;
    let manifest_path = inputs.first().ok_or("pack: no manifest")?;
    let text = String::from_utf8(read(Path::new(manifest_path))?).map_err(|e| e.to_string())?;
    let manifest = Manifest::parse(&text).map_err(|e| e.join("; "))?;
    let mut wasm = None;
    let mut extra: Vec<(String, Vec<u8>)> = vec![];
    for input in &inputs[1..] {
        let p = PathBuf::from(input);
        if p.is_dir() {
            for lang in ["fr", "en", "es"] {
                let f = p.join(format!("{lang}.json"));
                if f.is_file() {
                    extra.push((format!("i18n/{lang}.json"), read(&f)?));
                }
            }
        } else if input.ends_with(".wasm") {
            wasm = Some(read(&p)?);
        } else if input.ends_with(".svg") {
            extra.push(("icon.svg".into(), read(&p)?));
        } else {
            return Err(format!("pack: {input}: neither a component, a folder of words nor an icon"));
        }
    }
    let refs: Vec<(&str, &[u8])> = extra.iter().map(|(p, d)| (p.as_str(), d.as_slice())).collect();
    let bytes = package::build(&serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?, wasm.as_deref(), &refs).map_err(|e| e.to_string())?;
    let name = format!("{}-{}.bzm", manifest.id, manifest.version);
    let out = Path::new(out);
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    std::fs::write(out.join(&name), &bytes).map_err(|e| e.to_string())?;
    std::fs::write(out.join(format!("{name}.message")), package::signed_message(&manifest.id, &manifest.version, &bytes)).map_err(|e| e.to_string())?;
    println!("{name}");
    Ok(())
}

/// What a package holds, read without its signature (it is being published, not installed).
fn manifest_of(bytes: &[u8]) -> Result<(Manifest, bool), String> {
    use std::io::Read;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(bytes));
    let (mut manifest, mut code) = (None, false);
    for entry in tar.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path().map_err(|e| e.to_string())?.to_string_lossy().into_owned();
        if path == "manifest.json" {
            let mut text = String::new();
            entry.read_to_string(&mut text).map_err(|e| e.to_string())?;
            manifest = Some(Manifest::parse(&text).map_err(|e| e.join("; "))?);
        }
        code |= path == "module.wasm";
    }
    Ok((manifest.ok_or("no manifest.json in the package")?, code))
}

fn index(args: &[String]) -> Result<(), String> {
    let [current, out, packages @ ..] = args else { return Err("index: <index.json or -> <out dir> <package>…".into()) };
    let mut index: Index = if current == "-" { Index { seq: 0, modules: vec![] } } else { serde_json::from_slice(&read(Path::new(current))?).map_err(|e| e.to_string())? };
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    for p in packages {
        let bytes = read(Path::new(p))?;
        let (manifest, code) = manifest_of(&bytes)?;
        let file = Path::new(p).file_name().and_then(|f| f.to_str()).ok_or("a package without a name")?.to_string();
        let published = Published { file, size: bytes.len() as u64, sha256: hex::encode(Sha256::digest(&bytes)), published_at: now, yanked: false, code, manifest };
        let pos = index.modules.iter().position(|e| e.id == published.manifest.id).unwrap_or_else(|| {
            index.modules.push(Entry { id: published.manifest.id.clone(), versions: vec![] });
            index.modules.len() - 1
        });
        let entry = &mut index.modules[pos];
        if entry.versions.iter().any(|v| v.manifest.version == published.manifest.version) {
            return Err(format!("{} {} is already published", published.manifest.id, published.manifest.version));
        }
        entry.versions.push(published);
        entry.versions.sort_by(|a, b| semver::Version::parse(&b.manifest.version).ok().cmp(&semver::Version::parse(&a.manifest.version).ok()));
    }
    index.seq += 1;
    index.modules.sort_by(|a, b| a.id.cmp(&b.id));
    let bytes = serde_json::to_vec_pretty(&index).map_err(|e| e.to_string())?;
    let out = Path::new(out);
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    std::fs::write(out.join("index.json"), &bytes).map_err(|e| e.to_string())?;
    std::fs::write(out.join("index.json.message"), breezone_server::modules::library::signed_message(index.seq, &bytes)).map_err(|e| e.to_string())?;
    println!("index {}", index.seq);
    Ok(())
}
