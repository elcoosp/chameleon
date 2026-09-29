# CHAMELEON — Evening addendum to the 2026-09-29 session handoff

**Written:** 2026-09-30 early morning, after the DCFR alpha=0.9 result.
**Supersedes for the "evening findings" section:**
`HANDOFF-2026-09-29-FULL.md` (written 2026-09-29 22:02) and
`SESSION-HANDOFF-2026-09-29.md` (written 21:57).
**Repository root:** `/Users/adm/Documents/Repos/chameleon`
**Branch:** `main`
**HEAD at handoff:** see `git log --oneline -1` (should be at or after
`5a71c15 exp(scripts): queue delay0 + eps=0.02 combined experiment`).

The full 22:02 handoff is still the primary reference. This addendum
lists ONLY the material that landed after it.

---

## 1. What ran between 22:02 and 00:30

Three pipelines finished; two are still queued:

| run | finished | result |
|---|---|---|
| `par-20M-delay0`       | 22:20 | BB improves 7.4%, mean −3.5% |
| `par-20M-avguniform`   | 23:07 | BB floor reached, mean tied with delay0 |
| `par-5M-warmfix`       | 23:18 | BB −184, SB +671, mean worse at 5M |
| `par-20M-alpha09`      | 23:48 | CATASTROPHIC negative (2.5x worse) |
| `par-20M-alpha05`      | ETA 00:20 | pending |
| `freeze-diag` (queued) | ETA after alpha05 | pending |
| `par-20M-delay0-eps02` (queued) | ETA after freeze-diag | pending |

Logs to watch:
- `artifacts/dcfr-alpha-pipeline.log`
- `artifacts/freeze-diag-pipeline.log`
- `artifacts/delay0-eps02-pipeline.log`

---

## 2. The evening results, in one table (1000-deal LBR, depth 100)

| variant | SB | BB | mean | notes |
|---|---:|---:|---:|---|
| tiny 20M no-fix       | 13 319 | 14 706 | 14 012 | baseline |
| tiny 20M warmfix      | 13 554 | 14 090 | 13 822 | warmup fix, net win |
| tiny 20M eps=0.02     | 13 682 | 13 652 | 13 667 | floor, net win |
| **tiny 20M delay0**   | **13 431** | **13 618** | **13 524** | **best mean 20M so far** |
| tiny 20M avguniform   | 13 977 | 13 133 | 13 555 | BB floor reached |
| tiny 20M **alpha=0.9** | **35 344** | **25 834** | **30 589** | **catastrophic** |
| tiny 5M no-fix (peak) | 13 977 | **12 858** | **13 417** | **still SOTA** |
| tiny 5M warmfix       | 14 648 | 12 674 | 13 661 | net loss at 5M |
| medium 20M            | **13 237** | 14 021 | 13 629 | new SB SOTA at 20M |

**The frontier has not moved:** tiny 5M no-fix (1000 deals) is still the
shipping candidate on mean LBR.

**The most effective single lever this session is the averaging
schedule** (delay0 or avguniform). Both drop 20M BB by 7–11%. Neither
alone closes the gap to 12 858.

## 3. The DCFR negative is more interesting than its number

`--regret-discount 0.9` produced a policy 2.5x worse than no-fix on
BOTH seats. But `rm_freeze` on the same table shows the diagnostic
metrics improved exactly as designed:

| metric | no-fix 20M | alpha=0.9 |
|---|---:|---:|
| soft (<0.5)         | 3.3%  | **16.6%** |
| avg_near_frozen     | 60.0% | **11.9%** |
| mean cur max_p      | 0.871 | **0.734** |
| mean avg max_p      | 0.859 | **0.616** |

**The discount un-freezes the iterate to noise, not to a better
equilibrium.** The positive-regret discount at alpha=0.9 has a
half-life of ~10 iterations on a 20M run, so the "accumulated regret"
becomes the last ~10 iterations of noise. See
`DCFR-ALPHA09-NEGATIVE-2026-09-29.md`.

The correct reading of this whole session: **the RM+ freeze is not a
bug — it is correct convergence on rows whose true equilibria are
nearly pure.** The BB regression comes from a small set of rows whose
equilibria need mixing. Those rows need a *targeted* lever (schedule
weight, per-row exploration floor), not a *global* regret discount.
The exploration floor (eps=0.02) is the right shape of fix. DCFR is
the wrong shape.

## 4. Code fixes that landed this evening

### 4.1 M-6 warning is now serial-only (commit `da90405`)

The stale warning ("trainer is single-threaded, no worker pool") was
firing on the parallel-Robust path, which DOES spawn workers (added
2026-09-28, commit 5c77401). Provenance also hardcoded `threads: 1`
regardless. Both fixed. Future parallel runs will report the real
worker count in provenance.

### 4.2 `--checkpoint-dir` is now exposed on `train-bp` (commit `dcb96eb`)

`train-bp` only had `--checkpoint-every`, hardcoding the output dir to
`<out>/checkpoints`. The freeze-evolution script wanted a throwaway
path. `--checkpoint-dir PATH` now overrides.

### 4.3 `sb_root_internals` diagnostic is `#[ignore]`d (commit `d106161`)

