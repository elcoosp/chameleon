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
use crate::table::{RegretTable, ThreadMode};
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

pub fn default_avg_gamma() -> f32 {
    0.9
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
    // window uniform (w=1 for all t). The default keeps linear-averaging
    // with γ decay. Unset → historical behavior, bit-identical.
    if std::env::var("CHAM_AVG_UNIFORM").as_deref() == Ok("1") {
        return 1.0;
    }
    let d = total / 4;
    let base = if t > d { (t - d) as f64 } else { 0.0 };
    if robust {
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

    // M-6 fix (2026-09-27): the trainer is single-threaded. `Hogwild` and
    // `Snapbatch` are implemented as storage strategies (relaxed CAS-adds,
    // buffered writes) — they do NOT spawn worker threads, and `--threads`
    // is only serialized into provenance. Recording the REQUESTED count
    // without the workers existing is an auditability lie. Emit a loud
    // warning and record the EFFECTIVE worker count (1).
    let effective_threads: u32 = 1;
    if threads > 1 || thread_mode != ThreadMode::Deterministic {
        eprintln!(
            "cham-blueprint: WARNING — trainer is single-threaded; \
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

    let robust = mode.tag() == TrainModeTag::Robust;
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
    for t in start..total_iters {
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
                    // L-8 fix (2026-09-27): the jitter redraw stream must be
                    // per-iteration. The previous form used `child(jitter_seed,
                    // "jd")` — a CONSTANT — so the low 32 bits of the seed
                    // were drawn from the same stream every iteration and the
                    // only per-iteration entropy came from `| t`, i.e. the
                    // iteration INDEX itself. SPECS/04 §3 requires per-iteration
                    // jitter entropy; XOR the iteration into the seed label so
                    // each `t` gets a fresh draw.
                    let mut jd = child(*jitter_seed ^ t, "jd");
                    let seed = (cham_core::rng::next_u32(&mut jd) as u64) << 32 | t;
                    Some(Box::new(
                        cham_opponents::archetype::ArchetypeAgent::jittered(
                            *a,
                            seed,
                            cham_opponents::PercentileChart::global(),
                        ),
                    ))
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
                    table: &mut table,
                    opp: opp_dyn,
                    rbp: RbpConfig::default(),
                    iteration: t,
                    total_iters: cfg.iters,
                    mode: mode.tag(),
                    hero_nodes: 0,
                    pruned_nodes: 0,
                    regret_discount: cfg.regret_discount,
                };
                walker.walk_with_sink(
                    &mut state, hero_seat, w_t, &mut seq, enc, iter_rng, &mut sink,
                );
            }
            sink.flush(&table);
        } else {
            let mut walker = Traversal {
                table: &mut table,
                opp: opp_dyn,
                rbp: RbpConfig::default(),
                iteration: t,
                total_iters: cfg.iters,
                mode: mode.tag(),
                hero_nodes: 0,
                pruned_nodes: 0,
                regret_discount: cfg.regret_discount,
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
    };
    let prov_path = out_dir.join("provenance.json");
    std::fs::write(&prov_path, serde_json::to_vec_pretty(&prov)?)?;
    Ok((table, prov))
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
