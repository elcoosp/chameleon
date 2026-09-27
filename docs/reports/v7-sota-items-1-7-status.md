# v7 SOTA competitiveness — items 1–7 status (2026-09-27 session)

Implements `docs/plans/v7-sota-competitiveness-runbook.md`. Code changes
landed this session; wall-clock-heavy sweeps are infra-ready with status noted.

## Item 1 — honest re-baseline: DONE (verified, pre-existing + confirmed)

- RBP-gate fix confirmed in tree (`traversal.rs` `prune_enabled = theta_t > 0.0`,
  `git log` shows `fe84467` in history).
- `theta-inf-3M-s7` ledgered (`honest-lbr-tiny-3M-s7`: mean 26,641 vs 37,757
  uniform, −29%). `artifacts/nopruning-diag/theta-inf-10M-s7/robust-7` bundle
  exists (10M honest retrain landed by the prior driver commit `ade80ff`);
  its LBR bench is still pending — run
  `cargo bench -p cham-blueprint --bench exploitability -- --save-baseline theta-inf-10M-s7`.
- EXP-011 / EXP-014-widened rows pre-`fe84467` remain STALE (no new numbers
  claimed here).

## Item 2 — convergence checkpoints: INFRA LANDED, sweep pending compute

- `TrainerConfig.{checkpoint_every, checkpoint_dir}` + `train-bp
  --checkpoint-every` landed; `docs/reports/convergence-curve.md` holds the
  baseline table + sweep loop + verdict rule. Full 100M sweep not run this
  session (wall-clock).

## Item 3 — DCFR α/γ honest re-sweep: INFRA READY, sweep pending Item 2

- Flags already exist (`--regret-discount`, `--avg-gamma`; defaults 1.0/0.9).
  Stale EXP-011 prior (γ=0.5, −8.6% at 100k collapsed iters) must NOT be
  trusted. Re-run the 9-cell grid at Item 2's operating iters, then ledger
  row `exp-011-dcfr-alpha-gamma-sweep-HONEST` per the runbook §3.2 schema.

## Item 4 — EMD bucket rebuild: STILL [WIP], not re-derived

- `artifacts/gpu-tables/flop-full/{flop.bin,flop.json}` present; turn bulk-fill
  log `artifacts/gpu-tables/turn-build.log` exists. Per BOARD, flop background
  job + queued turn (55M orbits) continue per v6 Item 6 §§6.1–6.7; promotion
  gated on Item 2's curve, not in isolation.

## Item 5 — search headroom + B-7 leaf blend: LANDED

- Default RNR iters 400 → 2000 (`trigger.rs`; ~5×, still inside the 250ms cap
  by linear extrapolation from the measured 13.15ms/400 — re-measure with
  `cargo bench -p cham-search --bench solve`).
- Multi-leaf continuation wired into the LIVE solve path (`solve.rs:
  blended_villain_prior` + per-villain-node blending before every solver
  branch), with `prior.rs: blended_leaf_prior/LEAF_BLEND_WEIGHTS` as the
  Action-typed companion. `turn re-solving` (`river_only: false`) recorded as
  TODO Item 5b (budget bench first).
- Tests: `multileaf_blend_wired_into_live_solve` green; `cargo test -p cham-search` green.

## Item 6 — router changepoint shield: LANDED (flag-gated), grid re-run pending

- `ChangepointShield` (run-length posterior, O(1) truncated, deterministic) +
  `effective_n0()` wiring in `weights_for_hand`, `reset_session` reset,
  `CHAM_ROUTER_CHANGEPOINT=1` env + `--router-changepoint-shield` CLI flag
  (process-wide, no `unsafe`), `hero.rs` A/B plumbing.
- Unit tests green: stationary stays near base N0; post-switch N0 drops.
- EXP-015 60-cell honest re-run (runbook §6.1 loop) + shielded A/B still
  pending Item 1's 10M LBR bench + compute.

## Item 7 — fallback hi-iters: STILL [WIP] (job running)

- `scripts/exp-014-hi-iters.sh` in flight (`artifacts/exp-014-hi-iters.log`
  shows slot 0 training at 2M iters). Verdict (capacity-confirmed vs
  capacity-ceiling + 5th-specialist TODO) on completion per v6 Item 7.
