//! The trainer driver (SPECS/04 §4, §6): iterations, seat randomization, delayed
//! linear averaging, per-iteration opponent jitter redraws, snapshots + resume,
//! flight records.

use std::path::Path;

use serde::{Deserialize, Serialize};

use cham_core::card::Deck;
use cham_core::engine::State;
use cham_core::engine::config::EngineConfig;
use cham_core::obs::Agent;
use cham_core::rng::{Rng, child};
use cham_engine::encoder::ActionSeq;
use cham_rec::Recorder;
use cham_rec::schema::RecordKind;

use crate::BlueprintError;
use crate::modes::{TrainMode, TrainModeTag};
use crate::table::{RegretTable, ThreadMode, set_train_explore_eps};
#[allow(unused_imports)]
use crate::traversal::sample_index;
use crate::traversal::{RbpConfig, SnapBatchSink, Traversal};

/// Default worker count per thread mode (PERF-PLAN T5): `Deterministic` is
/// single-threaded by contract; `Hogwild` keeps the historical 8; `Snapbatch`
/// uses `available_parallelism` — on Apple M1 (4 P-cores + 4 E-cores) 4
/// workers usually beats 8 for this memory-bound workload.
pub fn default_threads(mode: ThreadMode) -> u32 {
    match mode {
        ThreadMode::Deterministic => 1,
        ThreadMode::Hogwild => 8,
        ThreadMode::Snapbatch => std::thread::available_parallelism()
            .map(|n| n.get() as u32)
            .unwrap_or(4),
    }
}

/// Trainer configuration.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrainerConfig {
    pub depth_bb: i64,
    pub iters: u64,
    pub train_seed: u64,
    pub snapshot_every: u64,
    pub bayes_session_block: u64,
    /// DCFR positive-regret discount α (Brown & Sandholm 2019).
    /// 1.0 = classic CFR+ (no discount). < 1.0 discounts accumulated
    /// positive regret before adding the new delta. Serialized with a
    /// default for backward compat with existing snapshots.
    #[serde(default = "default_regret_discount")]
    pub regret_discount: f32,
    /// DCFR strategy-sum weight discount γ (v3 §3.1: the α/γ split).
    /// Robust mode multiplies the delayed-linear averaging weight by
    /// `γ^(T−t)`; previously hard-coded to 0.9 inside `averaging_weight`,
    /// now independently tunable so EXP-011 can sweep {α, γ} on a grid.
    /// Default 0.9 reproduces the historical behavior exactly; serialized
    /// with a default so old snapshots/configs keep parsing.
    #[serde(default = "default_avg_gamma")]
    pub avg_gamma: f32,
    /// LBR convergence checkpoints (v7 Item 2): every N iters, write a
    /// standalone snapshot to `<checkpoint_dir>/iter-<t>/table.snap`.
    /// 0 = disabled (default; preserves historical behavior exactly).
    #[serde(default)]
    pub checkpoint_every: u64,
    #[serde(default)]
    pub checkpoint_dir: Option<std::path::PathBuf>,
}

pub fn default_regret_discount() -> f32 {
    1.0
}

/// Default strategy-averaging γ for robust mode.
///
/// Was 0.9 from the v3 §3.1 γ-split. Measured 2026-09-28 (tiny abstraction,
/// 500k iters, seed 7): γ = 0.9 gives LBR 39 692 / 15 068 (seat 0 / seat 1);
/// γ = 1.0 (pure Linear CFR+, no decay) gives 25 652 / 15 184; γ = 0.9999
/// gives 24 246 / 14 466. The 0.9 default is **55 % worse on seat 0** than
/// either alternative.
///
/// Why 0.9 is pathological: the weight applied at iteration `t` is
/// `(t − T/4) · γ^(T−t)`. `γ^(T−t)` underflows to exactly 0 in f64 once
/// `T − t > ~700` (0.9^700 ≈ 10⁻³²), so the effective average window
/// collapses to the last ~700 iterations of a still-oscillating CFR+
/// current iterate. 500k and 5M iters then look identical — which is
/// exactly what the 50k / 500k / 5M sweep showed.
///
/// 1.0 is the principled choice: it is pure Linear CFR+ averaging as in
/// Brown & Sandholm 2019 (their strategy sum weight is `t − D`, no
/// exponential decay). It is also within noise of the best measured value.
///
/// Operators who want recency can pass `--avg-gamma 0.9999` explicitly;
/// any γ that underflows before `T − D` is now a footgun we no longer
/// ship as the default.
pub fn default_avg_gamma() -> f32 {
    1.0
}

impl TrainerConfig {
    pub fn validate(&self) -> Result<(), BlueprintError> {
        if self.iters == 0 {
            return Err(BlueprintError::Training("iters must be > 0".into()));
        }
        if self.snapshot_every == 0 {
            return Err(BlueprintError::Training(
                "snapshot_every must be > 0".into(),
            ));
        }
        if self.bayes_session_block == 0 {
            return Err(BlueprintError::Training(
                "bayes_session_block must be > 0".into(),
            ));
        }
        Ok(())
    }
}

/// Provenance for one training run (SPECS/04 §6).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunProvenance {
    pub mode: TrainModeTag,
    pub opponent_id: Option<String>,
    pub depth_bb: i64,
    pub iters: u64,
    pub train_seed: u64,
    pub thread_mode: ThreadMode,
    pub threads: u32,
    pub abstraction_hash: u64,
    pub infosets: usize,
    pub wall_s: f64,
    pub parent: Option<String>,
    /// hero-seat draws during training (seat_randomized test / histograms)
    pub seat_histogram: [u64; 2],
    /// Exploration floor used by this run (2026-09-29). 0.0 = no floor,
    /// bit-identical to pre-2026-09-29 behavior. Recorded so a resumed
    /// run can detect a mismatch and so provenance is auditable: a run
    /// with eps > 0 produced a DIFFERENT artifact than one with eps = 0
    /// from the same seed, and the ledger needs to see that.
    #[serde(default)]
    pub train_explore_eps: f64,
}

