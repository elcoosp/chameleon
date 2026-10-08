//! The `chameleon` binary (SPECS/09): orchestration only — no business logic.
//! Commands (the spec's §2 list): verify, train-buckets, train-bp, train-router,
//! collect, probe, ladder, ab, slumbot, play, trace, dashboard.
//!
//! D-013: SPECS/09 §2 says "11 exactly" while listing twelve commands (v1's
//! `replay` animation is gone); we implement the LIST and pin the count to it.

mod cmd;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "chameleon",
    version,
    about = "Project CHAMELEON — routed mixture of specialist blueprints"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Tier 0: workspace invariants, proofs, infoset estimates, perf gates
    Verify {
        #[arg(long)]
        perf: bool,
        #[arg(long)]
        count_infosets: bool,
        #[arg(long)]
        proofs: bool,
        /// GPU-track gates (P7/P8/P9); reads pre-built tables
        #[arg(long)]
        gpu: bool,
        /// Tables directory for --gpu (default: artifacts/gpu-tables)
        #[arg(long)]
        tables: Option<String>,
    },
    /// OFFLINE: flop/turn iso tables + k-means (SPECS/02 §3)
    TrainBuckets {
        #[arg(long, default_value = "config/abstraction.toml")]
        config: String,
        #[arg(long, default_value = "artifacts/buckets")]
        out: String,
        #[arg(long, default_value = "tiny")]
        profile: String,
    },
    /// Train one blueprint (SPECS/04)
    TrainBp {
        #[arg(long, default_value = "robust")]
        mode: String,
        #[arg(long)]
        opponent: Option<String>,
        #[arg(long, default_value = "7")]
        seed: u64,
        #[arg(long, default_value = "100")]
        depth: i64,
        #[arg(long, default_value = "10000")]
        iters: u64,
        #[arg(long, default_value = "artifacts/blueprints")]
        out: String,
        #[arg(long)]
        status: Option<String>,
        /// Worker threads (default: available parallelism; on Apple M1
        /// 4 workers usually beats 8 for this memory-bound workload)
        #[arg(long)]
        threads: Option<u32>,
        /// Thread mode: deterministic (default, bit-exact single-thread),
        /// hogwild (atomic adds), snapbatch (buffered atomic adds)
        #[arg(long)]
        thread_mode: Option<String>,
        /// Abstraction config TOML (default: config/abstraction-tiny.toml)
        #[arg(long)]
        config: Option<String>,
        /// Bucket directory (default: artifacts/buckets-tiny)
        #[arg(long)]
        buckets: Option<String>,
        /// DCFR positive-regret discount (1.0 = CFR+ classic; try 0.9)
        #[arg(long, default_value = "1.0")]
        regret_discount: f32,
        /// DCFR strategy-sum discount γ (v3 §3.1 α/γ split).
        ///
        /// Default 1.0 (Linear CFR+, no decay). The historical 0.9 default
        /// was measured 2026-09-28 to cost 55% worse LBR on seat 0: at
        /// γ=0.9, `γ^(T−t)` underflows to 0 in f64 for `T−t ≳ 700`, so the
        /// averaging window collapses to the last few hundred iterations
        /// of a still-oscillating CFR+ iterate. Prefer 1.0, or ≥ 0.9999 if
        /// you want recency. See `cham_blueprint::default_avg_gamma`.
        #[arg(long, default_value = "1.0")]
        avg_gamma: f32,
        /// F5 (2026-10-01): DCFR positive-regret discount exponent alpha.
        /// CFR+ classic is alpha=1.0; Brown & Sandholm 2019 DCFR uses alpha=1.5.
        #[arg(long, default_value = "1.0")]
        dcfr_alpha: f64,
        /// F5 (2026-10-01): DCFR negative-regret discount exponent beta.
        /// CFR+ classic is beta=1.0; Brown & Sandholm 2019 DCFR uses beta=0.0.
        #[arg(long, default_value = "1.0")]
        dcfr_beta: f64,
        /// Phase C: DCFR strategy-sum exponent gamma.
        #[arg(long, default_value = "2.0")]
        dcfr_gamma: f64,
        /// Reuse a cached blueprint with identical inputs (V2 A/B speedup)
        #[arg(long)]
        reuse: bool,
        /// Resume training from a snapshot path
        #[arg(long)]
        resume: Option<String>,
        /// Override the training cache directory
        #[arg(long)]
        cache_dir: Option<String>,
        /// v7 Item 2: write standalone LBR checkpoints every N iters to
        /// `<out>/checkpoints/iter-<t>/table.snap` (0 = disabled, default)
        #[arg(long, default_value = "0")]
        checkpoint_every: u64,
        /// Override the checkpoint directory (default: `<out>/checkpoints`).
        /// Useful for freeze diagnostics that want to write into a
        /// throwaway path (e.g. `artifacts/freeze-diag/checkpoints`).
        #[arg(long)]
        checkpoint_dir: Option<String>,
    },
    /// Train the router on a .rbin dataset (SPECS/05)
    TrainRouter {
        #[arg(long, default_value = "artifacts/router_rows.rbin")]
        rows: String,
        #[arg(long, default_value = "artifacts/routers/v1")]
        out: String,
        /// Feature set name to record on the model (2026-09-30). Determines
        /// which feature vector the runtime dispatches at inference:
        ///   opportunity-gated-20 (default), raw-opponent-10/11/19
        /// See docs/plans/ROUTER-INTEGRATION-DESIGN-2026-09-30.md.
        #[arg(long, default_value = "opportunity-gated-20")]
        feature_set: String,
    },
    /// Build the binary router dataset from instrumented sessions (SPECS/05 §4)
    Collect {
        #[arg(long, default_value = "artifacts/router_rows.rbin")]
        out: String,
        #[arg(long, default_value = "2000000")]
        max_rows: usize,
        /// Produce REAL instrumented features by playing the shipped agent
        /// against the archetypes, instead of the synthetic TrackerStub.
        /// The stub encodes the label in the features and its gate is
        /// vacuous; `--real` is the honest pipeline (see
        /// docs/plans/ROUTER-FAILS-ON-REAL-DATA-2026-09-29.md).
        #[arg(long)]
        real: bool,
        /// Bundle to load for --real (default artifacts/agent).
        #[arg(long, default_value = "artifacts/agent")]
        bundle: String,
        /// Sessions per opponent for --real.
        #[arg(long, default_value = "60")]
        sessions: u64,
        /// Hands per session for --real.
        #[arg(long, default_value = "500")]
        hands: u64,
        /// For --real: emit the 10-dim opponent-only feature vector
        /// (raw action frequencies) instead of the 20-dim
        /// opportunity-gated vector. Fixes the (opponent, hero-policy)
        /// leak. See docs/plans/ROUTER-FEATURE-LEAK-2026-09-29.md.
        #[arg(long)]
        raw_opponent: bool,
        /// For --real: emit the 11-dim opponent-only feature vector
        /// (10 raw frequencies + preflop/postflop tilt) — the tilt
        /// separates TAG from LAG where raw frequencies cannot. See
        /// docs/plans/ROUTER-TILT-FEATURE-DESIGN-2026-09-30.md.
        #[arg(long)]
        raw_opponent_11: bool,
        /// For --real: emit the 19-dim opponent-only feature vector
        /// (10 raw frequencies + 8 postflop bet-size histogram buckets +
        /// 1 preflop/postflop tilt). The bet-size histogram captures
        /// "which hands the opponent raises with", the last cheap signal
        /// on public history. See
        /// docs/plans/ROUTER-BET-SIZE-FEATURE-DESIGN-2026-09-30.md.
        #[arg(long)]
        raw_opponent_19: bool,
    },
    /// Tier 1 probe (LBR proxy, coverage, router calibration)
    Probe {
        #[arg(long, default_value = "full")]
        agent: String,
        /// P1 diagnostic: print per-expert fallback breakdown.
        #[arg(long)]
        diag_fallback: bool,
        /// Agent bundle directory (default: artifacts/agent).
        #[arg(long)]
        bundle: Option<String>,
        /// 2026-10-01 (F1): enable live river solving.
        #[arg(long)]
        search: bool,
    },
    /// Diagnostic: probe Apple Metal device + whitelist amendment status (GPU-PLAN G0.1)
    GpuDoctor,
    /// Tier 2 screening ladder
    Ladder {
        #[arg(long)]
        fast: bool,
        #[arg(long)]
        full: bool,
        #[arg(long, default_value = "full")]
        agent: String,
        #[arg(long, default_value = "config/pool.toml")]
        pool: String,
        /// 2026-10-01 (F1): enable live river solving for this ladder run.
        #[arg(long)]
        search: bool,
    },
    /// Tier 3 paired A/B with promotion
    Ab {
        #[arg(long, default_value = "full")]
        a: String,
        #[arg(long, default_value = "robust-only")]
        b: String,
        #[arg(long, default_value = "1000")]
        deals: u64,
        #[arg(long, default_value = "3")]
        clusters: usize,
        #[arg(long, default_value = "0.0")]
        margin: f64,
        /// Disable SPRT early-stopping (screening arms run SPRT by default)
        #[arg(long)]
        no_sprt: bool,
        #[arg(long)]
        promote: bool,
    },
    /// Slumbot anchor (diagnostic; SPECS/08 §5)
    Slumbot {
        #[arg(long, default_value = "20000")]
        seatings: u64,
        #[arg(long)]
        real: bool,
        #[arg(long)]
        yes_i_am_live: bool,
        #[arg(long)]
        resume: Option<String>,
    },
    /// Interactive play
    Play {
        #[arg(long, default_value = "full")]
        agent: String,
        #[arg(long, default_value = "100")]
        depth: i64,
        /// B6: solver warm-start (default OFF; flag-off path is bit-identical)
        #[arg(long)]
        search_warmstart: bool,
        /// 2026-10-01 (F1): enable live river solving for this session.
        /// Uses `EXP-SEARCH` as the auditable G4 ledger token.
        #[arg(long)]
        search: bool,
    },
    /// Textual decision traces (no animation — cut)
    Trace {
        #[arg(long)]
        run: String,
        #[arg(long, default_value = "20")]
        top: usize,
        #[arg(long, default_value = "fallback")]
        by: String,
    },
    /// Static dashboard (trimmed 4 sections)
    Dashboard {
        #[arg(long, default_value = "artifacts/reports/index.html")]
        out: String,
        #[arg(long, default_value = "50")]
        last: usize,
    },
    /// Pre-seed the river-subgame cache from a canonical fixture (v3 §1.2);
    /// run once before a multi-arm EXP-* sweep so the first arm is warm too
    WarmCache {
        #[arg(long, default_value = "config/pool.toml")]
        pool: String,
        #[arg(long, default_value = "artifacts/river-cache.bin")]
        out: String,
    },
    /// Check a pre-registered gate against the ledger (v3 §2.2)
    LintLedger {
        #[arg(long)]
        prereg: String,
        #[arg(long, default_value = "artifacts/ledger")]
        ledger: String,
    },
    /// Self-exploit audit vs a frozen snapshot: static LBR + adaptive
    /// router-manipulation probe, G-SELF with CI (v3 §6, diagnostic only)
    SelfExploit {
        /// Snapshot run dir containing `policy/policy.bin`
        #[arg(long)]
        snapshot: String,
        #[arg(long, default_value = "artifacts/buckets-tiny")]
        buckets: String,
        #[arg(long, default_value = "config/abstraction-tiny.toml")]
        config: String,
        #[arg(long, default_value = "200")]
        deals: u64,
        /// If > 0, also train a best-response exploiter vs the frozen victim
        #[arg(long, default_value = "0")]
        train_iters: u64,
        #[arg(long, default_value = "artifacts/blueprints")]
        out: String,
        /// EXP-015: manipulator switch point (default 40 = legacy behavior)
        #[arg(long, default_value = "40")]
        switch_at: u64,
        /// 2026-10-06: enable the live river search (safe-resolve gadget)
        /// on the adaptive victim, to A/B search ON vs OFF vs an adaptive
        /// exploiter.
        #[arg(long)]
        search: bool,

        /// EXP-015: router temp override (default: bundle default)
        #[arg(long)]
        router_temp: Option<f64>,
        /// EXP-015: router prior strength N0 override
        #[arg(long)]
        router_n0: Option<f64>,
        /// v7 Item 6: enable the Bayesian changepoint shield on the victim
        /// router (A/B vs the fixed-N0 baseline on the same EXP-015 grid)
        #[arg(long)]
        router_changepoint_shield: bool,
    },
    /// EXP-016 shadow ladder: snapshot a champion / gauntlet vs shadows
    Shadow {
        #[command(subcommand)]
        cmd: ShadowCmd,
    },
    /// EXP-018 empirical meta-strategy: Nash over the agent zoo from ledger
    MetaSolve {
        #[arg(long, default_value = "full,robust-only,argmax,bayes,fmbr")]
        modes: String,
        #[arg(long, default_value = "artifacts/ledger/ledger.jsonl")]
        ledger: String,
    },
    /// EXP-017 bucket-quality audit: within vs between realized-EV variance
    AuditBuckets {
        #[arg(long, default_value = "artifacts/audit.json")]
        input: String,
        /// Generate the audit file from a short match instead of reading it (EXP-017).
        #[arg(long)]
        generate: bool,
        /// Agent bundle dir for --generate (default: artifacts/agent).
        #[arg(long, default_value = "artifacts/agent")]
        bundle: String,
        /// Opponent pool TOML for --generate (default: config/pool.toml).
        #[arg(long, default_value = "config/pool.toml")]
        pool: String,
        /// Deals per opponent for --generate.
        #[arg(long, default_value = "40")]
        deals: u64,
        /// Output file for --generate (default: artifacts/audit.json).
        #[arg(long, default_value = "artifacts/audit.json")]
        out: String,
    },
}

