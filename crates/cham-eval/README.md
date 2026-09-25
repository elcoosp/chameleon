# cham-eval

Numbers with defensible intervals (SPECS/08). Duplicate-deck matching:
profit(d) = (netA + netB)/2 — the SUM cancels seat advantage; identical
opponent streams per deal index make A/B paired.

- `stats.rs` — session-clustered bootstrap CIs, paired CIs, Welch, Wald SPRT,
  Holm correction, sample-size math (σ calibrated per opponent).
- `vr.rs` — all-in-EV adjustment + AIVAT-style known-opponent baseline
  (validated ≥ 1.5× variance reduction or auto-disabled).
- `slumbot.rs` — the published dialect (`/api/login|new_hand|act`, k/c/f/b)
  with rate limiting, backoff, and a mock for tests; verify-first gate.
- `ab.rs`/`ledger.rs` — paired verdicts (promote ⟺ CI lower > margin AND Holm
  passes) and the append-only ledger; `dashboard.rs` — trimmed 4-section HTML.