/// Delayed linear averaging weight (SPECS/04 §4): `w_t = max(0, t − D)`, `D = iters/4`;
/// Robust mode multiplies by `γ^{T−t}` (Linear CFR). The 3-arg form keeps the
/// historical γ = 0.9; [`averaging_weight_gamma`] exposes γ for the v3 §3.1
/// EXP-011 {α, γ} sweep.
pub fn averaging_weight(t: u64, total: u64, robust: bool) -> f64 {
    averaging_weight_gamma(t, total, robust, 0.9)
}

/// [`averaging_weight`] with an explicit strategy-sum discount γ (v3 §3.1).
/// `gamma = 1.0` is pure delayed-linear averaging (no recency tilt).
pub fn averaging_weight_gamma(t: u64, total: u64, robust: bool, gamma: f32) -> f64 {
    // Diagnostic (session 2026-09-27): CHAM_AVG_UNIFORM=1 makes the average
    // window uniform (w=1 for all t). Unset → historical behavior.
    if std::env::var("CHAM_AVG_UNIFORM").as_deref() == Ok("1") {
        return 1.0;
    }
    // CHAM_AVG_DELAY overrides the delay fraction of the strategy-average
    // window. Default is 1/4 (historical). 0 disables the delay entirely:
    // w_t = t, the standard Linear CFR+ weight from Brown & Sandholm 2019.
    // This is the next ablation after the γ=1.0 fix (which was worth 40%
    // on seat 0; see docs/plans/AVG-GAMMA-FINDING-2026-09-28.md).
    let d = if let Ok(s) = std::env::var("CHAM_AVG_DELAY") {
        if let Ok(n) = s.parse::<u64>() {
            n
        } else {
            total / 4
        }
    } else {
        total / 4
    };
    let base = if t > d { (t - d) as f64 } else { 0.0 };
    if robust {
        // Underflow tripwire (2026-09-28): `γ^(T−t)` underflows to 0 in f64
        // for `T − t ≳ 700` when γ = 0.9, collapsing the effective averaging
        // window to the last few hundred iterations of a still-oscillating
        // CFR+ iterate. Warn once per process if the caller has chosen a γ
        // whose decay reaches 1e-30 before the halfway mark of the window —
        // the LBR cost is severe (measured 55 % on seat 0 at 500k iters).
        use std::sync::OnceLock;
        static WARNED: OnceLock<()> = OnceLock::new();
        if gamma < 1.0 && total > 2000 {
            let half_window = (total as f64 * 0.5).max(1.0);
            let decay_at_half = (gamma as f64).powf(half_window);
            if decay_at_half < 1e-30 {
                WARNED.get_or_init(|| {
                    eprintln!(
                        "cham-blueprint: WARNING — avg_gamma={gamma} decays to \
                         {decay_at_half:.1e} over half of a {total}-iteration run; \
                         the effective averaging window is the LAST FEW HUNDRED \
                         iterations only. This is the γ-underflow pitfall that \
                         measured 55 % worse LBR on seat 0 (2026-09-28). Prefer \
                         avg_gamma=1.0 (Linear CFR+) or ≥ 0.9999."
                    );
                });
            }
        }
        base * (gamma as f64).powi((total.saturating_sub(t)).min(1 << 20) as i32)
    } else {
        base
    }
}

/// Build the M6 frozen victim opponent (v3 §6): victim encoder rebuilt from
/// the oracle's buckets dir + config (keys are encoder-content-addressed, so
/// this must match the snapshot's build exactly), rows materialized from the
/// snapshot export. Any failure is a loud training refusal, never a silent
/// uniform fallback.
fn build_frozen_opponent(
    spec: &cham_opponents::OpponentSpec,
    oracle: &crate::modes::FrozenOracle,
) -> Result<Box<dyn Agent>, BlueprintError> {
    let text = std::fs::read_to_string(&oracle.config_path).map_err(|e| {
        BlueprintError::Training(format!("frozen oracle config {}: {e}", oracle.config_path))
    })?;
    let cfg: cham_engine::config::AbstractionConfig = cham_engine::config::parse_config(&text)
        .map_err(|e| BlueprintError::Training(format!("frozen oracle config parse: {e}")))?;
    let encoder =
        cham_engine::Encoder::from_artifacts_dir(std::path::Path::new(&oracle.buckets_dir), cfg)
            .map_err(|e| BlueprintError::Training(format!("frozen oracle encoder: {e}")))?;
    Ok(cham_opponents::factory::build_frozen(
        spec,
        cham_opponents::PercentileChart::global(),
        encoder,
        cham_opponents::FrozenRows(oracle.rows.clone()),
    ))
}

/// Train per the config. Returns the table + provenance; writes snapshots into
/// `out_dir` and emits `bp_snapshot` records.
pub fn train(
    cfg: &TrainerConfig,
    mode: &TrainMode,
    engine_cfg: EngineConfig,
    enc: &mut cham_engine::Encoder,
    thread_mode: ThreadMode,
    out_dir: &Path,
    rec: Option<&mut Recorder>,
    resume_from: Option<&Path>,
) -> Result<(RegretTable, RunProvenance), BlueprintError> {
    train_with_threads(
        cfg,
        mode,
        engine_cfg,
        enc,
        thread_mode,
        default_threads(thread_mode),
        out_dir,
        rec,
        resume_from,
    )
}

