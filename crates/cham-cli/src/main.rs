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
        /// Reuse a cached blueprint with identical inputs (V2 A/B speedup)
        #[arg(long)]
        reuse: bool,
        /// Resume training from a snapshot path
        #[arg(long)]
        resume: Option<String>,
        /// Override the training cache directory
        #[arg(long)]
        cache_dir: Option<String>,
    },
    /// Train the router on a .rbin dataset (SPECS/05)
    TrainRouter {
        #[arg(long, default_value = "artifacts/router_rows.rbin")]
        rows: String,
        #[arg(long, default_value = "artifacts/routers/v1")]
        out: String,
    },
    /// Build the binary router dataset from instrumented sessions (SPECS/05 §4)
    Collect {
        #[arg(long, default_value = "artifacts/router_rows.rbin")]
        out: String,
        #[arg(long, default_value = "2000000")]
        max_rows: usize,
    },
    /// Tier 1 probe (LBR proxy, coverage, router calibration)
    Probe {
        #[arg(long, default_value = "full")]
        agent: String,
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
        #[arg(long)]
        sprt: bool,
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
            reuse,
            resume,
            cache_dir,
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
            reuse,
            resume.as_deref(),
            cache_dir.as_deref(),
        ),
        Command::TrainRouter { rows, out } => cmd::train_router::run(&rows, &out),
        Command::Collect { out, max_rows } => cmd::collect::run(&out, max_rows),
        Command::Probe { agent } => cmd::probe::run(&agent),
        Command::GpuDoctor => cmd::gpu_doctor::run(),
        Command::Ladder {
            fast,
            full,
            agent,
            pool,
        } => cmd::ladder::run(fast, full, &agent, &pool),
        Command::Ab {
            a,
            b,
            deals,
            clusters,
            margin,
            sprt,
            promote,
        } => cmd::ab::run(&a, &b, deals, clusters, margin, sprt, promote),
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
        } => cmd::play::run(&agent, depth, search_warmstart),
        Command::Trace { run, top, by } => cmd::trace::run(&run, top, &by),
        Command::Dashboard { out, last } => cmd::dashboard::run(&out, last),
    };
    // exit codes: 0 green, 1 failure, 2 budget refusal (SPECS/09 §3)
    match code {
        0 => Ok(()),
        c => std::process::exit(c),
    }
}
