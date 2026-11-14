//! Frozen contracts between nodes (ADR 0012): what each published version sends. The current
//! version must keep reading all of them, and keep offering everything an earlier version
//! announced, so that a node that is not updated keeps what worked.
//!
//! On a release that changes what nodes exchange, add `tests/contracts/<version>/` with what it
//! sends (`snapshot.json`, `hello.json`).

use breezone_server::capabilities::{Capabilities, PROTOCOL_MIN};
use breezone_server::federation::FedSnapshot;
use std::path::PathBuf;

fn contracts(file: &str) -> Vec<(String, String)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/contracts");
    let mut out = vec![];
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path().join(file);
        if path.is_file() {
            out.push((entry.file_name().to_string_lossy().into_owned(), std::fs::read_to_string(path).unwrap()));
        }
    }
    out.sort();
    out
}

#[test]
fn snapshots_of_every_published_version_are_still_read() {
    let all = contracts("snapshot.json");
    assert!(!all.is_empty());
    for (version, json) in all {
        let snap: FedSnapshot = serde_json::from_str(&json).unwrap_or_else(|e| panic!("snapshot of {version}: {e}"));
        let inst = &snap.installations[0];
        assert!(!inst.zones.is_empty(), "zones of {version}");
        assert_eq!(inst.meta[0].display_name.as_deref(), Some("Salon"), "{version}");
    }
}

#[test]
fn what_an_earlier_version_announced_is_still_offered() {
    let local = Capabilities::local();
    let all = contracts("hello.json");
    assert!(!all.is_empty());
    for (version, json) in all {
        let theirs: Capabilities = serde_json::from_str(&json).unwrap_or_else(|e| panic!("hello of {version}: {e}"));
        // Below the floor, a version may lose what it had (that is what raising the floor means).
        if theirs.protocol.current < PROTOCOL_MIN {
            continue;
        }
        for (name, versions) in &theirs.traits {
            for v in versions {
                assert!(local.has_trait(name, *v), "{name}@{v}, announced by {version}, is no longer served");
            }
        }
        for (name, v) in &theirs.features {
            assert!(local.has_feature(name, *v), "{name}@{v}, announced by {version}, is no longer offered");
        }
        assert_ne!(local.compatibility(&theirs).state, "outdated", "{version} must still be understood");
    }
}

#[test]
fn a_node_without_hello_gets_what_0_10_0_did() {
    // The legacy profile stands for every node up to 0.10.0: it must match what they serve.
    let legacy = Capabilities::legacy();
    assert!(legacy.has_trait("climate.zone", 1));
    for f in ["federation.watch", "federation.control", "federation.schedules"] {
        assert!(legacy.has_feature(f, 1), "{f}");
    }
    assert!(!legacy.has_feature("federation.hello", 1));
}