/// Train with an explicit worker count (PERF-PLAN T5; overrides
/// [`default_threads`]). `Deterministic` ignores worker counts > 1 and stays
/// single-threaded by contract; Hogwild-class modes size their snapbatch
/// buffers from it.
#[allow(clippy::too_many_arguments)]
pub fn train_with_threads(
    cfg: &TrainerConfig,
    mode: &TrainMode,
    engine_cfg: EngineConfig,
    enc: &mut cham_engine::Encoder,
    thread_mode: ThreadMode,
    threads: u32,
    out_dir: &Path,
    mut rec: Option<&mut Recorder>,
    resume_from: Option<&Path>,
) -> Result<(RegretTable, RunProvenance), BlueprintError> {
    cfg.validate()?;
    std::fs::create_dir_all(out_dir)?;

    // EXPLORATION FLOOR (2026-09-29): read CHAM_TRAIN_EPS and set the
    // process-wide value read by `RegretTable::sigma_rms`. Default 0.0
    // → bit-identical to pre-2026-09-29 behavior.
    //
    // Why: RM+ floors regrets at zero, so at high iteration counts the
    // current iterate freezes one-hot and the averaged strategy follows
    // it. Measured mean max prob of the average strategy climbs 0.45
    // (500k) → 0.86 (20M) → 0.88 (50M). A concentrated average is
    // trivially exploitable. A small floor (~0.02) keeps every action's
    // regret channel alive. See docs/plans/RM-PLUS-FREEZE-2026-09-29.md.
    if let Ok(s) = std::env::var("CHAM_TRAIN_EPS") {
        match s.parse::<f64>() {
            Ok(v) if (0.0..=0.5).contains(&v) => {
                set_train_explore_eps(v);
                if v > 0.0 {
                    eprintln!("cham-blueprint: training exploration floor ε = {v}");
                }
            }
            Ok(v) => {
                eprintln!(
                    "cham-blueprint: CHAM_TRAIN_EPS={v} out of range [0, 0.5]; \
                     ignoring (floor stays {})",
                    crate::table::train_explore_eps()
                );
            }
            Err(e) => {
                eprintln!(
                    "cham-blueprint: CHAM_TRAIN_EPS parse error ({e}); \
                     floor stays {}",
                    crate::table::train_explore_eps()
                );
            }
        }
    }

    // Compute parallel_requested BEFORE the M-6 warning so we know
    // whether the parallel Robust path (added 2026-09-28, commit
    // 5c77401) will actually run. `robust` is used only for that
    // decision today.
    let robust = mode.tag() == TrainModeTag::Robust;
    // Only parallelize when the run is long enough that the 8-slice
    // warmup/parallel structure has room to work. Below this threshold,
    // slice_len is so small that warmup rounds to zero and the parallel
    // phase (allow_insert=false) can never create rows — the table stays
    // empty. Small tests and short experiments fall through to the
    // deterministic serial loop instead.
    const MIN_PARALLEL_ITERS: u64 = 100_000;
    // 2026-09-30: exploit vs a jitterable archetype opponent also uses the
    // parallel path now. Restricted to Arch/Jitter + frozen=None: frozen
    // oracles and ExploitBayes stay serial (they carry non-Sync state).
    let exploit_parallel_ok = match mode {
        TrainMode::Exploit {
            opponent, frozen, ..
        } => {
            frozen.is_none()
                && matches!(
                    opponent,
                    cham_opponents::OpponentSpec::Arch(_)
                        | cham_opponents::OpponentSpec::Jitter(_, _)
                )
        }
        _ => false,
    };
    let parallel_requested = thread_mode != ThreadMode::Deterministic
        && threads > 1
        && (robust || exploit_parallel_ok)
        && cfg.iters >= MIN_PARALLEL_ITERS
        && !matches!(mode, TrainMode::ExploitBayes { .. });

    // M-6 fix (2026-09-27, revised 2026-09-29): the serial path of this
    // trainer is single-threaded (Hogwild/Snapbatch are storage
    // strategies, not worker pools). But the Robust parallel path added
    // on 2026-09-28 DOES spawn `threads` real workers via
    // `train_robust_parallel`. The stale warning was firing even when the
    // parallel path ran, and `effective_threads` was hardcoded to 1, so
    // a 4-worker 20M run recorded `threads: 1` in its provenance — an
    // auditability lie. This revision:
    //   * emits the warning ONLY when the serial path will run,
    //   * records the ACTUAL worker count for the parallel path.
    let effective_threads: u32 = if parallel_requested { threads } else { 1 };
    if (threads > 1 || thread_mode != ThreadMode::Deterministic) && !parallel_requested {
        eprintln!(
            "cham-blueprint: WARNING — trainer is single-threaded on this path; \
             requested threads={threads} mode={thread_mode:?} but no worker \
             pool is implemented. Recording effective_threads=1 in \
             provenance (M-6). Artifact correctness is unaffected."
        );
    }

    let mut table = match resume_from {
        Some(p) => RegretTable::load_from(p)?,
        None => RegretTable::new(thread_mode),
    };
    let t0 = std::time::Instant::now();

    let mut dummy = DummyOpponent;
    let mut seat_histogram = [0u64; 2];

    // H-8 fix (2026-09-27): ExploitBayes needs a concrete family agent per
    // session block. The previous code sampled a family, set the belief
    // bin, and then fell through to `_ => None` for `iter_opp`, so the
    // traversal consulted `DummyOpponent`, whose `action_probs` returns
    // `Err`, and the traversal silently used uniform over the legal slots.
    // Every belief bin thus trained against the SAME random opponent — the
    // Bayesian-game arm (EXP-005) was invalid while appearing to work.
    // This cache holds the concrete agent for the current session block;
    // it is rebuilt only when the block index rolls over.
    let mut bayes_opp: Option<Box<dyn Agent>> = None;

    // H-9 fix (2026-09-27): start from the table's recorded last_iter, so a
    // resumed run continues the RNG bitstream from where the previous run
    // stopped instead of replaying iterations 0..N on top of the restored
    // table. `total_iters` is the GLOBAL end (start + this call's cfg.iters)
    // — averaging weights and cadence checks use it, not the local count.
    let start = table.last_iter();
    let total_iters = start + cfg.iters;

    // PERF (2026-09-29): dispatch to the parallel trainer when the caller
    // requested it. Currently Robust-only — the other modes need the same
    // treatment but involve additional stateful pieces (bayes blocks,
    // per-iteration opponent construction) that benefit less from
    // Hogwild. `deterministic` mode is never parallel (contract).
    //
    // Warmup = min(20% of this call's iterations, 50 000). The warmup's job
    // is to populate the table with every infoset the walk will see; the
    // parallel phase then only touches existing rows. If warmup is too
    // small, workers skip subtrees that would have been created later —
    // this is the same effect as a sample-starved run on those infosets,
    // and it self-corrects on the next warmup interval (per resume).
    // (parallel_requested computed above, next to the M-6 warning.)
    if parallel_requested {
        let warmup_iters = (cfg.iters / 5).min(50_000).max(1);
        eprintln!(
            "cham-blueprint: parallel {mode_tag} trainer — mode={thread_mode:?} \
             threads={threads} warmup={warmup_iters} total={total_iters}",
            mode_tag = if robust { "Robust" } else { "Exploit" }
        );
        // Per-slice checkpoints: snapshot the table to disk so a crash
        // doesn't lose the whole parallel run. Uses the same atomic
        // write (tmp+rename) as the outer loop.
        let snap_path = out_dir.join("table.snap");
        let checkpoint = |t: &RegretTable, iter: u64| -> Result<(), BlueprintError> {
            let bytes = t.snapshot();
            let tmp = snap_path.with_extension("tmp");
            std::fs::write(&tmp, &bytes)?;
            std::fs::rename(&tmp, &snap_path)?;
            if let Some(r) = rec.as_deref_mut() {
                let prov = serde_json::json!({
                    "iters": iter,
                    "infosets": t.len(),
                    "bytes": std::fs::metadata(&snap_path).map(|m| m.len()).unwrap_or(0),
                    "wall_s": t0.elapsed().as_secs_f64(),
                    "thread_mode": format!("{thread_mode:?}"),
                    "threads": effective_threads,
                    "threads_requested": threads,
                });
                let _ = r.record(RecordKind::BpSnapshot, prov);
            }
            Ok(())
        };
        // 2026-09-30: dispatch robust vs exploit-parallel.
        match mode {
            TrainMode::Exploit {
                opponent:
                    cham_opponents::OpponentSpec::Arch(a) | cham_opponents::OpponentSpec::Jitter(a, _),
                jitter_seed,
                frozen: None,
            } if !robust => {
                train_exploit_parallel(
                    &mut table,
                    cfg,
                    engine_cfg,
                    enc,
                    threads,
                    start,
                    total_iters,
                    *a,
                    *jitter_seed,
                    &mut seat_histogram,
                    checkpoint,
                )?;
            }
            _ => {
                train_robust_parallel(
                    &mut table,
                    cfg,
                    engine_cfg,
                    enc,
                    threads,
                    start,
                    total_iters,
                    warmup_iters,
                    &mut seat_histogram,
                    checkpoint,
                )?;
            }
        }
        // Snapshots and provenance are still emitted by the same
        // post-loop path below; set last_iter and skip the single-threaded
        // loop entirely.
        table.set_last_iter(total_iters);
        // Fall through to the snapshot + provenance code below by skipping
        // the single-threaded loop. We do that with an early guard on the
        // loop's range: start==total_iters means zero iterations.
        //
        // (Cannot `return` here because the snapshot record path below is
        // part of the contract.)
    }

    // When parallel ran, this range is empty and the single-threaded loop
    // is a no-op. The final snapshot (below) and provenance (after) still
    // fire, using the shared table's accumulated state.
    let loop_start = if parallel_requested {
        total_iters
    } else {
        start
    };
    let loop_end = total_iters;
    for t in loop_start..loop_end {
        // ---- ExploitBayes session blocks: hidden type + belief bin ----
        if let TrainMode::ExploitBayes {
            families,
            obs_noise,
            bins,
        } = mode
        {
            if t % cfg.bayes_session_block == 0 {
                let block_idx = t / cfg.bayes_session_block;
                let mut block_rng = child(cfg.train_seed, &format!("block{block_idx}"));
                let chosen = cham_core::rng::pick(&mut block_rng, families.len().max(1));
                let mut freq = vec![0f64; families.len().max(1)];
                freq[chosen] = 1.0;
                let bin = bins.bin_of(
                    &freq,
                    cfg.bayes_session_block as u32,
                    *obs_noise,
                    &mut block_rng,
                );
                enc.set_belief_bin(bin);

                // H-8: build the concrete family agent for this block.
                // Same dispatch as the Exploit arm, but with a per-block
                // seed (jitter-only; the belief bin already carries the
                // sampled-family signal). Uses a SEPARATE rng label
                // ("bayes-opp{block_idx}") so it does not perturb the
                // belief-bin sampling stream above.
                let spec = &families[chosen];
                let built: Box<dyn Agent> = match spec {
                    cham_opponents::OpponentSpec::Arch(a)
                    | cham_opponents::OpponentSpec::Jitter(a, _) => {
                        let mut jd = child(cfg.train_seed, &format!("bayes-opp{block_idx}"));
                        let seed = (cham_core::rng::next_u32(&mut jd) as u64) << 32 | block_idx;
                        Box::new(cham_opponents::archetype::ArchetypeAgent::jittered(
                            *a,
                            seed,
                            cham_opponents::PercentileChart::global(),
                        ))
                    }
                    cham_opponents::OpponentSpec::Frozen { .. } => {
                        // A frozen victim needs a FrozenOracle (rows +
                        // encoder), which TrainMode::ExploitBayes does not
                        // carry. Refuse loudly rather than train the
                        // Bayesian arm against a uniform fallback (which is
                        // exactly the H-8 failure mode we are fixing).
                        return Err(BlueprintError::Training(
                            "ExploitBayes: Frozen opponent family is not \
                             supported (only Exploit carries a FrozenOracle); \
                             refusing to train a Bayesian arm against a \
                             uniform fallback"
                                .into(),
                        ));
                    }
                    other => cham_opponents::factory::build(
                        other,
                        cham_opponents::PercentileChart::global(),
                    ),
                };
                bayes_opp = Some(built);
            }
        }

        let iter_rng: &mut Rng = &mut child(cfg.train_seed, &format!("iter{t}"));
        let hero_seat = if robust {
            // Diagnostic override (session 2026-09-27): force a specific
            // updating seat for the SB-vs-BB exploitability investigation.
            // Unset → historical (t % 2) alternation, bit-identical.
            match std::env::var("CHAM_FORCE_SEAT")
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
            {
                Some(s) if s < 2 => s,
                _ => (t % 2) as usize,
            }
        } else {
            cham_core::rng::pick(iter_rng, 2)
        };
        seat_histogram[hero_seat] += 1;
        let w_t = averaging_weight_gamma(t, total_iters, robust, cfg.avg_gamma);

        // ---- per-iteration opponent (jitter redraw: `jitter_seed ^ iter`) ----
        // materialize a concrete agent each iteration when the spec is an archetype
        let mut iter_opp: Option<Box<dyn Agent>> = match mode {
            TrainMode::Exploit {
                opponent,
                jitter_seed,
                frozen,
            } => match opponent {
                cham_opponents::OpponentSpec::Arch(a)
                | cham_opponents::OpponentSpec::Jitter(a, _) => {
                    Some(build_archetype_opponent(*a, *jitter_seed, t))
                }
                // v3 §6 (M6): the frozen victim — real snapshot rows, victim
                // encoder rebuilt from the oracle's buckets/config. Missing
                // oracle = loud refusal (a uniform "frozen" opponent would
                // train an exploiter against nothing and report a lie).
                cham_opponents::OpponentSpec::Frozen { .. } => {
                    let oracle = frozen.as_ref().ok_or_else(|| {
                        BlueprintError::Training(
                            "Exploit vs frozen: no FrozenOracle (rows/encoder) — refusing to train against a uniform fallback"
                                .into(),
                        )
                    })?;
                    Some(build_frozen_opponent(opponent, oracle)?)
                }
                other => Some(cham_opponents::factory::build(
                    other,
                    cham_opponents::PercentileChart::global(),
                )),
            },
            // H-8 fix (2026-09-27): hand the block-cached family agent to the
            // traversal. It is restored to `bayes_opp` after the walk so it
            // survives to the next iteration within this session block.
            TrainMode::ExploitBayes { .. } => bayes_opp.take(),
            _ => None,
        };
        let mut state = State::new(engine_cfg, Deck::shuffled(iter_rng))
            .map_err(|e| BlueprintError::Training(format!("engine: {e}")))?;
        let mut seq = ActionSeq::default();

        // Robust mode: the traversal samples the other seat's strategy from its own
        // rows; the dummy opponent is never consulted.
        let opp_dyn: &mut dyn Agent = match iter_opp.as_mut() {
            Some(b) => &mut **b,
            None => &mut dummy,
        };

        if thread_mode == ThreadMode::Snapbatch {
            let mut sink = SnapBatchSink::with_discount(cfg.regret_discount);
            {
                let mut walker = Traversal {
                    table: crate::traversal::TableRef::Exclusive(&mut table),
                    opp: opp_dyn,
                    rbp: RbpConfig::default(),
                    iteration: t,
                    total_iters: cfg.iters,
                    mode: mode.tag(),
                    hero_nodes: 0,
                    pruned_nodes: 0,
                    regret_discount: cfg.regret_discount,
                    allow_insert: true,
                    warmup_only: false,
                };
                walker.walk_with_sink(
                    &mut state, hero_seat, w_t, &mut seq, enc, iter_rng, &mut sink,
                );
            }
            sink.flush(&table);
        } else {
            let mut walker = Traversal {
                table: crate::traversal::TableRef::Exclusive(&mut table),
                opp: opp_dyn,
                rbp: RbpConfig::default(),
                iteration: t,
                total_iters: cfg.iters,
                mode: mode.tag(),
                hero_nodes: 0,
                pruned_nodes: 0,
                regret_discount: cfg.regret_discount,
                allow_insert: true,
                warmup_only: false,
            };
            walker.walk(&mut state, hero_seat, w_t, &mut seq, enc, iter_rng);
        }

        // H-8 fix (2026-09-27): return the block-cached family agent to
        // `bayes_opp` so the NEXT iteration within this session block uses
        // the same opponent (it is rebuilt only when the block index rolls
        // over). Outside ExploitBayes this is a no-op.
        if let TrainMode::ExploitBayes { .. } = mode {
            if let Some(b) = iter_opp.take() {
                bayes_opp = Some(b);
            }
        }

        // ---- snapshot cadence: renorm pass + save + record ----
        if (t + 1) % cfg.snapshot_every == 0 || t + 1 == total_iters {
            let mut renormed = 0u64;
            let entries: Vec<(u64, u32, usize)> = table
                .iter()
                .map(|(k, off)| (k, off, table.row_width(off)))
                .collect();
            for (_k, off, w) in entries {
                if table.renorm_row(off, w) {
                    renormed += 1;
                }
            }
            let _ = renormed;
            let snap_path = out_dir.join("table.snap");
            // H-9: stamp the resume position BEFORE snapshotting, so the
            // saved file carries the correct last_iter.
            table.set_last_iter(t + 1);
            let bytes = table.snapshot();
            let tmp = snap_path.with_extension("tmp");
            std::fs::write(&tmp, &bytes)?;
            std::fs::rename(&tmp, &snap_path)?;
            if let Some(r) = rec.as_deref_mut() {
                let prov = serde_json::json!({
                    "iters": t + 1,
                    "infosets": table.len(),
                    "bytes": std::fs::metadata(&snap_path).map(|m| m.len()).unwrap_or(0),
                    "wall_s": t0.elapsed().as_secs_f64(),
                    "thread_mode": format!("{thread_mode:?}"),
                    "threads": effective_threads,
                    "threads_requested": threads,
                });
                let _ = r.record(RecordKind::BpSnapshot, prov);
            }
            // v7 Item 2: LBR convergence checkpoints (independent of the
            // rolling table.snap above — each checkpoint is self-contained).
            if cfg.checkpoint_every > 0
                && ((t + 1) % cfg.checkpoint_every == 0 || t + 1 == total_iters)
            {
                if let Some(cp_dir) = cfg.checkpoint_dir.as_ref() {
                    let dir = cp_dir.join(format!("iter-{}", t + 1));
                    if std::fs::create_dir_all(&dir).is_ok() {
                        let bytes = table.snapshot();
                        let tmp = dir.join("table.snap.tmp");
                        let dst = dir.join("table.snap");
                        if std::fs::write(&tmp, &bytes).is_ok() {
                            let _ = std::fs::rename(&tmp, &dst);
                        }
                    }
                }
            }
        }
    }

    // H-9: stamp the final position after the loop, so a caller that
    // immediately calls `save_to` records the correct resume position even
    // if the final iteration didn't trigger a snapshot.
    table.set_last_iter(total_iters);

    let prov = RunProvenance {
        mode: mode.tag(),
        opponent_id: match mode {
            TrainMode::Exploit { opponent, .. } => Some(opponent.id()),
            TrainMode::ExploitBayes { families, .. } => Some(
                families
                    .iter()
                    .map(|f| f.id())
                    .collect::<Vec<_>>()
                    .join("|"),
            ),
            TrainMode::Robust => None,
        },
        depth_bb: cfg.depth_bb,
        // H-9: record the GLOBAL iteration count this table has trained
        // through (start + this call's iters), not the local count.
        iters: total_iters,
        train_seed: cfg.train_seed,
        thread_mode,
        // M-6: record the effective (actual) worker count, not the
        // requested one, so the provenance is truthful about what ran.
        threads: effective_threads,
        abstraction_hash: enc.abstraction_hash(),
        infosets: table.len(),
        wall_s: t0.elapsed().as_secs_f64(),
        parent: None,
        seat_histogram,
        // Read the process-wide floor (already set at startup from
        // CHAM_TRAIN_EPS). Recorded for the same reason thread_mode is.
        train_explore_eps: crate::table::train_explore_eps(),
    };
    let prov_path = out_dir.join("provenance.json");
    std::fs::write(&prov_path, serde_json::to_vec_pretty(&prov)?)?;
    Ok((table, prov))
}

