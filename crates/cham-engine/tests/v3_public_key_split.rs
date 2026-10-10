//! `key_for_public` + `key_from_public` reconstruct `key_for` at v3, and
//! are identity-like at v2 (no split).

use cham_core::card::{Card, Deck};
use cham_core::engine::State;
use cham_core::engine::config::EngineConfig;
use cham_core::obs::{Observables, Player};
use cham_engine::config::AbstractionConfig;
use cham_engine::encoder::{ActionSeq, Encoder};
use cham_engine::ladder::ActionLadder;

const CFG: EngineConfig = EngineConfig {
    start_stack: 10_000,
    sb: 50,
    bb: 100,
};

fn v3_cfg() -> AbstractionConfig {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let txt = std::fs::read_to_string(root.join("config/abstraction-v3-nocompress.toml")).unwrap();
    cham_engine::config::parse_config(&txt).unwrap()
}

fn river_state() -> State {
    let prefix = [
        Card(2),
        Card(3),
        Card(4),
        Card(5),
        Card(40),
        Card(41),
        Card(42),
        Card(43),
        Card(44),
    ];
    State::new(CFG, Deck::with_prefix(&prefix)).expect("state")
}

#[test]
fn v3_public_split_reconstructs_full_key() {
    let cfg = v3_cfg();
    assert_eq!(cfg.version, 3, "test config must be v3");
    let ladder = ActionLadder::new(&cfg);
    let mut enc = Encoder::cfg_only(cfg).unwrap();
    let st = river_state();
    let obs = Observables::view(&st, Player::from_usize(1));
    let seq = ActionSeq::default();
    let slots = ladder.slots(&obs, &seq);

    let full = enc.key_for(&obs, &seq, &slots).0;
    let public = enc.key_for_public(&obs, &seq, &slots);
    let bucket = enc.bucket(&obs);
    let rebuilt = enc.key_from_public(public, bucket).0;

    assert_eq!(
        full, rebuilt,
        "v3: key_from_public(key_for_public(..), bucket) != key_for(..)"
    );

    // A different bucket must give a different key (proves the bucket
    // actually enters the reconstruction at v3).
    let other = enc.key_from_public(public, bucket.wrapping_add(1)).0;
    assert_ne!(full, other, "v3: different bucket gave the same key");

    // And the reconstruction is exactly `public ^ bucket_mix(bucket) |
    // (1<<63)`.
    let expected = (public ^ Encoder::bucket_mix(bucket)) | (1 << 63);
    assert_eq!(full, expected, "v3 reconstruction formula mismatch");
}

#[test]
fn v2_public_split_is_identity() {
    let cfg = AbstractionConfig::tiny();
    assert_eq!(cfg.version, 2, "tiny must be v2");
    let ladder = ActionLadder::new(&cfg);
    let mut enc = Encoder::cfg_only(cfg).unwrap();
    let st = river_state();
    let obs = Observables::view(&st, Player::from_usize(1));
    let seq = ActionSeq::default();
    let slots = ladder.slots(&obs, &seq);

    let full = enc.key_for(&obs, &seq, &slots).0;
    let public = enc.key_for_public(&obs, &seq, &slots);
    // At v2 there is no split: key_for_public IS the full key.
    assert_eq!(full, public, "v2: key_for_public != key_for");

    // key_from_public ignores the bucket at v2.
    let b = enc.bucket(&obs);
    assert_eq!(enc.key_from_public(public, b).0, full);
    assert_eq!(enc.key_from_public(public, b.wrapping_add(1)).0, full);
}
