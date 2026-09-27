//! Duplicate-deck match engine (SPECS/08 §2).
//!
//! ```text
//! profit(d) = (net_hero_seatA(d) + net_hero_seatB(d)) / 2
//! ```
//! — the SUM of the two seatings' nets (seat advantages CANCEL by adding; v1's
//! "(net1 − net2)/2" was garbled). σ over per-deal profits, clusterable by session.
//! Opponent stream per deal: `child(base_seed, "oppA{d}")` — deterministic given
//! the deal index, identical across hero configs, which is what makes A/B paired.

use serde::{Deserialize, Serialize};

use cham_core::card::{Card, Deck};
use cham_core::engine::config::EngineConfig;
use cham_core::engine::history::{HandHistory, PublicHistory};
use cham_core::engine::{Action, State, Street};
use cham_core::obs::{Agent, Observables, Player};
use cham_core::rng::{Rng, child};
use cham_opponents::OpponentSpec;
use cham_opponents::factory::OpponentSpecDto;
use cham_opponents::factory::build;
use cham_rec::Recorder;

use crate::EvalError;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MatchSpec {
    pub opponent: OpponentSpecDto,
    pub deals: u64,
    pub depth_bb: i64,
    pub base_seed: u64,
    pub label: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MatchResult {
    pub mb_per_seating: f64,
    pub se_mb: f64,
    pub seatings: u64,
    pub vr_factor: f64,
    pub per_deal_profits: Option<Vec<f64>>,
    pub wall_s: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct PoolResult {
    pub per_opponent: Vec<(String, f64, f64)>, // id, mb, se
    pub total_seatings: u64,
}

/// Play ONE seating from `state`; `hero_seat`'s actions come from `hero`, the
/// other seat's from `opp`. Returns `(hero net, all-in-EV-adjusted hero net)`
/// in chips (B4: identical when no flop/turn all-in runout applied).
///
/// DEAL LIFECYCLE (B1 wiring): every action is visible to both agents' hooks,
/// and both agents get `on_hand_end` with the public history afterwards, so a
/// single shared hero (e.g. `ChameleonAgent`, whose tracker/weights/seq are
/// per-hand state) can be reused across deals exactly as in live `play`.
fn play_seating(
    state: &mut State,
    hero_seat: usize,
    hero: &mut dyn Agent,
    opp: &mut dyn Agent,
    rng: &mut Rng,
    deal_seed: u64,
) -> Result<(i64, f64), EvalError> {
    let mut guard = 0;
    let mut log: Vec<(Street, Player, Action)> = Vec::new();
    // B4: all-in context for the EV adjustment — captured the moment both
    // stacks hit zero (board + street at all-in time; holes are read at the
    // terminal state, same cards).
    let mut allin_board: Option<Vec<Card>> = None;
    while !state.is_terminal() {
        guard += 1;
        if guard > 400 {
            return Err(EvalError::Match("hand did not terminate".into()));
        }
        let seat = state.to_act();
        let street = state.street();
        let player = Player::from_usize(seat);
        let obs = Observables::view(state, player);
        let a = if seat == hero_seat {
            hero.act(&obs, rng)
        } else {
            opp.act(&obs, rng)
        };
        // Public-history hook (I9 leak discipline, SPECS/01 §5): the match
        // driver is responsible for feeding EVERY action to both agents,
        // viewed from each agent's own seat, BEFORE state.apply. The
        // ChameleonAgent impl skips its own actions (already recorded in
        // act) and records the opponent's into its canonical ActionSeq.
        // Without this call the runtime seq only contains the hero's own
        // actions, so every infoset key diverges from the trainer's and
        // `BlueprintPolicy::strategy` returns None on ~65% of decisions.
        {
            let hero_obs = Observables::view(state, Player::from_usize(hero_seat));
            hero.on_public_action(&hero_obs, player, a);
        }
        {
            let opp_seat = 1 - hero_seat;
            let opp_obs = Observables::view(state, Player::from_usize(opp_seat));
            opp.on_public_action(&opp_obs, player, a);
        }
        log.push((street, player, a));
        let outcome = state
            .apply(a)
            .map_err(|e| EvalError::Match(format!("illegal action: {e}")))?;
        // H-11 fix (2026-09-27): the previous guard
        //   `allin_board.is_none() && state.stacks()==[0,0] && !state.is_terminal()`
        // was UNSATISFIABLE. Every path that zeroes both stacks calls
        // `all_in_runout_terminal()` inside `apply_in_place`, which sets
        // `hand_over = true`; `!state.is_terminal()` was therefore always
        // false at this point, and `allin_board` was never populated — the
        // whole VR layer was dead code. Now the engine reports the
        // pre-runout board length on `ApplyOutcome`, which is what we use
        // to reconstruct the board the agents actually saw at the all-in
        // moment (the board array is append-only, so the first N cards are
        // the pre-runout board).
        if allin_board.is_none() {
            if let Some(n) = outcome.runout_board_len {
                let n = n as usize;
                allin_board = Some(state.board()[..n].to_vec());
            }
        }
    }
    let nets = state.payoffs();
    let net_hero = nets[hero_seat];
    let net_adj = allin_adjusted_net(state, hero_seat, net_hero, allin_board.as_deref());
    // per-hand lifecycle for shared heroes (no-ops for stateless baselines)
    let n = state.board_len() as usize;
    let mut board = [Card(0); 5];
    board[..n].copy_from_slice(&state.board()[..n]);
    let hh = HandHistory {
        seed: deal_seed,
        actions: log,
        cfg: state.cfg(),
        holes: [state.hole(0), state.hole(1)],
        board,
        board_len: state.board_len(),
        result_sb: nets[0],
    };
    let ph = PublicHistory::from(&hh);
    hero.on_hand_end(&ph, net_hero);
    opp.on_hand_end(&ph, nets[1 - hero_seat]);
    Ok((net_hero, net_adj))
}

/// All-in EV adjustment, i.e. the `vr.rs` machinery WIRED (B4): on an all-in
/// showdown reached before the river, replace the realized runout by the exact
/// showdown equity vs the ACTUAL villain holding (singleton range) times the
/// all-in pot, minus the hero's investment — `allin_replacement`.
///
/// Unbiased by construction: the replacement is the conditional expectation of
/// the realized net given the all-in cards (Rao–Blackwell), applied
/// symmetrically on both seatings so the duplicate formula stays fair and
/// paired A/B streams stay paired. Scope (v3 §2.1 step 3: preflop now
/// included via the memoized completion table):
/// * flop/turn all-ins: exact singleton-range enumeration on the partial
///   board (as before);
/// * preflop all-ins: `vr::preflop_equity` — exact C(48,5) enumeration on
///   first sight per pair (~54 ms at 31.6M evals/s), memoized process-wide
///   afterwards. Previously kept the realized net (the largest remaining
///   variance source); now adjusted like every other street.
/// * river all-ins have no runout luck left (equity ∈ {0, ½, 1} reproduces the
///   realized net exactly, so adjustment is a no-op).
fn allin_adjusted_net(
    state: &State,
    hero_seat: usize,
    net_hero: i64,
    allin_board: Option<&[Card]>,
) -> f64 {
    let Some(aboard) = allin_board else {
        return net_hero as f64;
    };
    // only preflop/flop/turn all-in runouts adjust (see scope note above);
    // anything else — including a non-runout terminal — keeps realized net
    if !state.is_all_in_runout() || aboard.len() >= 5 {
        return net_hero as f64;
    }
    let hero_hand = state.hole(hero_seat);
    let vill_hand = state.hole(1 - hero_seat);
    let eq = if aboard.is_empty() {
        crate::vr::preflop_equity(hero_hand, vill_hand)
    } else {
        let mut vill_range = cham_core::eval::Range::default();
        vill_range.set(vill_hand.combo_id(), true);
        let (w, t) = cham_core::eval::equity_exact(hero_hand, &vill_range, aboard);
        w + t / 2.0
    };
    let start = state.cfg().start_stack as f64;
    crate::vr::allin_replacement(eq, 2.0 * start, start)
}

/// Deal-count of a sub-range (Range<u64> has no `.len()`).
fn range_len(range: &std::ops::Range<u64>) -> u64 {
    range.end.saturating_sub(range.start)
}

/// Duplicate-pairing variance statistics (B4 wiring).
///
/// Each entry is `(realized chips, all-in-EV-adjusted chips)` per seating.
/// The reported estimator is the duplicate mean over the ADJUSTED nets (seat
/// effects cancel by adding; runout luck is replaced by exact equity where a
/// flop/turn all-in runout applied). `vr_factor` =
/// `variance_factor(raw, adjusted)` — 1.0 when no all-in runout fired (the
/// series are identical), > 1.0 when the adjustment removed runout variance.
/// The returned SE is the reduced (adjusted) SE used in ladder/ledger CIs.
fn vr_stats(nets_a: &[(i64, f64)], nets_b: &[(i64, f64)]) -> (f64, f64, f64, Vec<f64>) {
    let raw_mb: Vec<f64> = nets_a
        .iter()
        .zip(nets_b.iter())
        .map(|(a, b)| (a.0 + b.0) as f64 / 2.0 / 100.0 * 1000.0)
        .collect();
    let adj_mb: Vec<f64> = nets_a
        .iter()
        .zip(nets_b.iter())
        .map(|(a, b)| (a.1 + b.1) / 2.0 / 100.0 * 1000.0)
        .collect();
    let m = crate::stats::mean(&adj_mb);
    let se = crate::stats::se(&adj_mb);
    let vr = crate::vr::variance_factor(&raw_mb, &adj_mb);
    (m, se, vr, adj_mb)
}

/// MatchRunner: duplicate deals + flight records.
pub struct MatchRunner;

impl MatchRunner {
    /// Run a duplicate match: deal d is played twice with seats swapped.
    #[allow(clippy::too_many_arguments)]
    pub fn run<F>(
        spec: &MatchSpec,
        hero_factory: &F,
        rec: Option<&mut Recorder>,
    ) -> Result<MatchResult, EvalError>
    where
        F: Fn() -> Box<dyn Agent>,
    {
        Self::run_range(spec, hero_factory, rec, 0..spec.deals)
    }

    /// Run a deal sub-range `[range.start, range.end)` with GLOBAL deal indices
    /// in the seed derivation (B3 chunking): concatenating chunked ranges yields
    /// per-deal seeds identical to the single-shot `run` for the same total.
    pub fn run_range<F>(
        spec: &MatchSpec,
        hero_factory: &F,
        rec: Option<&mut Recorder>,
        range: std::ops::Range<u64>,
    ) -> Result<MatchResult, EvalError>
    where
        F: Fn() -> Box<dyn Agent>,
    {
        let t0 = std::time::Instant::now();
        let engine_cfg = EngineConfig::depth(spec.depth_bb);
        let opp_spec = OpponentSpec::parse(&spec.opponent.0)
            .map_err(|e| EvalError::Match(format!("spec: {e}")))?;
        let mut opp = build(&opp_spec, cham_opponents::PercentileChart::global());
        let mut nets_a: Vec<(i64, f64)> = Vec::with_capacity(range_len(&range) as usize);
        let mut nets_b: Vec<(i64, f64)> = Vec::with_capacity(range_len(&range) as usize);
        for g in range.clone() {
            // ONE shuffle per deal — both seatings see the identical deck.
            // Seeds use the GLOBAL deal index g so chunked runs re-derive the
            // same streams a sequential run would use (never re-rolled).
            let deal_rng = &mut child(spec.base_seed, &format!("d{g}"));
            let deck = Deck::shuffled(deal_rng);
            // identical opponent streams per (deal, seating) across hero configs
            let opp_rng_a = &mut child(spec.base_seed, &format!("oppA{g}"));
            let opp_rng_b = &mut child(spec.base_seed, &format!("oppB{g}"));
            let mut hero_a = hero_factory();
            let mut hero_b = hero_factory();
            // seating A: hero at seat 0 (SB)
            let mut state_a =
                State::new(engine_cfg, deck).map_err(|e| EvalError::Match(format!("{e}")))?;
            let net_a = play_seating(
                &mut state_a,
                0,
                hero_a.as_mut(),
                opp.as_mut(),
                opp_rng_a,
                spec.base_seed ^ g,
            )?;
            // seating B: SAME deck, hero at seat 1 (BB) — seats swapped
            let mut state_b =
                State::new(engine_cfg, deck).map_err(|e| EvalError::Match(format!("{e}")))?;
            let net_b = play_seating(
                &mut state_b,
                1,
                hero_b.as_mut(),
                opp.as_mut(),
                opp_rng_b,
                spec.base_seed ^ g,
            )?;
            nets_a.push(net_a);
            nets_b.push(net_b);
        }
        Self::finish(spec, &nets_a, &nets_b, range_len(&range), rec, t0)
    }

    /// Shared-hero variant (B1): one `hero` instance plays every seating of the
    /// range (its `on_hand_end` hook resets per-hand state deal-by-deal, exactly
    /// as in live `play`). Stateless baselines behave identically to `run`.
    pub fn run_shared(
        spec: &MatchSpec,
        hero: &mut dyn Agent,
        rec: Option<&mut Recorder>,
    ) -> Result<MatchResult, EvalError> {
        Self::run_shared_range(spec, hero, rec, 0..spec.deals)
    }

    /// Shared-hero sub-range with global-index seeds (B2/B3: chunked + parallel
    /// ladder runs re-derive identical per-deal streams).
    pub fn run_shared_range(
        spec: &MatchSpec,
        hero: &mut dyn Agent,
        rec: Option<&mut Recorder>,
        range: std::ops::Range<u64>,
    ) -> Result<MatchResult, EvalError> {
        let t0 = std::time::Instant::now();
        let engine_cfg = EngineConfig::depth(spec.depth_bb);
        let opp_spec = OpponentSpec::parse(&spec.opponent.0)
            .map_err(|e| EvalError::Match(format!("spec: {e}")))?;
        let mut opp = build(&opp_spec, cham_opponents::PercentileChart::global());
        let mut nets_a: Vec<(i64, f64)> = Vec::with_capacity(range_len(&range) as usize);
        let mut nets_b: Vec<(i64, f64)> = Vec::with_capacity(range_len(&range) as usize);
        for g in range.clone() {
            let deal_rng = &mut child(spec.base_seed, &format!("d{g}"));
            let deck = Deck::shuffled(deal_rng);
            let opp_rng_a = &mut child(spec.base_seed, &format!("oppA{g}"));
            let opp_rng_b = &mut child(spec.base_seed, &format!("oppB{g}"));
            let mut state_a =
                State::new(engine_cfg, deck).map_err(|e| EvalError::Match(format!("{e}")))?;
            let net_a = play_seating(
                &mut state_a,
                0,
                hero,
                opp.as_mut(),
                opp_rng_a,
                spec.base_seed ^ g,
            )?;
            let mut state_b =
                State::new(engine_cfg, deck).map_err(|e| EvalError::Match(format!("{e}")))?;
            let net_b = play_seating(
                &mut state_b,
                1,
                hero,
                opp.as_mut(),
                opp_rng_b,
                spec.base_seed ^ g,
            )?;
            nets_a.push(net_a);
            nets_b.push(net_b);
        }
        Self::finish(spec, &nets_a, &nets_b, range_len(&range), rec, t0)
    }

    /// Assemble the `MatchResult` from per-seating nets (shared by all runners).
    fn finish(
        spec: &MatchSpec,
        nets_a: &[(i64, f64)],
        nets_b: &[(i64, f64)],
        deals: u64,
        rec: Option<&mut Recorder>,
        t0: std::time::Instant,
    ) -> Result<MatchResult, EvalError> {
        // THE formula (v2 fix): sum cancels seat advantage; VR factor wired (B4)
        let (m, se, vr, mb) = vr_stats(nets_a, nets_b);
        if let Some(r) = rec {
            use cham_rec::schema::RecordKind;
            r.record(
                RecordKind::Match,
                serde_json::json!({
                    "label": spec.label,
                    "spec_ids": [spec.opponent.0.clone()],
                    "seeds": [spec.base_seed],
                    "deals": deals,
                    "seatings": deals * 2,
                    "mb_per_seating": m,
                    "se_mb": se,
                    "vr_factor": vr,
                    "wall_s": t0.elapsed().as_secs_f64(),
                }),
            )
            .map_err(|e| EvalError::Match(format!("record: {e}")))?;
        }
        Ok(MatchResult {
            mb_per_seating: m,
            se_mb: se,
            seatings: deals * 2,
            vr_factor: vr,
            per_deal_profits: Some(mb),
            wall_s: t0.elapsed().as_secs_f64(),
        })
    }

    /// Run the hero against a pool of opponents.
    pub fn run_pool<F>(
        hero_factory: &F,
        pool: &[OpponentSpec],
        deals_per_opp: u64,
        base_seed: u64,
        depth_bb: i64,
        mut rec: Option<&mut Recorder>,
    ) -> Result<PoolResult, EvalError>
    where
        F: Fn() -> Box<dyn Agent>,
    {
        let mut out = PoolResult::default();
        for (i, opp) in pool.iter().enumerate() {
            let spec = MatchSpec {
                opponent: OpponentSpecDto(opp.id()),
                deals: deals_per_opp,
                depth_bb,
                base_seed: base_seed ^ ((i as u64) << 32),
                label: format!("pool:{}/{}", opp.id(), i),
            };
            let r = Self::run(&spec, hero_factory, rec.as_deref_mut())?;
            out.per_opponent.push((opp.id(), r.mb_per_seating, r.se_mb));
            out.total_seatings += r.seatings;
        }
        Ok(out)
    }
}
