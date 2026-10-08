//! PCS training loop: iterate the tree walk over sampled boards and
//! extract the average strategy.
//!
//! The artifact writer is `cham_blueprint::BlueprintPolicy::build_artifact`,
//! which consumes a `(key, probs)` row set. This module produces that row
//! set via `RegretTable::export_average_strategy`; the caller (CLI) hands
//! it to `build_artifact` unchanged.

use crate::pcs::sampling::sample_board;
use crate::pcs::table::RegretTable;
use crate::pcs::walk::PcsIteration;
use cham_core::rng::rng_from_seed;
use cham_engine::encoder::Encoder;

/// Training configuration for PCS. Deliberately separate from
/// `TrainerConfig` (which carries tabular-only fields); the CLI builds a
/// `PcsConfig` from its args and passes it here.
#[derive(Clone, Debug)]
pub struct PcsConfig {
    pub iters: u64,
    pub seed: u64,
    pub dcfr_alpha: f64,
    pub dcfr_beta: f64,
    pub dcfr_gamma: f64,
    /// Log every N iterations (0 = silent).
    pub log_every: u64,
}

impl Default for PcsConfig {
    fn default() -> Self {
        PcsConfig {
            iters: 1_000_000,
            seed: 7,
            dcfr_alpha: 1.5,
            dcfr_beta: 0.0,
            dcfr_gamma: 2.0,
            log_every: 0,
        }
    }
}

/// Run `cfg.iters` PCS updates. Returns the trained table.
pub fn run_pcs(iter: &PcsIteration<'_>, encoder: &mut Encoder, cfg: &PcsConfig) -> RegretTable {
    let mut table = RegretTable::new();
    let mut rng = rng_from_seed(cfg.seed);
    let t0 = std::time::Instant::now();
    for t in 1..=cfg.iters {
        let board = sample_board(&mut rng);
        iter.run(
            encoder,
            &mut table,
            board,
            t,
            cfg.dcfr_alpha,
            cfg.dcfr_beta,
            cfg.dcfr_gamma,
        );
        if cfg.log_every > 0 && t % cfg.log_every == 0 {
            let dt = t0.elapsed().as_secs_f64();
            eprintln!(
                "pcs: iter {t}/{} ({:.1}s, {:.0} iter/s, {} rows)",
                cfg.iters,
                dt,
                t as f64 / dt.max(1e-9),
                table.len()
            );
        }
    }
    table
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_design_values() {
        let c = PcsConfig::default();
        assert_eq!(c.dcfr_alpha, 1.5);
        assert_eq!(c.dcfr_beta, 0.0);
        assert_eq!(c.dcfr_gamma, 2.0);
        assert_eq!(c.seed, 7);
    }
}
