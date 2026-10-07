//! rule-4 (2026-10-07): slot-bucket / history-compression gates read CONFIG
//! at abstraction version >= 3, legacy ENV at v2. Pins back-compat + v3 reads.

use cham_engine::config::{AbstractionConfig, parse_config};

#[test]
fn v2_defaults_are_false() {
    let t = AbstractionConfig::tiny();
    assert_eq!(t.version, 2);
    assert!(!t.ladder.slot_bucket);
    assert!(!t.compress_history);
}

#[test]
fn parse_v3_with_new_fields() {
    let toml = r#"
version = 3
spr_bands = [0.3, 1.0, 40.0]
seq_history_len = 8
compress_history = true
[buckets]
preflop = "exact169"
flop_k = 32
turn_k = 16
river_eq_bins = 16
river_texture_classes = 4
[ladder]
preflop_open_bb = [2.5]
raise_fracs = [1.0]
flop_bet_fracs = [0.5]
turn_bet_fracs = [0.5]
river_bet_fracs = [0.5]
raises_per_street_cap = 1
all_in_always = true
slot_bucket = true
"#;
    let c = parse_config(toml).expect("parse v3");
    assert_eq!(c.version, 3);
    assert!(c.ladder.slot_bucket);
    assert!(c.compress_history);
}

#[test]
fn parse_v2_without_new_fields() {
    let toml = r#"
version = 2
spr_bands = [0.3, 1.0, 40.0]
seq_history_len = 8
[buckets]
preflop = "exact169"
flop_k = 32
turn_k = 16
river_eq_bins = 16
river_texture_classes = 4
[ladder]
preflop_open_bb = [2.5]
raise_fracs = [1.0]
flop_bet_fracs = [0.5]
turn_bet_fracs = [0.5]
river_bet_fracs = [0.5]
raises_per_street_cap = 1
all_in_always = true
"#;
    let c = parse_config(toml).expect("parse v2 no new fields");
    assert!(!c.ladder.slot_bucket);
    assert!(!c.compress_history);
}
