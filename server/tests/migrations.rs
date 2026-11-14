//! Applied migrations must never change: SQLx stores their checksum and refuses to start if a
//! migration file differs from the one applied on an existing database (even a comment).
//! Add new migrations instead, and append their hash here.

use sha2::{Digest, Sha256};

const APPLIED: &[(&str, &str)] = &[
    ("0001_init.sql", "d5287b0f98595a94c63fbb34b5811f7260135f8ac9ecb8101df1397ed31fee29"),
    ("0002_cloud_sync.sql", "7e7d9907b4f9f41af1aba089ea3f8bf8af839235a1e03895ea64d8d5d4fc24ff"),
    ("0003_installation_access.sql", "a961b87945dae191d40f6e451d78904a8ad6a66c1e175bd85afa72fe4ad02e56"),
    ("0004_federation.sql", "03b882a1b66c158d19d4a3f9873bc613076b4f88499c2bfb1f5671e7790bdef0"),
    ("0005_routine_origin.sql", "1c19d7d213b51fd5e74cce9fa0d732db9cc532dd1bb5ad17dfd41c3a3c4c1a2d"),
    ("0006_airzone_schedules.sql", "305dbe6efbe1dc6c6ecd273dce1888d13b2562acd0ec86b28854e0c6de2ba8d2"),
    ("0007_timers.sql", "190e858a202f25ce277aae97f4a868032ff8d3a0d831508086b2dc2530951086"),
    ("0008_routines_on_demand.sql", "cbc51332cbf9b9aa39524f0f8e89c05cf4846fbbfec6514cbef243144acfdebb"),
    ("0009_signed_pairing.sql", "5be35fca82e8e11723e6e3c799ea357a123b4a7f0cf91f710136f26ff10bfb4c"),
    ("0010_federation_v2.sql", "6c5226bd7af97dc40f20e3c8fa0171ef7957e9cd55d99025d4c9c18dad6e12bd"),
    ("0011_user_access.sql", "68eba7a16d3e94990d75ee65ecebc391c900e8db9ab15d2da249c3e936627f7e"),
    ("0012_node_exclusions.sql", "b4c9b9ad3dd7e258b5bf167d34520e76e2b61b416cfbaf8d6b690059cc7cf9ac"),
    ("0013_user_rights.sql", "05b762c66ba9d38b1a095da56e1d6584d3f7638bf03e5cce7ee3c886ca78c289"),
    ("0014_languages.sql", "de771077740cd94aaf4c7629cca12883a68b468e9ebe541a7ecdce865d29effc"),
    ("0015_installation_rights.sql", "907cad13e1aa809cd27a71a5985f54659d385933bb0d563157f49f777dfda4f0"),
    ("0016_demand_and_air_quality.sql", "d3ad3a5d199a523f47e061b45230c88a3d1e6694ee7599d4c2579ee9324c741b"),
    ("0017_peer_capabilities.sql", "9b135c73021f66192d96bc83809f27a49791847a25c22d4e1b7af77484684831"),
    ("0018_rooms_and_devices.sql", "f673b07b67d64f5d68c0a5f22244642d82a51eb5304a65af4d3172d51e3b4681"),
    ("0019_sources.sql", "1e334192fed4df620ff24b6d4cbfeb790e82b295da0d935f2193082514f94503"),
    ("0020_device_roles.sql", "d15bb293b5ee824cb1de8cfac0f5957dc5bdd0f905bb15c431962424b9df704f"),
    ("0021_device_samples.sql", "5ab08c8f201231378a1cafa686c9c998fdb38a932a694439e73fa23d18c83fe2"),
    ("0022_source_secrets.sql", "6b3d19191833e0e6acd2f7d7d199bf9d38e716aa0d834e2f9e3c2fd61c311f24"),
];

#[test]
fn applied_migrations_are_unchanged() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let mut files: Vec<String> =
        std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned())).filter(|n| n.ends_with(".sql")).collect();
    files.sort();
    for (name, hash) in APPLIED {
        let content = std::fs::read(dir.join(name)).unwrap_or_else(|_| panic!("{name} was removed"));
        assert_eq!(hex::encode(Sha256::digest(&content)), *hash, "{name} was modified: add a new migration instead");
    }
    let known: Vec<&str> = APPLIED.iter().map(|(n, _)| *n).collect();
    for f in &files {
        assert!(known.contains(&f.as_str()), "new migration {f}: append its SHA-256 to APPLIED");
    }
}
