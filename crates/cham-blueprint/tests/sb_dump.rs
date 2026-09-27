//! Diagnostic: dump the trained SB-root average strategy to inspect its
//! skew. Not a gate; deleted once the SB/BB exploitability investigation
//! concludes.
use cham_blueprint::policy::BlueprintPolicy;
use cham_core::card::{Card, Deck};
use cham_core::engine::config::EngineConfig;
use cham_core::engine::{Action, State};
use cham_core::obs::{Observables, Player};
use cham_engine::config::AbstractionConfig;
use cham_engine::encoder::{ActionSeq, Encoder};

/// Diagnostic — NOT a gate. Ignored by default so CI isn't gated on a
/// pre-H-6 fixture under `artifacts/agent/robust`. Run manually after
/// rebuilding that artifact (or pass `CHAM_DUMP_BP=<fresh-artifact>`):
///
///     CHAM_DUMP_BP=artifacts/blueprints-full/robust \
///       cargo nextest run -p cham-blueprint --run-ignored dump_sb_root -- --nocapture
///
/// H-6 (2026-09-27) made `policy.bin` self-verifying: the payload hash is
/// stamped into the embedded provenance, and `load` refuses a mismatch.
/// The stale fixture under `artifacts/agent/robust` predates the fix.
#[test]
#[ignore = "diagnostic: needs a post-H-6 artifact via CHAM_DUMP_BP; see doc comment"]
fn dump_sb_root() {
    let rel = std::env::var("CHAM_DUMP_BP").unwrap_or_else(|_| "artifacts/agent/robust".into());
    // Tests run with cwd = the crate dir; resolve workspace-root-relative
    // paths via CARGO_MANIFEST_DIR (crates/cham-blueprint → workspace root).
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let bpdir = if std::path::Path::new(&rel).is_absolute() {
        std::path::PathBuf::from(&rel)
    } else {
        root.join(&rel)
    };
    let bp = BlueprintPolicy::load(&bpdir, 0).expect("load bp");
    let cfg = AbstractionConfig::tiny();
    let mut enc = Encoder::cfg_only(cfg).expect("enc");
    // One canonical HU starting state at 100bb, seeded so the deal is fixed.
    let deck = Deck::shuffled(&mut cham_core::rng::rng_from_seed(0xC0FFEE));
    let state = State::new(EngineConfig::depth(100), deck).expect("state");
    let obs = Observables::view(&state, Player::from_usize(0));
    let seq = ActionSeq::default();
    let slots = enc.slots(&obs, &seq);
    eprintln!("SB root: {} slots, holes={:?}", slots.len(), obs.hole);
    for (i, s) in slots.iter().enumerate() {
        eprintln!(
            "  slot {}: action={:?} frac={:.3} all_in={}",
            i, s.action, s.frac, s.is_all_in
        );
    }
    let probs = bp.strategy(&obs, &mut enc, &seq);
    match probs {
        Some(p) => eprintln!("SB root avg strategy: {:?}", p),
        None => eprintln!("SB root avg strategy: UNCOVERED (miss)"),
    }
    // Also test a second seed to see if it's bucket-specific.
    for seed in [0xAA, 0xBB, 0xCC, 0xDD] {
        let deck = Deck::shuffled(&mut cham_core::rng::rng_from_seed(seed));
        let state = State::new(EngineConfig::depth(100), deck).expect("state");
        let obs = Observables::view(&state, Player::from_usize(0));
        let seq = ActionSeq::default();
        let probs = bp.strategy(&obs, &mut enc, &seq);
        eprintln!("seed {:x}: {:?} -> {:?}", seed, obs.hole, probs);
    }
    let _ = Card(0);
    let _ = Action::Check;
}
