# Overnight run — 2026-09-25

> **Status:** [DONE] — run completed 2026-09-26; see worklog for numbers

## One question to answer by morning

**Do the CHAMELEON blueprints need the *full* abstraction to produce
real strength numbers, or is the tiny abstraction enough?**

The current ladder reads ±0.0 because `artifacts/agent/` does not exist —
that is a stub, not a signal. Tonight we build the agent bundle both ways
and run the same ladder against each.

## What's running now

- Turn EHS table build (`gpu-build --kind turn --limit 0`, pid at
  `artifacts/gpu-tables/turn-build.pid`), ~55% → ~40 min remaining.
  Its log will end with `wrote ...turn.bin (1436673240 bytes, blake3 ...)`.

## Plan — sequential, error-stopping, time-boxed

| # | Phase | Command (abridged) | Budget | Output |
|---|-------|--------------------|--------|--------|
| 0 | turn EHS (already running) | — | 1 h wait | `artifacts/gpu-tables/turn.bin` |
| 1 | tiny robust bp | `train-bp --mode robust --iters 10000 --seed 7 --threads 4` | 30 min | `artifacts/blueprints-tiny/robust-7/` |
| 2 | tiny 4 experts | `train-bp --mode exploit --opponent arch:{nit,tag,lag,station} --iters 10000` | 60 min | `artifacts/blueprints-tiny/exploit-*/` |
| 3 | assemble tiny agent | copy buckets + robust + experts into `artifacts/agent/` | 1 min | `artifacts/agent/` |
| 4 | tiny ladder | `ladder --fast --agent full` | 20 min | tiny per-opponent numbers |
| 5 | tiny probe | `probe --agent full` | 5 min | LBR proxy |
| 6 | full buckets | `train-buckets --profile full` | **3 h time-box** | `artifacts/buckets/` |
| 7 | full robust bp | `train-bp --mode robust` (targeting full buckets) | **2 h time-box** | `artifacts/blueprints-full/robust-7/` |
| 8 | swap in full agent | assemble `artifacts/agent/` from full artifacts | 1 min | — |
| 9 | full ladder | `ladder --fast --agent full` | 20 min | full per-opponent numbers |
| 10 | summary | parse both ladders, print comparison | 1 min | `summary.txt` |

Total budget ≈ 9 h. Leaves ~1 h buffer.

## Time-boxes and error handling

- Every phase has a wall-clock budget. If it exceeds, the driver sends
  SIGTERM, waits 10 s, SIGKILLs, then records `status=abort` and continues
  to the next phase (or skips dependents).
- `train-bp` and `train-buckets` have **no checkpoint** — a timed abort
  loses that phase. That's accepted; the artifact is regenerated tomorrow.
- The driver never competes for the GPU while phase 0 is running; it waits.

## Resource guards

- `nice -n 10` on every child.
- Memory watch: driver polls child RSS every 30 s; if > 12 GB (75 % of
  16 GB), SIGKILL and record — this is what bit us earlier.
- Disk watch: abort whole run if free disk drops below 3 GB.
- All logs to `artifacts/overnight-2026-09-25/<phase>.log`.

## What we won't do tonight

- `cargo bench` (would contend with training)
- Slumbot `--real` (network + cost)
- `ladder --full` (25k deals × 9 opponents = too long for one night)
- Anything v3-related (not scheduled; brainstorm doc exists)

## What to check tomorrow

1. `cat artifacts/overnight-2026-09-25/summary.txt` — one-line-per-phase status.
2. `tail -60 artifacts/overnight-2026-09-25/*.log` — full detail.
3. `git status` — no auto-commits, all artifacts go under git-ignored dirs.
4. Run `cargo run -q -p cham-cli -- verify --gpu` — will exercise the P7
   resample against the freshly built turn table.

## Failure modes and what they mean

| Symptom | Meaning | What to do |
|---|---|---|
| `tiny-ladder: real numbers ≠ ±0.0` | Unlock succeeded; the agent is live | Compare with full-ladder numbers |
| `tiny-ladder: still ±0.0` | Agent bundle not loaded; check guard paths | Inspect `artifacts/agent/` layout |
| `full-buckets: abort` | Full build did not fit in 3 h | Run tomorrow with `nice -n 15`; or accept tiny-only |
| `full-robust: abort` | Full bp did not fit in 2 h | Reduce iters, or use fewer buckets |
| `RSS > 12 GB at <phase>` | Memory guard tripped | Document the phase; re-run with fewer threads |
| Missing `PHASE_*` lines | Driver crashed | `tail` the driver log; the last phase is the culprit |


---

## UPDATE (post-implementation): driver phases as committed

The original plan's `full-buckets` phase was dropped: `sample_orbits: 0`
enumerates ~56 M canonical orbits (flop 1.29 M + turn 55 M), an
M2 multi-day build — it would have consumed the whole window and
aborted with nothing. The revised driver (`scripts/overnight-2026-09-25.sh`)
runs:

| # | Phase | Budget | Purpose |
|---|-------|--------|---------|
| 0 | turn-ehs-wait | 1 h | wait for the running turn EHS build |
| 1 | tiny-robust | 30 min | robust blueprint at 10k iters, tiny abstraction |
| 2 | tiny-expert-{nit,tag,lag,station} | 4 × 15 min | 4 experts at 10k iters |
| 3 | assemble agent | 1 min | layout artifacts/agent from tiny artifacts |
| 4 | tiny-ladder | 30 min | first real ladder (the ±0.0 unlock) |
| 5 | tiny-probe | 15 min | LBR proxy on the live agent |
| 6 | full-flop | 3 h | full flop EHS build (GPU is now free) |
| 7 | verify-gpu | 10 min | P7 resample on turn + flop |

Total budget ≈ 5 h 15 min. Leaves >4 h buffer.
