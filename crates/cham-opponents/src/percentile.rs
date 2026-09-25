//! The 169-class equity chart (SPECS/03 §2): classes ranked by head-to-head
//! equity vs a uniform opponent (seeded MC — exact enumeration over all boards is
//! 2.8e11 evaluations; seeded MC at 20k iters gives SE ≈ 0.0035, spec budget ~1 s).

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use cham_core::card::{Card, Hand2};
use cham_core::eval::{Range, equity_mc};
use cham_core::rng::{Rng, child};

/// One chart entry: class id + its equity vs uniform preflop.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct ChartEntry {
    pub class_id: u8,
    pub equity: f64,
}

/// 169-entry chart, sorted by equity DESC (rank = position).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PercentileChart {
    entries: Vec<ChartEntry>,
    /// class_id → percentile (0 = strongest)
    pct_by_class: Vec<f64>,
}

const MC_ITERS: u32 = 20_000;

fn build_chart() -> PercentileChart {
    let mut entries: Vec<ChartEntry> = Vec::with_capacity(169);
    // canonical class representatives, constructed directly from the pinned ordering
    let mut reps: Vec<Hand2> = Vec::with_capacity(169);
    for r in 0..13u8 {
        // pairs: 22..AA (class 12−r)
        reps.push(Hand2::new(Card(r * 4), Card(r * 4 + 1)));
    }
    for hi in 1..13u8 {
        for lo in 0..hi {
            // suited then offsuit, matching class_id's t ordering
            reps.push(Hand2::new(Card(hi * 4), Card(lo * 4)));
            reps.push(Hand2::new(Card(hi * 4), Card(lo * 4 + 1)));
        }
    }
    for h in reps {
        let mut rng = child(0xC1A55, &format!("class{}", h.class_id()));
        let range = Range::all();
        let (w, t) = equity_mc(h, &range, &[], MC_ITERS, &mut rng);
        entries.push(ChartEntry {
            class_id: h.class_id(),
            equity: w + t / 2.0,
        });
    }
    assert_eq!(entries.len(), 169, "169 classes charted");
    entries.sort_by(|x, y| {
        y.equity
            .partial_cmp(&x.equity)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut pct_by_class = vec![0f64; 169];
    for (rank, e) in entries.iter().enumerate() {
        pct_by_class[e.class_id as usize] = rank as f64 / 169.0;
    }
    PercentileChart {
        entries,
        pct_by_class,
    }
}

static CHART: OnceLock<PercentileChart> = OnceLock::new();

impl PercentileChart {
    /// Build once per process (~1-2 s) and share.
    pub fn global() -> &'static PercentileChart {
        CHART.get_or_init(build_chart)
    }

    /// Percentile of a hand (0 = strongest, 1 = weakest).
    pub fn percentile(&self, h: Hand2) -> f64 {
        self.pct_by_class[h.class_id() as usize]
    }

    /// Equity of a hand class (interpolated to the exact combo by class identity).
    pub fn equity(&self, h: Hand2) -> f64 {
        self.entries
            .iter()
            .find(|e| e.class_id == h.class_id())
            .map(|e| e.equity)
            .unwrap_or(0.5)
    }

    /// Entry by rank (0 = strongest).
    pub fn entry(&self, rank: usize) -> &ChartEntry {
        &self.entries[rank.min(self.entries.len() - 1)]
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        false
    }
}

/// Sampling helper for tests: a uniform random hand.
pub fn random_hand(rng: &mut Rng) -> Hand2 {
    use cham_core::rng::next_u32;
    loop {
        let a = (next_u32(rng) % 52) as u8;
        let b = (next_u32(rng) % 52) as u8;
        if a != b {
            return Hand2::new(Card(a), Card(b));
        }
    }
}
