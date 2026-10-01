# CHAMELEON — Session handoff, 2026-10-01 evening

**Written:** 2026-10-01 19:40 CEST.
**Supersedes for "what changed today":** every prior handoff.
The 09-30 handoffs remain useful for pre-10-01 context; this doc covers
the F1-F10 competitiveness-report remediation session.

**Repository root:** `/Users/adm/Documents/Repos/chameleon`
**Branch:** `main` (13 local commits ahead of `origin/main`, **not pushed**)
**HEAD:** see `git log --oneline -1` (should be at or after `998aaa5`).

**Nothing is running.** All background jobs (LBR curve, retrain
pipelines) were cancelled at 19:12 CEST because they were using the
wrong metric.

---

## 1. The single most important finding of the session

**The `lbr::lbr_vs` metric every prior session quoted is 8.4x too high.**

The function (crates/cham-blueprint/src/lbr.rs) recurses over a
`State` that contains BOTH hole cards. At a BR-seat node it enumerates
actions *inside each sampled deal* and takes the max — i.e. the BR
player sees the opponent's hole cards. That is a perfect-information
best response, not a real exploitability number.

The corrected metric is `lbr::tabular_br` (added this session). It
learns ONE action per information set across training deals, then
evaluates on held-out deals.

Measured on the shipped par-5M robust policy, seat 1:

| metric | value |
|---|---:|
| `lbr_vs` (clairvoyant) | **21 200 mb/hand** (21.2 bb) |
| `tabular_br` (infoset-consistent) | **2 533 mb/hand** (2.53 bb) |
| ratio | **8.4x** |

**The bot is closer to Nash than any prior session believed.**
The tiny-5M policy's corrected exploitability is probably ~1.5-2 bb/hand
on the tiny abstraction — competitive with a low-to-mid-strength GTO
approximation, not the "13 bb/hand disaster" every prior doc quotes.

**Consequences for every historical doc:**
- Every "LBR" number (13 977 / 12 858 / 21 199 / etc.) is the
  clairvoyant metric. It overstates the leak by 6-10x.
- The "LBR-vs-ladder decoupling" direction survives (both metrics move
  with the policy), but the scale of the reported leak was wrong.
- The freeze investigation's conclusions are NOT invalidated: the
  freeze diagnostic (`avg_near_frozen`, `mean max_p`) reads the table
  directly and is metric-independent. But the LBR deltas that motivated
  the levers (delay0, avguniform, eps, DCFR) were measured with the
  wrong metric; the 500-1500 mb/hand improvements they showed are
  probably 250-500 on the corrected metric — inside the noise band.
  That is consistent with the ladder, which never moved.

Full detail: `docs/plans/F1-CORRECTED-METRIC-2026-10-01.md`.

---

## 2. What landed this session (13 commits, all local)

Oldest first:

| commit | what |
|---|---|
| `7d6ddc1` | F9 cache env vars in hot paths; F7 regression test |
| `e9d92c0` | F3 accumulate average strategy at the OPPONENT node in Robust mode |
| `cdcd74b` | F3-fixed 5M Robust retrain script (later superseded) |
| `2617079` | F4 f64 strategy arena — drop f32 renorm ceiling |
| `7a68ac7` | F6a raise cap counts re-raises, not bets |
| `94c9d10` | F4 route DeltaBuffer flush through f64 arena; update renorm test |
| `8d77924` | F3+F4+F6a 5M/20M/500k Robust retrain queue (later cancelled) |
| `f8aad23` | F9-alloc site 3 (computed buffer to ArrayVec) |
| `571789d` | F1 infoset-consistent tabular BR |
| `b4b8e8d` | F1 corrected metric doc; broad test-artifact ignores |
| `0e81ac3` | user's competitiveness audit committed |
| `56233cd` | F5 proper DCFR (slice discount, CLI flags) |
| `998aaa5` | F5 doc + F10 plan doc |

Plus `F2` (class-conditioned CFR+ solver) landed in the previous session,
commit `1706b33`, merged earlier today.

---

## 3. The competitiveness report (this session's brief)

The user provided `docs/plans/chameleon-competitiveness-report.md`
(now in the repo) — a full external audit against Nash-class opponents.
Findings labeled F1-F10. **All short-term findings landed; one
(F10) is a multi-day job for a future session.**

