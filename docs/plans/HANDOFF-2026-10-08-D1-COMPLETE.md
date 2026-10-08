# CHAMELEON — session handoff, 2026-10-08 (evening, D1 COMPLETE)

**Written:** 2026-10-08 (after landing the full-game VBR).
**Repository:** /Users/adm/Documents/Repos/chameleon, branch `main`.
**HEAD:** ~85 commits ahead of `origin/main` (unpushed).

## 0. Read this first

1. `docs/plans/D1-RESULT-2026-10-08.md` — **Decision D1 is ANSWERED**.
2. `docs/plans/FULLGAME-VBR-DESIGN-2026-10-08.md` — how the walker works.
3. `docs/plans/PHASE-C-PCS-DESIGN-2026-10-08.md` — Phase C design (next task).
4. This file — where we are, gotchas, next task.

**No single authoritative plan file exists.** The session-start handoff
cited `CHAMELEON-SOTA-PLAN.md`, but that file is not in the working tree
and has no git history (`git log --all -- '*CHAMELEON-SOTA-PLAN*'` is
empty). Decisions are made in the per-phase design docs listed above.

## 1. HEADLINE: Decision D1 is answered

**Full-game VBR vs the shipped blueprint: 6.41 ± 1.50 bb/hand**
(20 boards, 30h/30v combos, 0.49% policy miss, agent-honest-19dim).

- River-only VBR on rlf-* policies: 5.89 bb.
- Tabular in-abstraction BR: 8.19 (tiny) / 10.56 (rlf-12m) bb.

Interpretation:
- Earlier streets add ~0.5 bb over the river-only slice (within 1 SE of
  zero). The river dominates the blueprint's exploitable surface.
- The tabular BR (8.19–10.56) was a **metric artifact** (C-2 hazard
  confirmed by the honest ruler, not by assertion).
- The blueprint IS exploitable at ~6.4 bb against a perfect BR. The
  earlier "~0 exploitable" claims were under-converged-learner artifacts
  (F-5).
- Phase C (PCS trainer) is mandatory. D2's baseline to beat is 6.41.

Ledger entry: `artifacts/ledger/ledger.jsonl`, `type:"d1-vbr"`, ts
1791463156. Note `artifact_hash` is sha256, not blake3 — b3sum was not
installed; the notes field says so.

## 2. What landed this session (commits oldest first)

- `fc414b3` feat(search): PublicTree::build_with_deck + card-independence
- `79d5363` test(search): pubtree_card_independent
- `ad1f704` docs: FULLGAME-VBR-DESIGN-2026-10-08.md
- `daff793` feat(search): full-game VBR walker
- `57735d3` chore: clippy clean vbr_validate
- `619fba1` test(search): full-game VBR smoke
- `1805406` test(search): diagnostic — PublicTree size
- `da077b6` test(search): diagnostic — PublicTree dump + terminal tally
- `139134e` feat(search): PublicTree::build_from_state + river diag
- `09a153d` test(search): brute-force validation of VBR walker
- `53053f6` **fix(search): VBR fold winner from last action** (real bug)
- `c312244` refactor: policy callback takes (&State, &ActionSeq, na, combo)
- `e8c119c` refactor: callback also receives action path
- `d2f950f` chore(agent): clippy clean d1_vbr_blueprint
- `7dc522a` test(agent): D1 full-game VBR harness
- `69af8b9` test(agent): d1_key_probe
- `45317ec` docs(agent): D1 run command (drop CHAM_SLOT_BUCKET)
- `23057e0` docs(plans): D1 result
- `21c705e` chore(ledger): record D1

## 3. Where we are in the plan

| Phase | Task | Status |
|---|---|---|
| A | rule-4 env->config gates | DONE |
| A | search default OFF | DONE |
| A | sparring partner (seed 11, 20M) | possibly still TRAINING |
| B | T1.1 W1 kernels | DONE |
| B | T1.2/T1.3 river VBR | DONE |
| B | T1.4 D1 harness | **DONE — river AND full-game** |
| B | **Decision D1 (honest number)** | **ANSWERED: 6.41 bb** |
| C | PCS trainer | **NOT STARTED — next substantive work** |
| C | W2 key v2 / abstraction v3 | PARTIAL |
| D | W3 combo solver | NOT STARTED |

## 4. The full-game VBR walker (crates/cham-search/src/fullgame.rs)

- Drives the shared `PublicTree` from a `State` seeded with a sampled
  board.
- Hero: max over actions per combo.
- Villain: reach split by the policy callback
  `FnMut(&State, &[Action], &ActionSeq, usize /*na*/, usize /*combo*/)
   -> Vec<f64>`.
- Terminal: showdown via `showdown_cfv_two`; fold via the recorded last
  action (NOT stacks — see gotcha G1).
