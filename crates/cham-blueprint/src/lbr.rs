//! Local best response (SPECS/04 §8): the honest exploitability number.
//!
//! Tabular BR against a FIXED policy on our abstraction: chance deals are sampled
//! (seeded, Iterations-mode determinism), hero actions ENUMERATED, the fixed
//! policy's actions are AVERAGED over (expectation, no sampling). The result is an
//! upper-bound estimate of what a perfect exploiter earns vs that policy.

use serde::Serialize;

use cham_core::card::Deck;
use cham_core::engine::State;
use cham_core::engine::config::EngineConfig;
use cham_core::obs::{Observables, Player};
use cham_core::rng::{Rng, child};
use cham_engine::encoder::ActionSeq;

use crate::BlueprintError;

#[derive(Clone, Debug, Serialize)]
pub struct LbrReport {
    /// BR value for the BR seat, in mb/hand (milli-bb per hand)
    pub lbr_mb_per_hand: f64,
    /// BR value in bb/hand
    pub lbr_bb_per_hand: f64,
    pub deals_sampled: u32,
    pub depth_bb: i64,
    /// Standard error of the held-out mean (bb/hand). §3.3: the estimator
    /// must be able to say "I don't know" — policy deltas below ~2 SE
    /// (≈1.5 bb on the two-seat sum) are noise, not signal.
    pub se_bb: f64,
}

/// Minimum infoset visits before the learned choice may move off the
/// passive default (§3.3). Thin infosets carry noise, not signal — the
/// old code took the max over raw sums, which is how converged BRs went
/// negative.
pub const MIN_BR_VISITS: u32 = 30;

