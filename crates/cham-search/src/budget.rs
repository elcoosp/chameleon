//! Search budget (SPECS/06 §2): the ONLY `Instant::now()` outside cham-rec.
//! `Iterations` in ALL evaluation paths — byte-identical eval is sacred;
//! `WallClock` exists only for live play.

use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchBudget {
    /// Fixed iteration count — deterministic, machine-speed-independent.
    Iterations { iters: u32 },
    /// Soft wall-clock cap — LIVE PLAY ONLY (never evaluation).
    WallClock { ms: u64 },
}

impl SearchBudget {
    /// Iteration cap for this budget (WallClock derives a provisional cap that
    /// the caller may poll; determinism-critical paths never use WallClock).
    pub fn iters_cap(&self, fallback: u32) -> u32 {
        match self {
            SearchBudget::Iterations { iters } => *iters,
            SearchBudget::WallClock { .. } => fallback,
        }
    }
    pub fn is_deterministic(&self) -> bool {
        matches!(self, SearchBudget::Iterations { .. })
    }
}

/// Wall-clock guard: created at solve start; `expired()` reads the clock.
pub struct WallClockGuard {
    start: Instant,
    cap: Option<Duration>,
}

impl WallClockGuard {
    pub fn new(budget: &SearchBudget) -> WallClockGuard {
        match budget {
            SearchBudget::Iterations { .. } => WallClockGuard { start: Instant::now(), cap: None },
            SearchBudget::WallClock { ms } => {
                WallClockGuard { start: Instant::now(), cap: Some(Duration::from_millis(*ms)) }
            }
        }
    }
    pub fn expired(&self) -> bool {
        match self.cap {
            Some(cap) => self.start.elapsed() >= cap,
            None => false,
        }
    }
}