/// PERF (2026-09-29): parallel Hogwild trainer for Robust mode.
///
/// Sliced design: the training range is divided into `N_SLICES` slices; each
/// slice runs a brief single-threaded warmup (populating any new infosets it
/// encounters) followed by a parallel burst (all workers write atomically
/// via the shared table). New keys introduced in slice K's parallel phase
/// are inserted by slice K+1's warmup; over the whole run the table
/// converges to full coverage.
///
/// Determinism: deliberately NOT deterministic — Hogwild trades
/// reproducibility for wall-clock speed (Niu et al. 2011). Only the
/// `Hogwild` and `Snapbatch` thread modes reach here.
#[allow(clippy::too_many_arguments)]
fn train_robust_parallel<F>(
    table: &mut RegretTable,
    cfg: &TrainerConfig,
    engine_cfg: EngineConfig,
    enc: &cham_engine::Encoder,
    threads: u32,
    start: u64,
    total_iters: u64,
    _warmup_iters: u64,
    seat_histogram: &mut [u64; 2],
    mut checkpoint: F,
) -> Result<(), BlueprintError>
where
    F: FnMut(&RegretTable, u64) -> Result<(), BlueprintError>,
{
    use std::sync::atomic::{AtomicU64, Ordering};

    // PERF (2026-09-30, rev 2): normally 8 slices per run. When
    // cfg.checkpoint_every > 0, use checkpoint_every as the slice length
    // so every slice boundary is a multiple of the checkpoint cadence.
    // My first attempt computed n_slices and divided — that clamps to 8
    // for a 20M/2M config (want=10, clamp(10,8,64)=10 works, but
    // want=5 clamps to 8, giving slice_len=2.5M and missing every
    // requested 2M boundary). Using slice_len = checkpoint_every
    // guarantees the boundaries align.
    //
    // Tradeoff: a very small checkpoint_every means many slices and many
    // short warmups. Cap the slice count at 64 to bound that.
    const N_SLICES_DEFAULT: u64 = 8;
    const N_SLICES_MAX: u64 = 64;

    let total_span = total_iters.saturating_sub(start);
    if total_span == 0 {
        return Ok(());
    }
    let slice_len: u64 = if cfg.checkpoint_every > 0 {
        let want_n = total_span.div_ceil(cfg.checkpoint_every);
        let bounded_n = want_n.min(N_SLICES_MAX).max(1);
        total_span.div_ceil(bounded_n)
    } else {
        total_span.div_ceil(N_SLICES_DEFAULT)
    };
    let mut slice_start = start;

    while slice_start < total_iters {
        let slice_end = (slice_start + slice_len).min(total_iters);
        let warmup_end = (slice_start + (slice_end - slice_start) / 5).min(slice_end);

        // ---- Warmup burst (single-threaded, allow_insert=true) ----
        {
            let mut enc_w = enc.clone();
            let mut dummy = DummyOpponent;
            for t in slice_start..warmup_end {
                let iter_rng = &mut child(cfg.train_seed, &format!("warm{t}"));
                let hero_seat = (t % 2) as usize;
                seat_histogram[hero_seat] += 1;
                let w_t = averaging_weight_gamma(t, total_iters, true, cfg.avg_gamma);
                let mut state = State::new(engine_cfg, Deck::shuffled(iter_rng))
                    .map_err(|e| BlueprintError::Training(format!("engine: {e}")))?;
                let mut seq = ActionSeq::default();
                let mut walker = Traversal {
                    table: crate::traversal::TableRef::Exclusive(table),
                    opp: &mut dummy,
                    rbp: RbpConfig::default(),
                    iteration: t,
                    total_iters,
                    mode: TrainModeTag::Robust,
                    hero_nodes: 0,
                    pruned_nodes: 0,
                    regret_discount: cfg.regret_discount,
                    allow_insert: true,
                    // WARMUP: insert-only. No CFR+ updates.
                    warmup_only: true,
                };
                walker.walk(&mut state, hero_seat, w_t, &mut seq, &mut enc_w, iter_rng);
            }
        }

        // ---- Parallel burst ----
        if warmup_end < slice_end {
            let counter = AtomicU64::new(warmup_end);
            let slice_end_local = slice_end;
            let regret_discount = cfg.regret_discount;
            let avg_gamma = cfg.avg_gamma;
            let train_seed = cfg.train_seed;
            let per_worker_hist: std::sync::Mutex<[u64; 2]> = std::sync::Mutex::new([0, 0]);

            std::thread::scope(|scope| {
                for _worker_id in 0..threads {
                    let counter_ref = &counter;
                    let table_ref: &RegretTable = table;
                    let enc_ref = enc;
                    let per_worker_hist_ref = &per_worker_hist;
                    scope.spawn(move || {
                        let mut enc_w = enc_ref.clone();
                        let mut dummy = DummyOpponent;
                        let mut local_hist = [0u64; 2];
                        loop {
                            let t = counter_ref.fetch_add(1, Ordering::Relaxed);
                            if t >= slice_end_local {
                                break;
                            }
                            let hero_seat = (t % 2) as usize;
                            local_hist[hero_seat] += 1;
                            let w_t = averaging_weight_gamma(t, total_iters, true, avg_gamma);
                            let deck = Deck::shuffled(&mut child(train_seed, &format!("d{t}")));
                            let mut state = match State::new(engine_cfg, deck) {
                                Ok(s) => s,
                                Err(_) => continue,
                            };
                            let mut seq = ActionSeq::default();
                            let mut walker = Traversal {
                                table: crate::traversal::TableRef::Shared(table_ref),
                                opp: &mut dummy,
                                rbp: RbpConfig::default(),
                                iteration: t,
                                total_iters,
                                mode: TrainModeTag::Robust,
                                hero_nodes: 0,
                                pruned_nodes: 0,
                                regret_discount,
                                allow_insert: false,
                                warmup_only: false,
                            };
                            let mut it_rng = child(train_seed, &format!("iter{t}"));
                            walker.walk(
                                &mut state,
                                hero_seat,
                                w_t,
                                &mut seq,
                                &mut enc_w,
                                &mut it_rng,
                            );
                        }
                        let mut h = per_worker_hist_ref.lock().expect("hist mutex");
                        h[0] += local_hist[0];
                        h[1] += local_hist[1];
                    });
                }
            });

            let h = per_worker_hist.lock().expect("hist mutex");
            seat_histogram[0] += h[0];
            seat_histogram[1] += h[1];
        }

        // PERF (2026-09-29): checkpoint at the end of every slice. With 8
        // slices over a 14-hour run, the worst-case loss on a crash is ~2
        // hours. Without this the parallel trainer produced NO intermediate
        // snapshot — a crash at hour 13 lost everything.
        checkpoint(table, slice_end)?;

        // PERF (2026-09-30): honor cfg.checkpoint_every in the parallel
        // path (the serial path always did). Write the same
        // `checkpoint_dir/iter-N/table.snap` shape so the freeze-evolution
        // diagnostic can read these snapshots identically.
        if cfg.checkpoint_every > 0 && slice_end % cfg.checkpoint_every == 0 {
            if let Some(cp_dir) = cfg.checkpoint_dir.as_ref() {
                let dir = cp_dir.join(format!("iter-{}", slice_end));
                if std::fs::create_dir_all(&dir).is_ok() {
                    let bytes = table.snapshot();
                    let tmp = dir.join("table.snap.tmp");
                    let dst = dir.join("table.snap");
                    if std::fs::write(&tmp, &bytes).is_ok() {
                        let _ = std::fs::rename(&tmp, &dst);
                    }
                }
            }
        }

        slice_start = slice_end;
    }

    table.set_last_iter(total_iters);
    Ok(())
}