#[derive(Subcommand)]
enum ShadowCmd {
    /// Snapshot the current champion policy rows into artifacts/shadow/
    Snapshot {
        #[arg(long, default_value = "artifacts/agent")]
        policy: String,
        #[arg(long, default_value = "artifacts/shadow")]
        out: String,
    },
    /// Gauntlet: challenger vs last-N frozen shadows
    Gauntlet {
        #[arg(long, default_value = "full")]
        agent: String,
        #[arg(long, default_value = "artifacts/shadow")]
        shadow_dir: String,
        #[arg(long, default_value = "10000")]
        deals: u64,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let code = match cli.command {
        Command::Verify {
            perf,
            count_infosets,
            proofs,
            gpu,
            tables,
        } => cmd::verify::run(perf, count_infosets, proofs, gpu, tables.as_deref()),
        Command::TrainBuckets {
            config,
            out,
            profile,
        } => cmd::train_buckets::run(&config, &out, &profile),
        Command::TrainBp {
            mode,
            opponent,
            seed,
            depth,
            iters,
            out,
            status,
            threads,
            thread_mode,
            config,
            buckets,
            regret_discount,
            avg_gamma,
            dcfr_alpha,
            dcfr_beta,
            dcfr_gamma,
            reuse,
            resume,
            cache_dir,
            checkpoint_every,
            checkpoint_dir,
        } => cmd::train_bp::run(
            &mode,
            opponent.as_deref(),
            seed,
            depth,
            iters,
            &out,
            status.as_deref(),
            threads,
            thread_mode.as_deref(),
            config.as_deref(),
            buckets.as_deref(),
            regret_discount,
            avg_gamma,
            dcfr_alpha,
            dcfr_beta,
            dcfr_gamma,
            reuse,
            resume.as_deref(),
            cache_dir.as_deref(),
            checkpoint_every,
            checkpoint_dir.as_deref(),
        ),
        Command::TrainRouter {
            rows,
            out,
            feature_set,
        } => cmd::train_router::run(&rows, &out, &feature_set),
        Command::Collect {
            out,
            max_rows,
            real,
            bundle,
            sessions,
            hands,
            raw_opponent,
            raw_opponent_11,
            raw_opponent_19,
        } => cmd::collect::run(
            &out,
            max_rows,
            real,
            &bundle,
            sessions,
            hands,
            raw_opponent,
            raw_opponent_11,
            raw_opponent_19,
        ),
        Command::Probe {
            agent,
            diag_fallback,
            bundle,
            search,
        } => cmd::probe::run(&agent, diag_fallback, bundle.as_deref(), search),
        Command::GpuDoctor => cmd::gpu_doctor::run(),
        Command::Ladder {
            fast,
            full,
            agent,
            pool,
            search,
        } => cmd::ladder::run(fast, full, &agent, &pool, search),
        Command::Ab {
            a,
            b,
            deals,
            clusters,
            margin,
            no_sprt,
            promote,
        } => cmd::ab::run(&a, &b, deals, clusters, margin, no_sprt, promote),
        Command::Slumbot {
            seatings,
            real,
            yes_i_am_live,
            resume,
        } => cmd::slumbot::run(seatings, real, yes_i_am_live, resume.as_deref()),
        Command::Play {
            agent,
            depth,
            search_warmstart,
            search,
        } => cmd::play::run(&agent, depth, search_warmstart, search),
        Command::Trace { run, top, by } => cmd::trace::run(&run, top, &by),
        Command::Dashboard { out, last } => cmd::dashboard::run(&out, last),
        Command::WarmCache { pool, out } => cmd::warm_cache::run(&pool, &out),
        Command::LintLedger { prereg, ledger } => cmd::lint_ledger::run(&prereg, &ledger),
        Command::SelfExploit {
            snapshot,
            buckets,
            config,
            deals,
            train_iters,
            out,
            switch_at,
            search,
            router_temp,
            router_n0,
            router_changepoint_shield,
        } => {
            if router_changepoint_shield {
                cham_router::enable_changepoint_global();
            }
            let overrides = match (router_temp, router_n0) {
                (Some(t), Some(n0)) => Some((t, n0, 0.5, -1.5)),
                (Some(t), None) => Some((t, 8.0, 0.5, -1.5)),
                (None, Some(n0)) => Some((0.7, n0, 0.5, -1.5)),
                (None, None) => None,
            };
            cmd::self_exploit::run(
                &snapshot,
                &buckets,
                &config,
                deals,
                train_iters,
                &out,
                switch_at,
                overrides,
                search,
            )
        }
        Command::Shadow { cmd } => match cmd {
            ShadowCmd::Snapshot { policy, out } => cmd::shadow::snapshot(&policy, &out),
            ShadowCmd::Gauntlet {
                agent,
                shadow_dir,
                deals,
            } => cmd::shadow::gauntlet(&agent, &shadow_dir, deals),
        },
        Command::MetaSolve { modes, ledger } => cmd::meta_solve::run(&modes, &ledger),
        Command::AuditBuckets {
            input,
            generate,
            bundle,
            pool,
            deals,
            out,
        } => {
            if generate {
                cmd::audit_buckets::run_generate(&bundle, &pool, deals, &out)
            } else {
                cmd::audit_buckets::run(&input)
            }
        }
    };
    // exit codes: 0 green, 1 failure, 2 budget refusal (SPECS/09 §3)
    match code {
        0 => Ok(()),
        c => std::process::exit(c),
    }
}
