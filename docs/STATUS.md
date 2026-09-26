# Project STATUS — 2026-09-26

Live snapshot. Update at the end of each working session.

## What's shipped and verified

| Area | Status |
|---|---|
| Core engine + evaluator | green (P1–P4 perf gates pass on M1) |
| Proofs P-1..P-4 | green (`verify --proofs`) |
| Tiny abstraction agent | ladder prints real numbers (was stubbed) |
| Full abstraction buckets | **building now** |
| GPU EHS tables | turn + flop built, blake3 recorded, `verify --gpu` green |
| bincode → postcard | **complete**, all tests pass, history purged of 88 MB cache |

## Known working numbers (tiny abstraction)

Ladder vs 9 opponents (500 seatings/pair, 500k-iter robust, 4 experts):
- pnash:overfold +5220 ± 1780
- noisy:0.1:arch:lag +1389 ± 1512
- everything else negative, real signal (un-beaten tiny agent)

Probe: LBR 29,973 mb/hand, coverage 0.91, acc_b_dev 0.84.

## Current blockers

None. Full-abstraction bp + ladder is queued; fires when buckets finish.

## In flight

- `artifacts/buckets-full/` build (turn phase, ~90 min elapsed, 3.4× parallel)
- `scripts/run-full-agent.sh` (pid polls for `turn.bin`, then trains robust +
  4 experts @ 500k iters, assembles `artifacts/agent-full/`, runs ladder)

## Recent landings (this session)

- `63b9550` parallel `enumerate_orbits` (2.9× — full-buckets unblocked)
- `8f43f8d` parallel kmeans++ init (O(kn) not O(k²n))
- `8410bea` train-bp `--config` / `--buckets` flags
- `f7b4e02` DCFR `--regret-discount` (opt-in, default off)
- `59d4b5a` + `7ea350a` B-2 persistent river cache (hydrate/save in play)
- `4f48df7` postcard migration complete
- `90ea1ad` force-pushed rewritten history (histo-cache purge)

## Perf backlog (docs/PERF-BACKLOG.md)

- B-1 rejected (3× slower)
- B-2, B-5 landed
- B-3, B-4 dead (trainer loop is serial)
- B-6..B-9 v3-scope or speculative

## Next candidates (post-buckets)

1. Full-abstraction ladder numbers — the real "competitive?" answer
2. Re-run A/B chains at correct iters (10k/100k/1M)
3. Slumbot 20k-seating baseline (external anchor)
4. Wire turn EHS into the runtime (v3 prereq — G2.x SKIP stands for v2)