/// Exact-ish BR vs `policy` (a mapping from observables to (action, prob) pairs)
/// computed over `deals` seeded chance deals. `br_seat` = 0 (SB) or 1 (BB).
pub fn lbr_vs<F>(
    policy: &mut F,
    br_seat: usize,
    engine_cfg: EngineConfig,
    enc: &mut cham_engine::Encoder,
    deals: u32,
    seed: u64,
) -> Result<LbrReport, BlueprintError>
where
    // the policy sees the observables AND the canonical action seq (infoset key
    // inputs); it returns its (action, prob) distribution at that decision
    F: FnMut(&Observables<'_>, &ActionSeq) -> Vec<(cham_core::engine::Action, f64)>,
{
    let mut total = 0f64;
    for d in 0..deals {
        let rng = &mut child(seed, &format!("deal{d}"));
        let mut state = State::new(engine_cfg, Deck::shuffled(rng))
            .map_err(|e| BlueprintError::Training(format!("engine: {e}")))?;
        let mut seq = ActionSeq::default();
        total += br_walk(&mut state, br_seat, policy, enc, &mut seq, rng);
    }
    let bb = total / deals.max(1) as f64;
    // Per-deal SE is not tracked in the clairvoyant path (single pass);
    // report 0 and prefer `tabular_br` (which tracks it) for comparisons.
    Ok(LbrReport {
        lbr_bb_per_hand: bb,
        lbr_mb_per_hand: bb * 1000.0,
        deals_sampled: deals,
        depth_bb: engine_cfg.depth_bb(),
        se_bb: 0.0,
    })
}

/// BR recursion: hero nodes enumerate + max; opponent nodes average over their
/// fixed probabilities; chance is the deck.
#[allow(clippy::only_used_in_recursion)]
fn br_walk<F>(
    state: &mut State,
    br_seat: usize,
    policy: &mut F,
    enc: &mut cham_engine::Encoder,
    seq: &mut ActionSeq,
    rng: &mut Rng,
) -> f64
where
    F: FnMut(&Observables<'_>, &ActionSeq) -> Vec<(cham_core::engine::Action, f64)>,
{
    if state.is_terminal() {
        return state.payoffs()[br_seat] as f64 / 100.0;
    }
    let p = state.to_act();
    let obs = Observables::view(state, Player::from_usize(p));
    if p != br_seat {
        // fixed policy: expectation over its distribution
        let dist = policy(&obs, seq);
        let mut ev = 0f64;
        let mut total_p = 0f64;
        for (a, prob) in &dist {
            if *prob <= 1e-12 {
                continue;
            }
            let mut s2 = *state;
            let mut seq2 = *seq;
            enc.record(&obs, Player::from_usize(p), *a, &mut seq2);
            if s2.apply(*a).is_err() {
                continue; // policy proposes something illegal here: skip (mass renorm below)
            }
            total_p += prob;
            ev += prob * br_walk(&mut s2, br_seat, policy, enc, &mut seq2, rng);
        }
        if total_p <= 1e-12 {
            // policy mass all illegal: treat as check/fold per legality
            let fallback = if obs.legal.is_empty() {
                cham_core::engine::Action::Check
            } else {
                obs.legal[0].action
            };
            enc.record(&obs, Player::from_usize(p), fallback, seq);
            state.apply(fallback).expect("legal fallback");
            return br_walk(state, br_seat, policy, enc, seq, rng);
        }
        return ev / total_p;
    }
    // BR seat: enumerate the abstraction slots, take the max
    let slots = enc.slots(&obs, seq);
    let mut best = f64::NEG_INFINITY;
    for s in slots.iter() {
        let mut s2 = *state;
        let mut seq2 = *seq;
        enc.record(&obs, Player::from_usize(p), s.action, &mut seq2);
        if s2.apply(s.action).is_err() {
            continue;
        }
        let v = br_walk(&mut s2, br_seat, policy, enc, &mut seq2, rng);
        if v > best {
            best = v;
        }
    }
    if best.is_infinite() {
        // no slot applied (shouldn't happen): pass through with first legal
        let fallback = obs
            .legal
            .first()
            .map(|l| l.action)
            .unwrap_or(cham_core::engine::Action::Check);
        enc.record(&obs, Player::from_usize(p), fallback, seq);
        state.apply(fallback).expect("legal fallback");
        return br_walk(state, br_seat, policy, enc, seq, rng);
    }
    best
}

// ============================================================================
// F1 (2026-10-01, chameleon-competitiveness-report): infoset-consistent
// tabular best response.
//
// The `lbr_vs` above is CLAIRVOYANT: at a BR-seat node it enumerates the
// actions *inside each sampled deal* and takes the max — i.e. the BR player
// sees the opponent's hole cards. A real best response must pick ONE action
// per information set (the BR player does not know the opponent's cards).
// The clairvoyant version overestimates exploitability substantially; on a
// near-Nash Kuhn strategy the report measured it 115x the true value.
//
// The fix: learn a per-infoset action choice by iterated best-response sweeps
// (Lisý & Bowling 2016 / Burch et al. 2014 style), then evaluate it on
// held-out deals. This is a legitimate lower bound on exploitability,
// evaluated in the real engine.
// ============================================================================

use std::collections::HashMap;

/// Counterfactual-value accumulator per BR infoset: (ladder width,
/// reach-weighted value sums per slot, visit count). The visit count
/// gates the `MIN_BR_VISITS` noise guard (§3.3).
type Cfv = HashMap<u64, (usize, [f64; 12], u32)>; // infoset key -> (width, sum reach_opp * v[a], visits)
type Choice = HashMap<u64, usize>; // infoset key -> chosen slot index

/// Pick a passive slot (Check / Call) as the neutral initial choice; fall
/// back to slot 0 if none.
fn passive_slot(slots: &[cham_engine::ladder::AbstractAction]) -> usize {
    slots
        .iter()
        .position(|s| {
            matches!(
                s.action,
                cham_core::engine::Action::Check | cham_core::engine::Action::Call
            )
        })
        .unwrap_or(0)
}

/// Inner walk with an optional fine-information key encoder.
#[allow(clippy::too_many_arguments)]
fn tab_walk_keyed<F>(
    st: &mut State,
    br_seat: usize,
    policy: &mut F,
    enc: &mut cham_engine::Encoder,
    mut key_enc: Option<&mut cham_engine::Encoder>,
    seq: &mut ActionSeq,
    choice: &Choice,
    reach_opp: f64,
    learn: bool,
    cfv: &mut Cfv,
) -> f64
where
    F: FnMut(&Observables<'_>, &ActionSeq) -> Vec<(cham_core::engine::Action, f64)>,
{
    if st.is_terminal() {
        return st.payoffs()[br_seat] as f64 / 100.0;
    }
    let p = st.to_act();
    let obs = Observables::view(st, Player::from_usize(p));

    if p != br_seat {
        // Fixed opponent policy: expectation over its distribution.
        let dist = policy(&obs, seq);
        let tot: f64 = dist.iter().map(|(_, q)| *q).sum::<f64>().max(1e-12);
        let mut ev = 0.0;
        for (a, q) in &dist {
            let q = q / tot;
            if q <= 1e-12 {
                continue;
            }
            let mut s2 = *st;
            let mut seq2 = *seq;
            enc.record(&obs, Player::from_usize(p), *a, &mut seq2);
            if s2.apply(*a).is_err() {
                continue;
            }
            ev += q * tab_walk_keyed(
                &mut s2,
                br_seat,
                policy,
                enc,
                key_enc.as_deref_mut(),
                &mut seq2,
                choice,
                reach_opp * q,
                learn,
                cfv,
            );
        }
        return ev;
    }

    // BR seat: use (or learn) one action per infoset.
    let slots = enc.slots(&obs, seq);
    let key = match key_enc.as_mut() {
        Some(ke) => ke.key_for(&obs, seq, &slots).0,
        None => enc.key_for(&obs, seq, &slots).0,
    };
    let w = slots.len();
    let pick = *choice.get(&key).unwrap_or(&passive_slot(&slots));

    // In learn mode, compute all action values; in eval mode only `pick`.
    let mut v = [0.0f64; 12];
    for (i, s) in slots.iter().enumerate() {
        if !learn && i != pick {
            continue;
        }
        let mut s2 = *st;
        let mut seq2 = *seq;
        enc.record(&obs, Player::from_usize(p), s.action, &mut seq2);
        if s2.apply(s.action).is_err() {
            continue;
        }
        let val = tab_walk_keyed(
            &mut s2,
            br_seat,
            policy,
            enc,
            key_enc.as_deref_mut(),
            &mut seq2,
            choice,
            reach_opp,
            learn,
            cfv,
        );
        v[i] = val;
    }

    if learn {
        let entry = cfv.entry(key).or_insert((w, [0.0; 12], 0));
        // Guard: if two infoset keys map to different widths this is a bug
        // (invariant I8); use the recorded width.
        let w_use = entry.0;
        for i in 0..w.min(w_use) {
            entry.1[i] += reach_opp * v[i];
        }
        entry.2 += 1;
    }
    v[pick]
}

/// Infoset-consistent tabular BR for one seat.
///
/// `sweeps` full passes over `train_deals` seeded deals update the per-infoset
/// action choice (each sweep recomputes the counterfactual values under the
/// current choices). The final choices are then evaluated on `test_deals`
/// held-out deals. Returns an `LbrReport` (bb/hand, mb/hand, SE).
///
/// §3.3 noise guards: an infoset keeps the passive default unless visited
/// at least `MIN_BR_VISITS` times, and the report carries the held-out SE.
#[allow(clippy::too_many_arguments)]
pub fn tabular_br<F>(
    policy: &mut F,
    br_seat: usize,
    engine_cfg: EngineConfig,
    enc: &mut cham_engine::Encoder,
    train_deals: u32,
    test_deals: u32,
    sweeps: u32,
    seed: u64,
) -> Result<LbrReport, BlueprintError>
where
    F: FnMut(&Observables<'_>, &ActionSeq) -> Vec<(cham_core::engine::Action, f64)>,
{
    tabular_br_keyed(
        policy,
        br_seat,
        engine_cfg,
        enc,
        None,
        train_deals,
        test_deals,
        sweeps,
        seed,
    )
}

/// Fine-information tabular BR (§3.3): the POLICY still sees `enc`, but
/// the BR keys its infosets with the finer `key_enc` (same ladder /
/// spr_bands / seq_history_len, finer full-coverage buckets). Report the
/// ratio same-abstraction : fine-information; ≫1 means the headline
/// number is an abstraction artifact.
#[allow(clippy::too_many_arguments)]
pub fn tabular_br_fine<F>(
    policy: &mut F,
    br_seat: usize,
    engine_cfg: EngineConfig,
    enc: &mut cham_engine::Encoder,
    key_enc: &mut cham_engine::Encoder,
    train_deals: u32,
    test_deals: u32,
    sweeps: u32,
    seed: u64,
) -> Result<LbrReport, BlueprintError>
where
    F: FnMut(&Observables<'_>, &ActionSeq) -> Vec<(cham_core::engine::Action, f64)>,
{
    tabular_br_keyed(
        policy,
        br_seat,
        engine_cfg,
        enc,
        Some(key_enc),
        train_deals,
        test_deals,
        sweeps,
        seed,
    )
}

#[allow(clippy::too_many_arguments)]
fn tabular_br_keyed<F>(
    policy: &mut F,
    br_seat: usize,
    engine_cfg: EngineConfig,
    enc: &mut cham_engine::Encoder,
    mut key_enc: Option<&mut cham_engine::Encoder>,
    train_deals: u32,
    test_deals: u32,
    sweeps: u32,
    seed: u64,
) -> Result<LbrReport, BlueprintError>
where
    F: FnMut(&Observables<'_>, &ActionSeq) -> Vec<(cham_core::engine::Action, f64)>,
{
    let mut choice: Choice = HashMap::new();

    for sweep in 0..sweeps.max(1) {
        let mut cfv: Cfv = HashMap::new();
        for d in 0..train_deals {
            let rng = &mut child(seed, &format!("tabtrain{sweep}-{d}"));
            let mut st = State::new(engine_cfg, Deck::shuffled(rng))
                .map_err(|e| BlueprintError::Training(format!("engine: {e}")))?;
            let mut seq = ActionSeq::default();
            let _ = tab_walk_keyed(
                &mut st,
                br_seat,
                policy,
                enc,
                key_enc.as_deref_mut(),
                &mut seq,
                &choice,
                1.0,
                true,
                &mut cfv,
            );
        }
        let mut changed = 0u32;
        for (k, (w, v, n)) in &cfv {
            if *n < MIN_BR_VISITS {
                continue; // thin infoset: keep the passive default
            }
            let mut best = 0usize;
            let mut best_v = f64::NEG_INFINITY;
            for i in 0..*w {
                if v[i] > best_v {
                    best_v = v[i];
                    best = i;
                }
            }
            if choice.insert(*k, best) != Some(best) {
                changed += 1;
            }
        }
        if changed == 0 && sweep > 0 {
            break;
        }
    }

    // Held-out evaluation with the frozen choice (per-deal values → SE).
    let mut vals: Vec<f64> = Vec::with_capacity(test_deals as usize);
    for d in 0..test_deals {
        let rng = &mut child(seed, &format!("tabtest{d}"));
        let mut st = State::new(engine_cfg, Deck::shuffled(rng))
            .map_err(|e| BlueprintError::Training(format!("engine: {e}")))?;
        let mut seq = ActionSeq::default();
        let mut dummy = Cfv::new();
        vals.push(tab_walk_keyed(
            &mut st,
            br_seat,
            policy,
            enc,
            key_enc.as_deref_mut(),
            &mut seq,
            &choice,
            1.0,
            false,
            &mut dummy,
        ));
    }
    let n = vals.len().max(1) as f64;
    let bb = vals.iter().sum::<f64>() / n;
    let se = if vals.len() > 1 {
        let var = vals.iter().map(|v| (v - bb).powi(2)).sum::<f64>() / (n - 1.0);
        (var / n).sqrt()
    } else {
        0.0
    };
    Ok(LbrReport {
        lbr_bb_per_hand: bb,
        lbr_mb_per_hand: bb * 1000.0,
        deals_sampled: test_deals,
        depth_bb: engine_cfg.depth_bb(),
        se_bb: se,
    })
}