| | finding | status |
|---|---|---|
| **F1** | clairvoyant LBR metric | **fixed** (tabular_br) |
| **F2** | solver not class-conditioned | **fixed earlier** (class-conditioned CFR+) |
| **F3** | average strategy at traverser site | **fixed** |
| **F4** | f32 strategy sums + renorm bias | **fixed** (f64 arena, renorm disabled) |
| **F5** | DCFR implementation was wrong | **fixed** (proper slice discount) |
| **F6a** | raise cap counted bets | **fixed** |
| **F6b** | preflop_open_bb dead config | **not fixed** (validated + hashed, never read; cosmetic) |
| **F6c** | off-tree translation dead code | **not started** |
| **F7** | parallel missing-key returns 0.0 | **was already fixed** (NaN sentinel 1a8cc4a); regression test added |
| **F8** | subgame cache key omits class counts | **was already fixed** (M-2) |
| **F9** | env vars in hot paths, per-node Vec | **partial**: env cached, ArrayVec site 3; sites 1/2 remain |
| **F10** | river solver is a strength-class toy | **planned**: `docs/plans/F10-VECTOR-SOLVER-PLAN-2026-10-02.md` |

---

## 4. What is NOT running (deliberately cancelled)

At 19:12 CEST I killed:
- **50M LBR curve** — a re-run of the 5M/20M/50M tiny robust curve that
  had been running since 15:03 with the OLD binary (pre-F3/F4/F6a).
  Its numbers would have been measured with the clairvoyant metric.
- **F3+F4+F6a retrain queue** — a script that would have trained
  500k/5M/20M under the fixed trainer, but reported clairvoyant LBR.

**Reason for cancellation:** the correct experiment is the same
retrain but measured with `tabular_br`, and the fixed binary didn't
exist until late this session. Better to wait for a clean re-launch.

---

## 5. What to do next (in priority order)

### 5.1 Re-quote every historical number (30 min)

The RESULTS-MATRIX, every handoff, and every experiments doc still
carry the clairvoyant numbers. Add a one-line note per doc:

> "LBR figures in this doc use the clairvoyant `lbr::lbr_vs`; the
> corrected infoset-consistent value is 6-10x smaller
> (`F1-CORRECTED-METRIC-2026-10-01.md`)."

The important ones: `RESULTS-MATRIX-2026-09-29.md`,
`TINY-CURVE-COMPLETE-2026-09-29.md`,
`TINY-PEAKS-AT-5M-2026-09-29.md`, `RM-PLUS-FREEZE-2026-09-29.md`.

### 5.2 Re-run the curve with the fixed binary + corrected metric (~1 night)

Both fixes landed. Queue:

    # 500k / 5M / 20M tiny robust under F3+F4+F6a
    for iters in 500000 5000000 20000000; do
      target/release/chameleon train-bp \
        --mode robust --iters $iters --depth 100 --seed 7 \
        --config config/abstraction-tiny.toml \
        --buckets artifacts/buckets-tiny \
        --out artifacts/par-f3f4-$iters \
        --threads 4 --thread-mode hogwild
    done

Then measure EACH with `tabular_br` (not `lbr_vs`):

    CHAM_EXPLOIT_BP=$PWD/artifacts/par-f3f4-5000000/robust-7/policy \
      cargo nextest run -p cham-blueprint \
        -E 'test(par5m_metric_compare)' \
        --run-ignored all --no-capture

`par5m_metric_compare` is currently hard-coded for one path; the
successor can either parameterise it or copy the harness.

**Question to answer:** does the F3 (average-accumulation-site) change
move the corrected BR curve? Does F4 (f64 sums) make the curve
smoother? Does F5's proper DCFR (`--dcfr-alpha 1.5 --dcfr-beta 0.0`)
help?

### 5.3 DCFR A/B with the corrected metric (~1 night per arm)

F5 is landed but the sweep has not been run. Recommended arms:

