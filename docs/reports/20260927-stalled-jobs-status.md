# Stalled / dead background jobs — status at 2026-09-27 14:30

Four background jobs from the v6/v7 sessions were claimed as "running" in
prior handoffs. Verified this session by `ps` + log:

| job | process | log tail | verdict |
|---|---|---|---|
| retrain-tiny-honest | alive (bash 73189 + station 21421) | on the last slot (station) | RUNNING, nearly done |
| nopruning-10M (theta-inf-10M-s7) | alive (PID 8243, 94 min) | still training | RUNNING, nearly done |
| exp-014-hi-iters | **no process** | stalled at "slot 0 <- jamfix" | **DEAD** |
| gpu-flop-full | **no process** | ends at 100-board smoke + "projected 3.5h" | **DEAD** |

## Why the two dead jobs will not be restarted

### exp-014-hi-iters
It retrains the widened-full blueprints at higher iters. Every blueprint
trained before commit `fe84467` (the RBP-gate fix) is **invalidated** —
see `docs/reports/20260927-rbp-gate-stale-results.md`. Restarting this job
would burn ~30 min producing artifacts whose policies are already known
to be in the collapsed regime. Do not restart; re-run this experiment
only after the honest re-baseline (v7 Item 1) lands and if the
capacity-vs-coverage question is still open.

### gpu-flop-full
Two independent blockers:
1. `crates/cham-gpu/src/bin/gpu-build.rs` only accepts `--kind turn|flop`.
   There is **no histogram output mode** — `--kind ehs-histogram`,
   `--profile`, `--street` do not exist. The full-orbit scalar EHS table
   was already built in the GPU-track sessions; this run re-did 100
   boards of it as a smoke test.
2. The EMD rebuild (v7 Item 4 / v6 Item 6) needs per-orbit
   `nextstreet_cdf16` histograms, which requires a **new kernel** (grep of
   `crates/cham-gpu/src/` for "nextstreet" or "histogram" returns nothing).

So the real state of v7 Item 4 is "not started" — not "stalled mid-flight."
The prerequisite is a GPU histogram kernel + CLI flags + a `train-buckets
--histo-source` flag, none of which exist yet.

## Verification recipe (for future agents)

A log timestamp alone is not evidence a job is alive. Always pair:

    tail -5 <log>
    ps -eo pid,etime,%cpu,command | grep -E '<name>|chameleon train-bp'

If the log's last line is old and `ps` shows nothing, the job died. If the
log is fresh but `ps` shows nothing, the job either just finished or
spawned a subprocess that already exited — check for a matching `train-bp`
process, not just the driver script.
