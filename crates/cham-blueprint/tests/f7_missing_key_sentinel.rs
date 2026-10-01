//! F7 (chameleon-competitiveness-report.md) regression: in parallel
//! (allow_insert=false) mode, a missing table key must NOT contribute a
//! value of 0.0 to the ancestor's regret update.
//!
//! The fix (commit 1a8cc4a) is a `f64::NAN` sentinel: the traversal
//! returns NaN, the hero node detects NaN in any action value, and the
//! whole subtree's regret update is skipped. This test pins that
//! contract: a fresh (empty) table + `allow_insert=false` must produce
//! NaN, not 0.0.

use cham_blueprint::table::{RegretTable, ThreadMode};
use cham_blueprint::traversal::{RbpConfig, TableRef, Traversal};
use cham_core::card::Deck;
use cham_core::engine::State;
use cham_core::engine::config::EngineConfig;
use cham_core::obs::Agent;
use cham_engine::config::AbstractionConfig;
use cham_engine::encoder::{ActionSeq, Encoder};

struct Dummy;
impl Agent for Dummy {
    fn name(&self) -> &str {
        "dummy"
    }
    fn act(
        &mut self,
        _o: &cham_core::obs::Observables<'_>,
        _r: &mut cham_core::rng::Rng,
    ) -> cham_core::engine::Action {
        cham_core::engine::Action::Check
    }
}

#[test]
fn missing_key_in_parallel_mode_returns_nan_not_zero() {
    let mut table = RegretTable::new(ThreadMode::Hogwild);
    let mut enc = Encoder::cfg_only(AbstractionConfig::tiny()).expect("enc");
    let mut dummy = Dummy;
    let mut state = State::new(
        EngineConfig::depth(100),
        Deck::shuffled(&mut cham_core::rng::rng_from_seed(0xFEED)),
    )
    .expect("state");
    let mut seq = ActionSeq::default();
    let mut rng = cham_core::rng::rng_from_seed(0xDEAD);

    let mut walker = Traversal {
        table: TableRef::Shared(&table),
        opp: &mut dummy,
        rbp: RbpConfig::default(),
        iteration: 0,
        mode: cham_blueprint::modes::TrainModeTag::Robust,
        hero_nodes: 0,
        pruned_nodes: 0,
        regret_discount: 1.0,
        allow_insert: false,
        warmup_only: false,
        explore_eps: 0.0,
    };
    let v = walker.walk(&mut state, 0, 1.0, &mut seq, &mut enc, &mut rng);
    assert!(
        v.is_nan(),
        "missing key in parallel mode must return NaN (not 0.0); got {v}"
    );
    // Silence unused warning
    let _ = &mut table;
}