/// Robust-mode placeholder opponent (never consulted: the traversal samples the
/// other seat's strategy from its own rows when mode == Robust).
struct DummyOpponent;
impl Agent for DummyOpponent {
    fn name(&self) -> &str {
        "robust-self"
    }
    fn act(
        &mut self,
        _obs: &cham_core::obs::Observables<'_>,
        _rng: &mut Rng,
    ) -> cham_core::engine::Action {
        unreachable!("cham-blueprint: invariant I1 (robust mode must not consult the dummy)")
    }
}

/// Build a per-iteration jittered archetype opponent. The jitter redraw
/// stream is per-iteration: `child(jitter_seed ^ t, "jd")` gives a fresh
/// seed for each `t`. This was originally inlined in the serial exploit
/// loop; extracted 2026-09-30 so the future parallel-exploit loop can
/// share the exact construction. See
/// `docs/plans/EXPLOIT-PARALLELIZATION-DESIGN-2026-09-30.md`.
///
/// `jitter_seed` is XORed with `t` (L-8 fix, 2026-09-27): the previous
/// form used a constant `child(jitter_seed, "jd")`, so per-iteration
/// entropy came only from `| t`. SPECS/04 §3 requires per-iteration
/// jitter entropy.
fn build_archetype_opponent(
    arch: cham_opponents::params::ArchetypeId,
    jitter_seed: u64,
    t: u64,
) -> Box<dyn Agent> {
    let mut jd = child(jitter_seed ^ t, "jd");
    let seed = (cham_core::rng::next_u32(&mut jd) as u64) << 32 | t;
    Box::new(cham_opponents::archetype::ArchetypeAgent::jittered(
        arch,
        seed,
        cham_opponents::PercentileChart::global(),
    ))
}

