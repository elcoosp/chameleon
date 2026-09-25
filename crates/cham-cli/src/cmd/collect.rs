//! `chameleon collect` (SPECS/05 §4): builds the binary router dataset from
//! instrumented ladder sessions — one row per hand, session-clustered splits,
//! out-of-family rows labeled (family governance).

use cham_router::dataset::RbinRow;

pub fn run(out: &str, max_rows: usize) -> i32 {
    // instrumented sessions: jittered archetypes playing each other produce the
    // labeled rows (family A) — the labels come from the session spec, not inference
    let mut rows: Vec<RbinRow> = Vec::new();
    let mut session_id = 1u16;
    let mut rng = cham_core::rng::rng_from_seed(0xC011EC7);
    for arch in 0..4usize {
        // 625 sessions × 200 hands × 4 archetypes = 500k rows. The contractual
        // hyperparameters (minibatch 512, lr 0.05 halved per 10 epochs, ≤ 100
        // epochs) need ~10⁵ gradient steps to reach the well-calibrated optimum
        // — at the real 2M-row scale that comes free; the stub matches it here.
        for _session in 0..625 {
            let mut t = TrackerStub {
                hands: 60 + (session_id as u64 * 17) % 400,
            };
            for _hand in 0..200 {
                if rows.len() >= max_rows {
                    break;
                }
                let features = t.next_features(&mut rng, arch);
                // Family governance (SPECS/05 §4): out-of-family rows (family B
                // here) may ONLY live in C-split sessions — derive the family
                // from the session's split, never assign family per-session-id
                // blindly (the loader refuses A/B rows labeled out-of-family).
                let family = if cham_router::dataset::split_of_session(session_id)
                    == cham_router::dataset::SESSION_C
                {
                    1
                } else {
                    0
                };
                rows.push(RbinRow {
                    features,
                    label: arch as u8,
                    session_id,
                    family,
                });
            }
            session_id += 1;
        }
    }
    // deterministic subsample beyond the cap
    if rows.len() > max_rows {
        rows.truncate(max_rows);
    }
    // session splits are computed by session_id (A/B-dev/B-test/C); family-C rows
    // labeled so the loader refuses them for A/B-dev (governance)
    match cham_router::write_dataset(std::path::Path::new(out), &rows, 20) {
        Ok(()) => {
            println!("collect: {} rows → {out}", rows.len());
            crate::cmd::EXIT_OK
        }
        Err(e) => {
            eprintln!("collect: {e}");
            crate::cmd::EXIT_FAIL
        }
    }
}

/// Deterministic feature synthesizer (stand-in for the full instrumented session
/// driver; the real producer is `ladder --instrument` at M3).
struct TrackerStub {
    hands: u64,
}

impl TrackerStub {
    fn next_features(&mut self, rng: &mut cham_core::rng::Rng, arch: usize) -> Vec<f32> {
        let mut f = vec![0.5f32; 20];
        for (i, v) in f.iter_mut().enumerate() {
            let x = cham_core::rng::next_f64(rng);
            *v = (0.35 + 0.3 * x + 0.05 * ((self.hands as f64 + i as f64).sin())).clamp(0.0, 1.0)
                as f32;
        }
        f[0] = cham_router::features::maturity_feature(self.hands) as f32;
        // class signal: archetype k elevates its signature EWM stat (dims 1..=4
        // map to vpip/pfr/three_bet/call_3bet in the SPECS/05 §2 contract) so the
        // dataset has learnable structure. The REAL producer at M3 is
        // instrumented play (`ladder --instrument`) — this stub exists so the
        // collect → train-router → ladder cycle is exercisable end-to-end at the
        // scale the contractual hyperparameters need (minibatch 512 × lr 0.05).
        let sig = 1 + (arch % 4);
        f[sig] = (f[sig] + 0.45).min(1.0);
        f
    }
}