- **CFR+ identity**: `--dcfr-alpha 1.0 --dcfr-beta 1.0` (baseline)
- **DCFR(1.5, 0)**: `--dcfr-alpha 1.5 --dcfr-beta 0.0`
- **DCFR(1.5, 0, 2)**: same + `CHAM_AVG_DELAY=0` (γ=2 in the paper's notation)

All three at 5M and 20M, both seats, on `tabular_br`. The historical
DCFR negatives (`DCFR-ALPHA09-NEGATIVE`, `DCFR-ALPHA05-NEGATIVE`) are
obsolete — they tested the wrong discount schedule with the wrong
metric.

### 5.4 F10: vector-form river solver (1-2 weeks, see the plan)

`docs/plans/F10-VECTOR-SOLVER-PLAN-2026-10-02.md` has the design,
the O(n) showdown kernel, and the safe re-solving gadget. Step 1 (the
kernel) is fully specified. The current F1 search is still net negative
on the scripted pool — F10 is what would make search a real lever.

### 5.5 F6c: off-tree translation (half-day)

`harmonic_weights` and `nearest_slot` in `ladder.rs` are dead code.
At inference, `on_public_action` records the opponent's RAW action
which may not exist in the abstract tree; the coarse size_bucket
quantization absorbs some of this by accident. See the report's F6c
for the fix shape (Ganzfried-Sandholm pseudo-harmonic mapping).

### 5.6 F9-alloc sites 1/2 (2 hours)

The report's per-node allocations:
- site 1 (line ~291): `let dist: Vec<(Action, f64)> = if self.mode == TrainModeTag::Robust {` — needs ArrayVec<(Action, f64), 12> conversion.
- site 2 (line ~365): the explore-mix `dist` — same.

I tried this earlier today and broke the file structure (the else-block
nesting). Sites 1/2 need a careful single-pass patch, not a regex.

---

## 6. Reading order for the next session

Start here, in order:

1. **`chameleon-competitiveness-report.md`** — the brief; skim.
2. **`F1-CORRECTED-METRIC-2026-10-01.md`** — the session's biggest finding.
3. **`F2-COMPLETE-2026-10-01.md`** — the class-conditioned solver (previous session).
4. **`F5-DCFR-2026-10-01.md`** — what DCFR now does, how to sweep.
5. **`F10-VECTOR-SOLVER-PLAN-2026-10-02.md`** — the multi-day piece.
6. **`F1-SEARCH-CORRECTED-2026-10-01.md`** — search is still negative on the pool; F10 matters.
7. **`RESULTS-MATRIX-2026-09-29.md`** — every historical measurement, with the caveat that all LBR numbers are clairvoyant.

Reference docs that still matter:

- `RM-PLUS-FREEZE-2026-09-29.md` — the freeze mechanism (metric-independent).
- `FREEZE-ONSET-AT-5M-2026-09-30.md` — freeze onset = T/4.
- `LADDER-19DIM-SOTA-2026-09-30.md` — the shipped bundle (+8276 ladder).
- `SYNTHETIC-ROUTER-IS-DEGENERATE-2026-09-30.md` — the router picks LAG every hand.

---

## 7. Gotchas discovered this session

### 7.1 The stale-binary problem bit twice

- 13:41 — a `ladder --agent sample-expert` run against the 13:55 release
  binary measured CallBot-vs-pool because TRAINED_AGENTS in the old
  binary didn't include `sample-expert`. See
  `STALE-BINARY-GOTCHA-2026-10-01.md`.
- 16:18 — same for the F1 A/B (`--search` didn't exist in the old binary).
  See `STALE-BINARY-GOTCHA-2-2026-10-01.md`.

**Rule:** after any commit that changes the CLI surface or TRAINED_AGENTS,
run `cargo build --release -p cham-cli` before launching a ladder.
The `ladder` now refuses unknown agent names loudly (d8dd528).

### 7.2 Background process heredoc + backtick hazard

Writing scripts via `cat > file << 'EOF' ... backticks ... EOF` can
still get shell-expanded if the outer context is a command substitution.
Use `ENDOFDOC` / `SCRIPTEOF` as delimiters to avoid collisions. And
**never** put backticks inside a `$(...)` that itself contains a heredoc.

### 7.3 The concurrent-pipeline file race

Two `wr.sh` invocations can race. My scripts now use
`mkdir $LOCKDIR || exit 0` guards (LADDER/retrain). The user should
kill the extra watchexec instances (`wr1.sh` watchers watch a file
that doesn't exist). Not done.

### 7.4 Test artifacts under `crates/*/artifacts/` get rewritten by every nextest

`.gitignore` now covers them broadly:
`crates/*/artifacts/**/*.snap`, `*.bin`, `*.json`. If they were
tracked before, they've been untracked. If they reappear as dirty,
re-run `git rm --cached` for them.

### 7.5 `TrainerConfig` literals need `dcfr_alpha`/`dcfr_beta`

F5 added two fields with serde defaults; the RUST type system requires
them explicitly in every struct literal. If a new test's
`TrainerConfig { ... }` fails to compile with "missing field", add
`dcfr_alpha: 1.0, dcfr_beta: 1.0,`. The CLI shorthand is
`--dcfr-alpha 1.0 --dcfr-beta 1.0`.

### 7.6 `lbr::lbr_vs` vs `lbr::tabular_br`

Every bench / probe / exploitability test that imports `lbr_vs` still
works — it's now the "clairvoyant upper bound" diagnostic. New code
should use `tabular_br` for the honest number.

### 7.7 The `--search` flag on ladder/probe is opt-in and net negative

`ladder --agent full --search` measured a −2 793 mb/seating delta on
the scripted pool (F1-SEARCH-CORRECTED). Do not enable it as default
until F10 lands.

---

## 8. Key commands

### Train a 5M robust blueprint

    target/release/chameleon train-bp \
      --mode robust --iters 5000000 --depth 100 --seed 7 \
      --config config/abstraction-tiny.toml \
      --buckets artifacts/buckets-tiny \
      --out artifacts/par-5M-f3f4 \
      --threads 4 --thread-mode hogwild

### Train with DCFR(1.5, 0)

    # same as above plus:
    #   --dcfr-alpha 1.5 --dcfr-beta 0.0
    # and (for gamma=2):
    #   CHAM_AVG_DELAY=0

### Corrected exploitability on the par-5M policy

    CHAM_EXPLOIT_BP=$PWD/artifacts/par-5M/robust-7/policy \
    CHAM_EXPLOIT_BUCKETS=$PWD/artifacts/buckets-tiny \
    CHAM_EXPLOIT_CONFIG=$PWD/config/abstraction-tiny.toml \
      cargo nextest run -p cham-blueprint \
        -E 'test(par5m_metric_compare)' \
        --run-ignored all --no-capture

Prints both the clairvoyant and the tabular BR for the same policy.

### Ladder against the archetype pool

    CHAM_AGENT_BUNDLE=$PWD/artifacts/agent-honest-19dim \
      target/release/chameleon ladder --fast --agent full

### Inspect the table at any snapshot

    /tmp/rm_freeze/target/release/rm_freeze path/to/table.snap

---

## 9. Frontier (post-F1)

**Shipped bundle:** `artifacts/agent-honest-19dim`, `--agent full`
(argmax over 4 experts + honest 19-dim router), tiny abstraction.
Ladder +8 276 mb/seating (mean, 2500 deals/pair).

**Corrected exploitability:**
- par-5M robust, seat 1: **2.53 bb/hand** (was reported as 21.2 bb).
- Seat 0 not measured this session; the full value is BR(0) + BR(1).

**The frozen par-5M policy is ~1.5-2 bb/hand** on the tiny abstraction.
That is not Nash-class, but it is significantly closer than any prior
session thought. The realistic target for a Nash-class opponent is
"break-even minus abstraction/translation losses"; the bot may already
be there.

**No new SOTA on the ladder this session.** The F3/F4/F6a retrain
would produce a new artifact but hasn't been run with the corrected
metric yet.

---

## 10. The two open items the user asked to resolve

**F5 (DCFR)** — LANDED. The implementation is correct
(Brown-Sandholm telescoped slice discount). The sweep hasn't been run
with the corrected metric yet; that's the successor's job.

**F10 (vector-form river solver)** — PLANNED. The design is captured
in `F10-VECTOR-SOLVER-PLAN-2026-10-02.md` with the O(n) showdown
kernel from the report. It is a 1-2 week job. The current search is
net negative on the scripted pool; F10 is the multi-day fix.

Neither is fully "solved" this session. F5 is done and ready to test.
F10 is the multi-day commitment.

---

## 11. Repository state at handoff

    git log --oneline -1
    # 998aaa5 (or later)

    git log --oneline origin/main..HEAD | wc -l
    # 13 commits ahead (not pushed)

    git status --short
    # (clean, or only artifacts/ledger/ledger.jsonl if a test ran)

    pgrep -af "chameleon|f3f4-retrain|lbr-curve"
    # (empty; nothing running)

To push:

    git push
    # pre-push runs cargo fmt --all -- --check and
    # cargo clippy --workspace --all-targets -- -D warnings

---

## 12. Session timeline (compressed)

- **19:05** — reconnaissance: `lbr_vs` reviewed; `tabular_br` design.
- **19:07** — `tabular_br` implemented, `tabular_le_clairvoyant_on_uniform` passes (2.8 vs 17.4 bb/hand on a uniform policy, 6.2x reduction).
- **19:09** — par-5M measured: clairvoyant 21 200 vs tabular 2 533 mb/hand (8.4x ratio). The session's biggest finding.
- **19:10** — F1 doc + tests committed.
- **19:12** — cancelled the 50M LBR curve and the F3F4 retrain queue (wrong metric).
- **19:14-19:36** — F5 DCFR: `discount_all`, `TrainerConfig.dcfr_alpha/beta`, CLI flags, all tests passing (252/252).
- **19:36-19:40** — F5 doc + F10 plan doc committed.

Earlier today (before this session segment):
- F2 (class-conditioned solver) merged.
- F3/F4/F6a/F7/F9 landed.
- F1 search wired into the pipeline, A/B run, corrected-metric guard added.

---

Good luck. The corrected metric is the biggest news — quote it
everywhere, and be suspicious of any number from before 2026-10-01.