/// PERF (2026-09-30): parallel Hogwild trainer for Exploit mode against
/// Archetype opponents.
///
/// Scope (minimal):
/// - Only handles `TrainMode::Exploit { opponent: Arch | Jitter, frozen: None }`.
/// - Frozen, ExploitBayes, and any other mode fall through to serial (the
///   caller checks and refuses this path).
/// - New infosets encountered mid-parallel are silently skipped
///   (`allow_insert: false`), matching the Robust parallel path's
///   tradeoff: a few cold rows lost, warmup coverage not provided.
///
/// Determinism: NOT bit-identical to serial by design. Each worker draws
/// a unique `t` from the atomic counter, then constructs its own
/// `Box<dyn Agent>` via `build_archetype_opponent(arch, jitter_seed, t)`.
/// The RNG labels match the serial ones (`iter{t}`, `jd`), so streams are
/// disjoint. Iteration count for the averaging weight is the GLOBAL one.
///
/// See `docs/plans/EXPLOIT-PARALLELIZATION-DESIGN-2026-09-30.md`.
#[allow(clippy::too_many_arguments)]
fn train_exploit_parallel<F>(
    table: &mut RegretTable,
    cfg: &TrainerConfig,
    engine_cfg: EngineConfig,
    enc: &cham_engine::Encoder,
    threads: u32,
    start: u64,
    total_iters: u64,
    arch: cham_opponents::params::ArchetypeId,
    jitter_seed: u64,
    seat_histogram: &mut [u64; 2],
    mut checkpoint: F,
) -> Result<(), BlueprintError>
where
    F: FnMut(&RegretTable, u64) -> Result<(), BlueprintError>,
{
    use std::sync::atomic::{AtomicU64, Ordering};

    let total_span = total_iters.saturating_sub(start);
    if total_span == 0 {
        return Ok(());
    }
    // Small fixed slice count so we can checkpoint periodically and
    // observe progress. Same convention as Robust but simpler: no warmup
    // (opponent construction doesn't need row pre-population).
    const N_SLICES: u64 = 8;
    let slice_len = total_span.div_ceil(N_SLICES);
    let mut slice_start = start;

    while slice_start < total_iters {
        let slice_end = (slice_start + slice_len).min(total_iters);
        let warmup_end = (slice_start + (slice_end - slice_start) / 5).min(slice_end);

        // ---- Warmup burst (single-threaded, allow_insert=true) ----
        // Without this, the parallel burst's `allow_insert: false` means
        // no rows are ever created and the table stays empty. This burst
        // populates every infoset the slice's walks will touch, exactly
        // like the Robust parallel path's warmup. The opponent is
        // reconstructed per-iteration (same as the parallel phase), but
        // the walk is single-threaded here.
        {
            let mut enc_w = enc.clone();
            for t in slice_start..warmup_end {
                let hero_seat =
                    cham_core::rng::pick(&mut child(cfg.train_seed, &format!("warm-seat{t}")), 2);
                seat_histogram[hero_seat] += 1;
                let w_t = averaging_weight_gamma(t, total_iters, false, cfg.avg_gamma);
                let mut iter_rng = child(cfg.train_seed, &format!("iter{t}"));
                let mut state = State::new(engine_cfg, Deck::shuffled(&mut iter_rng))
                    .map_err(|e| BlueprintError::Training(format!("engine: {e}")))?;
                let mut seq = ActionSeq::default();
                let mut opp = build_archetype_opponent(arch, jitter_seed, t);
                let mut walker = Traversal {
                    table: crate::traversal::TableRef::Exclusive(table),
                    opp: &mut *opp,
                    rbp: RbpConfig::default(),
                    iteration: t,
                    total_iters,
                    mode: TrainModeTag::Exploit,
                    hero_nodes: 0,
                    pruned_nodes: 0,
                    regret_discount: cfg.regret_discount,
                    allow_insert: true,
                    warmup_only: false,
                };
                walker.walk(
                    &mut state,
                    hero_seat,
                    w_t,
                    &mut seq,
                    &mut enc_w,
                    &mut iter_rng,
                );
            }
        }

        // ---- Parallel burst ----
        if warmup_end >= slice_end {
            // Slice too small for warmup+parallel; skip to next slice.
            slice_start = slice_end;
            continue;
        }

        let counter = AtomicU64::new(warmup_end);
        let slice_end_local = slice_end;
        let regret_discount = cfg.regret_discount;
        let avg_gamma = cfg.avg_gamma;
        let train_seed = cfg.train_seed;
        let per_worker_hist: std::sync::Mutex<[u64; 2]> = std::sync::Mutex::new([0, 0]);

        std::thread::scope(|scope| {
            for _worker_id in 0..threads {
                let counter_ref = &counter;
                let table_ref: &RegretTable = table;
                let enc_ref = enc;
                let per_worker_hist_ref = &per_worker_hist;
                scope.spawn(move || {
                    let mut enc_w = enc_ref.clone();
                    let mut local_hist = [0u64; 2];
                    loop {
                        let t = counter_ref.fetch_add(1, Ordering::Relaxed);
                        if t >= slice_end_local {
                            break;
                        }
                        let hero_seat =
                            cham_core::rng::pick(&mut child(train_seed, &format!("seat{t}")), 2);
                        local_hist[hero_seat] += 1;
                        let w_t = averaging_weight_gamma(t, total_iters, false, avg_gamma);
                        let deck = Deck::shuffled(&mut child(train_seed, &format!("d{t}")));
                        let mut state = match State::new(engine_cfg, deck) {
                            Ok(s) => s,
                            Err(_) => continue,
                        };
                        let mut seq = ActionSeq::default();
                        // Per-iteration opponent: constructed inside the
                        // worker so the stateful ArchetypeAgent is not
                        // shared across threads.
                        let mut opp = build_archetype_opponent(arch, jitter_seed, t);
                        let mut walker = Traversal {
                            table: crate::traversal::TableRef::Shared(table_ref),
                            opp: &mut *opp,
                            rbp: RbpConfig::default(),
                            iteration: t,
                            total_iters,
                            mode: TrainModeTag::Exploit,
                            hero_nodes: 0,
                            pruned_nodes: 0,
                            regret_discount,
                            allow_insert: false,
                            warmup_only: false,
                        };
                        let mut it_rng = child(train_seed, &format!("iter{t}"));
                        walker.walk(
                            &mut state,
                            hero_seat,
                            w_t,
                            &mut seq,
                            &mut enc_w,
                            &mut it_rng,
                        );
                    }
                    let mut h = per_worker_hist_ref.lock().expect("hist mutex");
                    h[0] += local_hist[0];
                    h[1] += local_hist[1];
                });
            }
        });

        let h = per_worker_hist.lock().expect("hist mutex");
        seat_histogram[0] += h[0];
        seat_histogram[1] += h[1];

        checkpoint(table, slice_end)?;
        slice_start = slice_end;
    }

    table.set_last_iter(total_iters);
    Ok(())
}