- Card removal handled exactly by the kernel + the two-range disjoint
  mass.

## 5. In-flight / open

- Nothing uncommitted. Working tree is clean except the six unexplained
  `docs/plans/HANDOFF-*.md` deletions (see §6).
- `docs/plans/HANDOFF-2026-10-08.md` in particular was deleted in the
  working tree before this session. Its deletion was never committed.
  Whoever picks this up should decide: restore those docs or commit the
  deletions as a housekeeping change. They were not touched by this
  session's work.

## 6. Gotchas learned this session

- **G1: fold winner from last action, not stacks.** `State::stacks()` at
  a fold terminal do not reflect pot resolution — both stacks read 9950
  at a preflop fold with pot 100, so a `hero_net > 0` test always picks
  the same loser. The walker now records `(last_action, last_actor)`
  through the recursion. Regression test: `fullgame_fold.rs`.
- **G2: `ActionSeq` is `Copy`.** Clippy `clone_on_copy` fires if you
  `.clone()` it; just pass by value.
- **G3: the shipped bundle is abstraction v2.** At v2, `CHAM_SLOT_BUCKET`
  and `CHAM_COMPRESS_HISTORY` are env-flag gated and change KEYS. The
  bundle `agent-honest-19dim` was trained with STACK-FRACTION sizes, so
  setting `CHAM_SLOT_BUCKET=1` corrupts every non-root seq entry and
  produces a 99.48% policy miss. Do NOT set it.
- **G4: `lint-ledger` requires `--prereg <TOML>`.** It's a
  pre-registration gate, not a general ledger parser. Don't invoke it
  without a prereg file.
- **G5: heredoc truncation is real.** Two long heredocs in this session
  got cut mid-write and produced a `[Command was successful]` from the
  outer runner while leaving a half-written file. Always `wc -l` and
  `grep -xq 'EOF'` after a big write.
- **G6: `git add artifacts/...` warns but succeeds** (artifacts is
  gitignored). The ledger is intentionally tracked despite the ignore
  (handoff says so). No `-f` needed — the warning is cosmetic.
- **G7: `blake3` is not a dependency of cham-eval.** Compute hashes in
  shell (b3sum / openssl) and pass via env for one-shot ledger writes.

## 7. Next tasks, in order

1. **Decide the six HANDOFF-*.md deletions** (§5). Restore or commit.
2. **Phase C: PCS trainer** on the tiny abstraction. Decision D2:
   PCS ≤ 4h wall beats the shipped full-game VBR by > 3 SE. Baseline to
   beat: **6.41 bb**. Uses the PublicTree — the same skeleton the
   walker uses.
3. **Optional: tighten D1's SE.** Current SE=1.50 at 20 boards. ~180
   boards (≈10 min wall clock) gives SE≈0.5. Useful before Phase C
   uses 6.41 as a hard baseline.
4. **W2:** 128/64 buckets (queue building them), key v2, abstraction v3.
5. **W3:** combo-level river->turn solver + gadget + live integration.

## 8. Key commands

State check:
    git log --oneline -1; git log --oneline origin/main..HEAD | wc -l
    df -h . | tail -1
    pgrep -af "train-bp|d1_vbr|train-buckets|driver.sh|insight10h" | grep -v grep

D1 full-game VBR (correct env — do NOT set CHAM_SLOT_BUCKET):
    CHAM_D1_BP=$PWD/artifacts/agent-honest-19dim/robust \
    CHAM_D1_CONFIG=$PWD/artifacts/agent-honest-19dim/abstraction.toml \
    CHAM_D1_BUCKETS=$PWD/artifacts/agent-honest-19dim/buckets \
      target/debug/deps/d1_fullgame_vbr-<hash> --ignored --nocapture

Run a cham-search test binary directly (never cargo nextest for long tests;
another repo's `pkill -f nextest` killed four of our runs):
    cargo test -p cham-search --test <name> --no-run
    target/debug/deps/<name>-<hash> --nocapture [--ignored]

## 9. Repository state

- Branch `main`, ~85 commits ahead of `origin/main`, unpushed.
- fmt + clippy clean per pre-push hook at last check.
- Ledger: `artifacts/ledger/ledger.jsonl` (append-only, committed via
  `git add`; the artifacts/ dir is gitignored but this file is tracked).

## 10. The honest bottom line

D1 is answered. The blueprint is exploitable at **6.41 ± 1.50 bb/hand**
against an exact card-perfect full-game best response. The gap between
this number and the river-only 5.89 bb is within one SE — the exploit-
ability is essentially river-dominated. The tabular BR (8.19–10.56) was
the metric artifact the plan predicted.

The next agent's job is Phase C: a real trainer (PCS DCFR) on the tiny
abstraction, targeting a full-game VBR clearly below 6.41.
