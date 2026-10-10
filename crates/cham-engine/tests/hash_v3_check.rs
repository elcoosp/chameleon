//! The v3 configs must serialize to distinct JSON — that JSON is what
//! `from_config` hashes into `abstraction_hash`.
//!
//! (`cfg_only` hardcodes `hash: 0`; it is not usable for hash comparison.
//! `from_config` hashes `serde_json::to_vec(&cfg)` plus artifact bytes.)

#[test]
fn v3_configs_serialize_distinctly() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let tiny = std::fs::read_to_string(root.join("config/abstraction-tiny.toml")).unwrap();
    let v3 = std::fs::read_to_string(root.join("config/abstraction-v3-nocompress.toml")).unwrap();
    let v3c = std::fs::read_to_string(root.join("config/abstraction-v3.toml")).unwrap();

    let c1 = cham_engine::config::parse_config(&tiny).unwrap();
    let c2 = cham_engine::config::parse_config(&v3).unwrap();
    let c3 = cham_engine::config::parse_config(&v3c).unwrap();

    eprintln!(
        "tiny:      version={} slot={} compress={}",
        c1.version, c1.ladder.slot_bucket, c1.compress_history
    );
    eprintln!(
        "v3-nocomp: version={} slot={} compress={}",
        c2.version, c2.ladder.slot_bucket, c2.compress_history
    );
    eprintln!(
        "v3:        version={} slot={} compress={}",
        c3.version, c3.ladder.slot_bucket, c3.compress_history
    );

    // The hash input is `serde_json::to_vec(&cfg)`.
    let j1 = serde_json::to_vec(&c1).unwrap();
    let j2 = serde_json::to_vec(&c2).unwrap();
    let j3 = serde_json::to_vec(&c3).unwrap();

    assert_ne!(j1, j2, "tiny and v3-nocompress serialize identically");
    assert_ne!(j2, j3, "v3-nocompress and v3 serialize identically");
    assert_ne!(j1, j3, "tiny and v3 serialize identically");

    // Structurally:
    assert_eq!(c1.version, 2);
    assert_eq!(c2.version, 3);
    assert_eq!(c3.version, 3);
    // v2 tiny has slot_bucket=false, compress_history=false.
    assert!(!c1.ladder.slot_bucket);
    assert!(!c1.compress_history);
    // v3 both have slot_bucket=true (config-sourced).
    assert!(c2.ladder.slot_bucket);
    assert!(c3.ladder.slot_bucket);
    // v3 variants differ only on compress_history.
    assert!(!c2.compress_history);
    assert!(c3.compress_history);
    eprintln!("v3 configs serialize distinctly; v3 fields are config-sourced");
}