It trained 300k iters with no asserts; the workspace `nextest run`
hung for >60s on it. Same pattern as `sb_dump.rs`. Run explicitly:

    cargo nextest run -p cham-blueprint --run-ignored all \
      -E 'test(sb_root_internals)'

### 4.4 `tracker_raw_freq.rs` now has the right actor (commit `0b1b4e9`)

The 14 tests for `Tracker::raw_opponent_frequencies` were failing
because the fold-action fixtures used `Player::Sb` (hero) instead of
`Player::Bb` (opponent). `observe_hand(hero_seat=0)` treats seat 1 as
the opponent, so hero folds were being counted as zero. Fixed. 14/14
green.

## 5. Known pipeline bugs that were NOT fixed

- `scripts/freeze-evolution-2026-09-29.sh` is dead code now. Its
  replacement is `scripts/freeze-diag-2026-09-29.sh`. Both are checked
  in. The original was launched while the release binary predated the
  `--checkpoint-dir` flag, so its freeze-diag arm failed. Its 5M
  warmfix arm ran anyway and produced the result in section 2.

## 6. Gotcha 5.1 bit us again

Every `cargo nextest run` regenerates the seven
`crates/cham-blueprint/artifacts/runs/*/provenance.json` files (only
`wall_s` changes) and three
`crates/cham-cli/artifacts/blueprints/robust-3/*` files. Revert them
before committing anything else:

    git checkout -- crates/cham-blueprint/artifacts/runs/*/provenance.json
    git checkout -- crates/cham-cli/artifacts/blueprints/robust-3/policy/policy.bin
    git checkout -- crates/cham-cli/artifacts/blueprints/robust-3/policy/provenance.json
    git checkout -- crates/cham-cli/artifacts/blueprints/robust-3/provenance.json

## 7. The watchexec duplication problem

The box has five `watchexec` processes:

    86485  ttys001  watchexec -w ./wr.sh  --clear -r ./wr.sh
    4504   ttys005  watchexec -w ./wr.sh  --clear -r ./wr.sh
    4739   ttys006  watchexec -w ./wr.sh  --clear -r ./wr.sh
    95621  ttys002  watchexec -w ./wr1.sh --clear -r ./wr1.sh
    23855  ttys004  watchexec -w ./wr1.sh --clear -r ./wr1.sh

`./wr1.sh` does not exist in the repo root, so the last two are stale
(they watch a missing file and never trigger). The three `wr.sh`
watchers each trigger on every write to `wr.sh`, so any commit step can
race against itself and produce duplicate commits (see the
`bfcf02a` / `0ac82c2` pair).

**Do not run `git commit` in a `wr.sh` script without making it
idempotent** (`git diff --cached --quiet` guard is enough). The
evening's scripts used the `git add ... && git commit` pattern, which
is safe because the second racing run finds nothing to commit — but
if a script modifies the tree non-idempotently, that pattern will
corrupt state. This is a live hazard until the extra watchers are
killed by the user.

## 8. What to do next

1. **Read `artifacts/par-20M-alpha05-lbr.log`** when it lands
   (ETA ~00:20). If alpha=0.5 is even worse than alpha=0.9, the DCFR
   lever is dead at any discount rate. If alpha=0.5 is *better* than
   alpha=0.9, the discount curve has a minimum somewhere and a softer
   discount (0.99) might be worth a try.

2. **Read `docs/plans/FREEZE-EVOLUTION-2026-09-29.raw.txt`** once
   freeze-diag runs. It will show where in the run the freeze starts.
   If it starts *before* T/4 (=5M of 20M), the delay0/avguniform wins
   make sense. If it starts *after* T/4, the schedule effect must be
   operating somewhere else.

3. **Read `artifacts/par-20M-delay0-eps02-lbr.log`** when the combined
   experiment finishes. If mean drops below 13 417, it becomes the new
   SOTA. If it's between delay0 (13 524) and 5M (13 417), the two
   levers don't compose.

4. **Do not chase the DCFR lever further at alpha ≤ 0.9.** The
   policy is too degraded. If regret-discounting is revisited, try
   alpha ≥ 0.99 or a different discount shape (Brown-Sandholm also
   discount negative regrets, which this workspace does not).

5. **Priority for the router remains the TAG/LAG separation.** See
   `ROUTER-RAW-FEATURES-RESULT-2026-09-29.md`. Aggregated action
   frequencies cannot distinguish them; the next step is raise-rate
   by street, or showdown-strength distribution.

## 9. What the frontier now looks like

    bundle:      artifacts/par-5M/robust-7/policy
    abstraction: config/abstraction-tiny.toml
    routing:     full (argmax)
    averaging:   gamma = 1.0
    ladder:      +7 146 mb/seating mean, 9/9 wins
    LBR:         13 977 / 12 858 (1000 deals)

No measured configuration this evening beats it on mean. The closest
are delay0 at 20M (13 524) and avguniform at 20M (13 555).

If a successor's new experiment beats 13 417 mean LBR at 1000 deals
with a non-tiny abstraction, or beats 13 417 on the *robust-only*
tiny frontier with a *single training script*, that is a new SOTA.

Good luck.
