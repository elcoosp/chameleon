# CHAMELEON — Deep-Dive Fix Runbook (implement everything, in order)

> **Audience:** an implementing agent with repo write access, no judgment
> calls required. Follow the steps in order — each item states the exact
> file, the exact text to find, the exact text to replace it with, the
> exact commands to run, and the exact pass condition. Do not skip the
> "verify" step of any item before moving to the next. If a "find" string
> doesn't match exactly (whitespace, prior edits), stop and diff the file
> against what's quoted here before proceeding — don't guess.
>
> Items 1–4 are one PR: zero behavior change, bit-exact, no `EXP-0NN`
> needed. Items 5–9 are separate PRs/sessions each, in the stated order.

---

## Item 1 — Fix `docs/HANDOFF.md` staleness

**Type:** doc only. **Risk:** none. **Time:** 10 min.

### Step 1.1 — open the file
```bash
sed -n '1,40p' docs/HANDOFF.md
```

### Step 1.2 — find this block (the stale P1 section)
```markdown
### P1 — Get the full-abstraction agent to eliminate fallback

The full-abstraction ladder from this session produced **26.7% fallback**
decisions (vs 0% for the tiny abstraction the previous day). This means
the router is allocating weight poorly at the fuller scale: the mixture
collapses to something the confidence gate rejects, and the numbers
below are therefore "diagnostic only."

**What to do:** this is the V2 Phase 2 (`Cold-start mixture LBR`) or
Phase 7 (`Ensemble-disagreement shield`) job. Neither has started. The
immediate question is: which of the 4 experts is falling below the
confidence gate, and on which opponents?

**Files:**
- `crates/cham-agent/src/pipeline.rs` — the mixture + fallback path
- `crates/cham-agent/src/tracker.rs` — the feature source feeding the router
- `crates/cham-cli/src/cmd/probe.rs` — the diagnostic harness (add a
  per-expert fallback report here)

**Acceptance:** rerun `scripts/run-full-agent.sh` after a fix and see
fallback < 5% (the tiny agent achieves 0%; a full agent should not be
worse than the tiny on this metric).
```

### Step 1.3 — replace it with
```markdown
### P1 — [RESOLVED this cycle] Full-abstraction fallback

Was 26.7% (full) / 17.3% (tiny). Root cause diagnosed and fixed:
`docs/reports/p1-fallback-diagnosis-20260926.md` found two distinct
classes (Cause A: training-reachability gap on `jamfix`/`pnash`; Cause B:
fallback-order over-report). EXP-012 (R3: report `fallback_used` from the
decision path) and EXP-013 (R2: drop-missed-tier + renormalize instead of
substitute-with-robust) fixed Cause B — **full-abstraction fallback is now
3.3%** (was 26.7%), meeting the < 5% acceptance bar. EXP-014 (widened
training curriculum) closes most of remaining Cause A at tiny scale but
regressed arch-opponent coverage at full scale (3.3% → 3.7%) — see
`docs/reports/v5-deepdive-audit.md` §B for the recommended follow-up
(re-run EXP-014 at full scale with higher `iters` before concluding
widening doesn't work at scale — capacity, not coverage, is the open
question). Current status and every other item's state: `docs/BOARD.md`
is the canonical source — read it, not this section, for anything not
covered above.

**Files that changed:** `crates/cham-agent/src/pipeline.rs`
(`AgentMode.fallback_mode`, mixture composition), `crates/cham-agent/src/modes.rs`,
`crates/cham-cli/src/cmd/probe.rs` (`--diag-fallback --bundle`).
```

### Step 1.4 — verify
```bash
grep -n "26.7%" docs/HANDOFF.md   # should print nothing
git diff docs/HANDOFF.md          # eyeball the diff
```

### Pass condition
`grep -c "26.7%" docs/HANDOFF.md` returns `0`. Commit with message
`docs: sync HANDOFF.md P1 status with BOARD.md (fallback fixed this cycle)`.

---

## Item 2 — `pipeline.rs`: stop recomputing expert/robust strategy for the reach update

**Type:** perf, bit-exact (same output, less work). **Risk:** none if the
find-string matches exactly. **Time:** ~1 hr incl. test.

### Step 2.1 — open the file and locate the block
```bash
grep -n "per-expert reach update" crates/cham-agent/src/pipeline.rs
```
This should point near the reach-update loop, shortly before
`encoder.record(obs, obs.player, action, seq);`.

### Step 2.2 — find this exact block
```rust
        // per-expert reach update: π_k *= σ_k(a_chosen | i)
        let chosen_slot = slots.iter().position(|s| s.action == action).unwrap_or(0);
        for k in 0..4 {
            if w[k] <= 1e-9 {
                continue;
            }
            let sigma = match experts[k].strategy(obs, encoder, seq) {
                Some(s) => s,
                None => continue,
            };
            let p = sigma.get(chosen_slot).copied().unwrap_or(0.0);
            reach[k] *= p;
        }
        let robust_p = robust
            .strategy(obs, encoder, seq)
            .and_then(|s| s.get(chosen_slot).copied())
            .unwrap_or(0.0);
        reach[4] *= robust_p;
```

### Step 2.3 — replace with
```rust
        // per-expert reach update: π_k *= σ_k(a_chosen | i)
        // PERF (v5-deepdive-audit item 1): reuse expert_sigma/robust_sigma
        // computed above instead of recomputing — obs/encoder/seq are
        // unchanged since those were computed (encoder.record happens
        // below, after this block), so the values are byte-identical;
        // this removes 5 redundant enc.key() + policy-decode + Vec<f64>
        // allocations per decision.
        let chosen_slot = slots.iter().position(|s| s.action == action).unwrap_or(0);
        for k in 0..4 {
            if w[k] <= 1e-9 {
                continue;
            }
            if let Some(sigma) = expert_sigma[k].as_ref() {
                reach[k] *= sigma.get(chosen_slot).copied().unwrap_or(0.0);
            }
        }
        if let Some(sigma) = robust_sigma.as_ref() {
            reach[4] *= sigma.get(chosen_slot).copied().unwrap_or(0.0);
        }
```

### Step 2.4 — add a bit-exact regression check

Open `crates/cham-agent/src/pipeline.rs` (or the crate's `tests/` dir if
one exists for this module — check first):
```bash
ls crates/cham-agent/tests/ 2>/dev/null
grep -rn "mod tests" crates/cham-agent/src/pipeline.rs
```

If `pipeline.rs` already has a `#[cfg(test)] mod tests { ... }` block, add
a test inside it that drives a small fixed-seed match and asserts the
chosen actions / `last_trace().weights_frozen` match a golden run captured
before this change. If writing that harness is more than this pass should
take on, use the cheaper but equally conclusive black-box check instead:

```bash
# BEFORE the edit (steps 2.2-2.3 not yet applied):
cargo run -q -p cham-cli -- probe --diag-fallback --bundle artifacts/agent --agent full > /tmp/before.json
# apply steps 2.2-2.3, then:
cargo run -q -p cham-cli -- probe --diag-fallback --bundle artifacts/agent --agent full > /tmp/after.json
diff /tmp/before.json /tmp/after.json   # MUST be empty — same seed, same math, only less work
```

### Step 2.5 — measure the win
```bash
cargo bench -p cham-agent --bench decision -- --save-baseline before-reach-fix
# (apply the edit if not already applied)
cargo bench -p cham-agent --bench decision -- --baseline before-reach-fix
```

### Step 2.6 — full verification
```bash
cargo test -p cham-agent
cargo clippy -p cham-agent --all-targets -- -D warnings
diff /tmp/before.json /tmp/after.json   # from step 2.4, must be empty
```

### Pass condition
`cargo test -p cham-agent` green, clippy clean, `/tmp/before.json` and
`/tmp/after.json` byte-identical, and the criterion bench report shows a
reduction in per-decision time (record the number in the worklog —
`## Reach-recompute fix — measured` with the before/after ns/decision).

---

## Item 3 — `pipeline.rs`: drop the duplicate `encoder.slots()` call

**Type:** perf cleanup. **Risk:** none. **Time:** 5 min. Do this in the
**same PR** as Item 2 (same file, same function, trivial to bundle).

### Step 3.1 — find
```rust
        let n_slots_at_decision = encoder.slots(obs, seq).len();
        let slots = encoder.slots(obs, seq);
        let n = n_slots_at_decision.max(slots.len());
```

### Step 3.2 — replace with
```rust
        let slots = encoder.slots(obs, seq);
        let n = slots.len();
```

### Step 3.3 — verify
```bash
cargo build -p cham-agent 2>&1 | grep -i "error\|warning"
cargo test -p cham-agent
```
(No behavior change is possible here — `n_slots_at_decision` and
`slots.len()` were provably always equal, same `obs`/`seq` both calls.)

### Pass condition
Builds clean, `cargo test -p cham-agent` green, same before/after `probe`
diff check as Item 2 stays empty.

---

## Item 4 — `cache.rs`: fix the `touch()` mutex-hold-through-if-let bug

**Type:** perf/concurrency, bit-exact (LRU order is an optimization detail,
never a correctness input — `solve_cached_equals_fresh` is the real
correctness gate and this change doesn't touch it). **Risk:** none.
**Time:** 15 min.

### Step 4.1 — find (in `crates/cham-search/src/cache.rs`, function `cached_build`)
```rust
    let cache = global();
    if let Some(hit) = cache.map.lock().expect("cache").get(&key) {
        cache.hits.fetch_add(1, Ordering::Relaxed);
        let hit = Arc::clone(hit);
        touch(key);
        return Ok(hit);
    }
    cache.misses.fetch_add(1, Ordering::Relaxed);
```

### Step 4.2 — replace with
```rust
    let cache = global();
    // PERF (v5-deepdive-audit item 3): the previous `if let Some(hit) =
    // cache.map.lock()...get(&key) { ... touch(key); ... }` form holds the
    // `map` mutex's temporary guard for the WHOLE if-let block (Rust's
    // temporary-scope rule for `if let` scrutinees), so every cache HIT
    // held the process-global map lock through touch()'s O(n) reorder on
    // a second mutex. Scope the guard explicitly and drop it before
    // touch() so concurrent readers (now parallel via AbRunner, v3 §1.1)
    // aren't serialized on an unrelated bookkeeping step.
    let hit_opt: Option<Arc<Subgame>> = {
        let map = cache.map.lock().expect("cache");
        map.get(&key).cloned()
    }; // map guard dropped HERE, before touch()
    if let Some(hit) = hit_opt {
        cache.hits.fetch_add(1, Ordering::Relaxed);
        touch(key);
        return Ok(hit);
    }
    cache.misses.fetch_add(1, Ordering::Relaxed);
```

### Step 4.3 — verify
```bash
cargo test -p cham-search
cargo clippy -p cham-search --all-targets -- -D warnings
```

### Step 4.4 — (optional but recommended) add the clippy lint that would have caught this, scoped to this crate only
Check whether `crates/cham-search/src/lib.rs` has a `#![warn(...)]` /
`#![deny(...)]` block:
```bash
sed -n '1,20p' crates/cham-search/src/lib.rs
```
If there's an existing lint block, add `clippy::significant_drop_in_scrutinee`
to it at `warn` level (not `deny` — this lint has false positives
elsewhere in the workspace; scope it to this crate only, don't add it to
the workspace-level lint table without auditing every other crate first):
```rust
#![warn(clippy::significant_drop_in_scrutinee)]
```
Run `cargo clippy -p cham-search --all-targets` and fix anything else it
flags in this crate only, or leave a `// TODO` comment if a flagged
instance turns out to be intentional — do not silently `#[allow]` it
without a one-line justification comment.

### Step 4.5 — benchmark under contention (confirms the fix actually helps, informs whether Item 9 is worth doing)
```bash
cargo bench -p cham-search --bench trigger_cache -- --save-baseline before-touch-fix
# (revert step 4.2 temporarily via git stash, or bench on the commit before this change)
git stash
cargo bench -p cham-search --bench trigger_cache -- --baseline before-touch-fix
git stash pop
```
If `crates/cham-search/benches/trigger_cache.rs` doesn't already have a
multi-threaded contention scenario (check with
`grep -n "thread\|spawn\|scope" crates/cham-search/benches/trigger_cache.rs`),
note that as a gap for Item 9 rather than adding one here — this item's
scope is the correctness/lock-hold fix, not new benchmark infrastructure.

### Pass condition
`cargo test -p cham-search` green, clippy clean (including the new
crate-scoped lint), no regression in single-threaded
`trigger_cache` numbers (this change should be neutral-to-positive
single-threaded and strictly positive under concurrent load).

---

## Item 5 — Wire the EXP-016 shadow gauntlet into `--promote`

**Type:** competitiveness/safety, behavior change (adds a gate). **Risk:**
low but real — this can now BLOCK a promotion that would previously have
gone through, which is the intent, so confirm with a dry run before
enforcing. **Time:** ~half day.

### Step 5.1 — locate the current promote path
```bash
grep -n "promote" crates/cham-cli/src/cmd/ab.rs | head -20
```

### Step 5.2 — locate the shadow gauntlet entry points
```bash
grep -n "pub fn" crates/cham-cli/src/cmd/shadow.rs
```
You're looking for something in the shape of `snapshot_champion` and
`run_gauntlet`. Confirm the actual shipped signatures by reading the file
directly rather than assuming:
```bash
sed -n '1,120p' crates/cham-cli/src/cmd/shadow.rs
```

### Step 5.3 — add the gate call before promotion commits

In `crates/cham-cli/src/cmd/ab.rs`, find where `--promote` currently takes
effect (likely something that writes to `artifacts/ledger/ledger.jsonl`
with `promote: true` and/or copies the winning bundle into the canonical
`artifacts/agent/` path). Before that write/copy happens, insert:

```rust
// EXP-016 gate (v5-deepdive-audit item 5): a promotion candidate must not
// regress against the last N shadow snapshots before it's allowed to
// promote. This does not replace the primary EXP-001-style CI-lower-bound
// check already gating `--promote` above — it's an ADDITIONAL check
// against recent history, not a substitute for the current experiment's
// own pass/fail.
if promote_requested {
    match crate::cmd::shadow::run_gauntlet(&candidate_agent_id, "artifacts/shadow", 10_000) {
        Ok(report) if report.all_within_tolerance(-10.0) => {
            println!("shadow gauntlet: PASS ({} shadows, all >= -10.0 mb/seating)", report.n_shadows);
        }
        Ok(report) => {
            eprintln!(
                "shadow gauntlet: FAIL — candidate regresses against a recent shadow (worst: {:+.1} mb/seating). Promotion BLOCKED.",
                report.worst_delta_mb
            );
            return crate::cmd::EXIT_FAIL;
        }
        Err(e) => {
            // No shadows yet (first-ever promotion) is not a failure —
            // there's nothing to regress against. Any other error IS.
            if e.contains("no shadow snapshots") {
                println!("shadow gauntlet: no prior shadows — first promotion, skipping gate.");
            } else {
                eprintln!("shadow gauntlet: error ({e}) — treating as a hard stop, not a silent pass.");
                return crate::cmd::EXIT_FAIL;
            }
        }
    }
}
```

**Note for the implementing agent:** the exact field names
(`report.all_within_tolerance`, `report.worst_delta_mb`, `report.n_shadows`)
are illustrative of the shape needed, not guaranteed to match
`shadow.rs`'s actual return type verbatim — **read `shadow.rs`'s actual
`run_gauntlet` return type first** (Step 5.2) and adapt the field/method
names to match exactly what's there. Do not invent fields; if the current
`run_gauntlet` doesn't return enough structure to check "all shadows within
tolerance," that's a prerequisite change to `shadow.rs` itself: extend its
return type to carry a `Vec<(shadow_id, delta_mb, ci_lower)>` or equivalent
before wiring the gate, rather than working around a thin return type in
`ab.rs`.

### Step 5.4 — snapshot a champion AFTER a successful promotion

Immediately after the gate passes and promotion completes, snapshot the
newly-promoted champion so the *next* promotion has it to compare against:
```rust
if let Err(e) = crate::cmd::shadow::snapshot_champion(&winning_policy_dir, "artifacts/shadow") {
    eprintln!("shadow snapshot: warning, failed to snapshot new champion ({e}) — next gauntlet run will be missing this baseline");
    // non-fatal: the promotion itself already succeeded; don't roll it back over a snapshot failure
}
```

### Step 5.5 — dry run before enforcing

Before merging, run once against the current `artifacts/agent/` bundle
with `--promote` on a real (or synthetic small) `ab` run to confirm the
gate doesn't false-fail on the very first invocation (no shadows yet should
skip cleanly per Step 5.3's `Err(e)` branch):
```bash
cargo run -q -p cham-cli -- ab full robust-only --deals 4 --promote
```
Expect: `"shadow gauntlet: no prior shadows — first promotion, skipping gate."`
followed by normal promotion output. If instead it errors or blocks, the
`Err(e)` string-matching in Step 5.3 doesn't match `shadow.rs`'s actual
error text — fix the match arm to check the real error variant/string.

### Step 5.6 — full verification
```bash
cargo test -p cham-cli -p cham-eval
cargo clippy -p cham-cli --all-targets -- -D warnings
cargo run -q -p cham-cli -- ab full robust-only --deals 4 --promote   # first promotion: should skip gate, succeed
cargo run -q -p cham-cli -- ab full robust-only --deals 4 --promote   # second promotion: should run the gauntlet against the first
```

### Pass condition
Both dry runs complete without spurious failures; the second run visibly
prints a gauntlet PASS/FAIL line; tests and clippy green. Update
`docs/BOARD.md`'s `TODO — next candidates` line for EXP-016 from *"shadow
gauntlet wiring to `--promote` (report → gate)"* to `[DONE]` under the DONE
section.

---

## Item 6 — Deploy the EMD bucket rebuild (full GPU bulk-fill)

**Type:** competitiveness, the single biggest lever currently available.
**Risk:** medium (multi-hour GPU job, real compute cost) but low
correctness risk — the CPU-validation-scale version already passed its
own bit-exactness bar this cycle (EXP-017's `exact` profile). **Time:**
multi-day, mostly GPU wall-clock, not engineering effort — this is largely
already spec'd; this item is the runbook to execute it, not new design.

### Step 6.1 — confirm the spec and current state
```bash
cat docs/plans/gpu-jobs-v3.md
grep -n "WIP\|EMD bucket rebuild" docs/BOARD.md
```
Confirm the `IN PROGRESS` entry for "EMD bucket rebuild validation" in
`docs/BOARD.md` — if it's already moved to DONE by the time you read this,
**stop, this item is already complete**, skip to Item 7.

### Step 6.2 — run the full-orbit GPU bulk-fill (job 1 from `gpu-jobs-v3.md`)
```bash
cargo run -q -p cham-gpu --bin gpu-build --features metal -- --help   # confirm real flags first
cargo run -q -p cham-gpu --bin gpu-build --features metal -- \
    --kind ehs-histogram --profile exact --street flop --out artifacts/gpu-tables/flop-histo-exact.bin
cargo run -q -p cham-gpu --bin gpu-build --features metal -- \
    --kind ehs-histogram --profile exact --street turn --out artifacts/gpu-tables/turn-histo-exact.bin
```
(Use the real flags printed by `--help` — the invocation above is the
shape per `gpu-jobs-v3.md`'s job description, not a guaranteed-verbatim
CLI surface.)

### Step 6.3 — bit-exact validate against the CPU `exact` profile
```bash
cargo run -q -p cham-cli -- verify --gpu --check-histogram-tables artifacts/gpu-tables/
```
This must reuse the same P7-style bit-exact comparison pattern already
established for the turn/flop EHS scalar tables — **do not skip this
step even though the GPU track's raw eval kernel already cleared this bar
once; a new kernel (histogram, not scalar EHS) is a new correctness
surface.**

### Step 6.4 — rebuild buckets from the validated GPU tables
```bash
cargo run -q -p cham-cli -- train-buckets --profile exact \
    --histo-source artifacts/gpu-tables/ \
    --out artifacts/buckets-exact-full
```

### Step 6.5 — retrain and re-measure
```bash
cargo run -q -p cham-cli -- train-bp --buckets artifacts/buckets-exact-full --out artifacts/agent-exact-full
cargo bench -p cham-blueprint --bench exploitability -- --save-baseline pre-emd-full
# (switch bundle config to point at artifacts/agent-exact-full, per how EXP-017's
#  earlier exact-vs-tiny comparison was run)
cargo bench -p cham-blueprint --bench exploitability -- --baseline pre-emd-full
cargo run -q -p cham-cli -- audit-buckets --generate --bundle artifacts/agent-exact-full --pool config/pool.toml
```

### Step 6.6 — gate check (from the original roadmap, unchanged)
Compare the new `audit-buckets` ratio and the exploitability bench delta
against the CPU-validation-scale numbers already on record (ratio 0.24 →
0.31, +29%, per EXP-017): the full-orbit GPU version should show a ratio at
least as good, likely better (more orbits = less sampling noise in the
histogram estimate). If exploitability doesn't improve ≥10% end-to-end
despite the bucket-quality ratio improving, that's the same kill signal
the original A2 spec already defined — stop and report, don't force a
promotion.

### Step 6.7 — full verification and promotion path
```bash
cargo test --workspace
cargo run -q -p cham-cli -- ab full full --deals 25000 --a-bundle artifacts/agent-exact-full --promote
```
(This promotion now goes through Item 5's shadow gauntlet gate — expect it
to run and report.)

### Pass condition
`verify --gpu` bit-exact check passes; exploitability bench shows ≥10%
improvement; `ab` gate (including the new shadow gauntlet) passes; update
`docs/BOARD.md`'s IN PROGRESS entry to DONE with the final measured
numbers, matching the style of the EXP-012..019 entries already there.

---

## Item 7 — Re-run EXP-014 at full scale with higher `iters` (isolate capacity vs. coverage)

**Type:** competitiveness, measurement. **Risk:** none (diagnostic only,
no promotion). **Time:** ~1 day, mostly training wall-clock.

### Step 7.1 — confirm current EXP-014 full-scale iters setting
```bash
cat scripts/exp-014-run.sh
grep -n "iters" config/agents/full.toml config/training/rotation-widened.toml
```

### Step 7.2 — retrain the widened-full bundle at a higher iteration count

Pick an iteration count materially higher than whatever Step 7.1 found
(e.g. 4–8×; the exact multiplier isn't load-bearing, the point is to
remove "under-trained" as a confound):
```bash
cargo run -q -p cham-cli -- train-bp \
    --config config/agents/full.toml \
    --buckets artifacts/buckets-full \
    --opponent-rotation config/training/rotation-widened.toml \
    --iters <HIGHER_N> \
    --out artifacts/agent-widened-full-hi-iters \
    --thread-mode deterministic
```
(Flag names — `--opponent-rotation`, `--iters` — must match whatever
`scripts/exp-014-run.sh` actually used; read that script first rather than
guessing the flags, since it's the known-working invocation from this
cycle.)

### Step 7.3 — re-run the fallback diagnosis on the new bundle
```bash
DIAG_DEALS=60 cargo run -q -p cham-cli -- probe --diag-fallback \
    --bundle artifacts/agent-widened-full-hi-iters --agent full
```

### Step 7.4 — compare against both prior full-scale results

You now have three data points to compare (all at full abstraction scale):
1. `artifacts/agent-full` (post-EXP-013 renorm, pre-widening): fallback 3.3%
2. `artifacts/agent-widened-full` (EXP-014 at original iters): fallback 3.7%
3. `artifacts/agent-widened-full-hi-iters` (this run): fallback ?%

Record all three side by side, per-opponent, same shape as the existing
`exp-014-widened-full` ledger entry (`artifacts/ledger/ledger.jsonl`).

### Step 7.5 — append the result to the ledger
Follow the exact JSON-lines shape already used by the `exp-014-*` entries
in `artifacts/ledger/ledger.jsonl` (copy the schema from the
`exp-014-widened-full` line, change `run`, `notes`, and the measured
numbers). Append, don't overwrite:
```bash
cat >> artifacts/ledger/ledger.jsonl << 'EOF'
{"run": "exp-014-widened-full-hi-iters", "type": "probe", "a": {"agent": "full", "bundle": "artifacts/agent-widened-full-hi-iters"}, "b": {"agent": "full", "bundle": "artifacts/agent-widened-full"}, "delta_mb": null, "ci": null, "sprt": null, "promote": false, "seatings": <N>, "notes": "EXP-014 follow-up (v5-deepdive-audit item 7): isolate capacity vs coverage by retraining the widened-full bundle at <HIGHER_N> iters (was <ORIGINAL_N>). Fallback: <RESULT>%. If this recovers toward 3.3% (the pre-widening full baseline) while KEEPING jamfix/pnash fixed, the EXP-014 full-scale regression was a capacity/under-training artifact, not a fundamental coverage tradeoff -- higher iters is the fix, not a 5th specialist. If it does NOT recover, the regression is a genuine per-specialist capacity ceiling and a 5th jamfix/pnash-focused specialist is the next step.", "ts": <UNIX_TS>}
EOF
```

### Step 7.6 — verify and decide
```bash
cargo run -q -p cham-cli -- lint-ledger --entry exp-014-widened-full-hi-iters
```

### Pass condition
`lint-ledger` accepts the new entry; `docs/BOARD.md` gets a one-line update
under the EXP-014 entry recording the verdict (capacity-confirmed vs.
capacity-ceiling) and, if capacity-ceiling, a new `[TODO]` line for "5th
specialist for jamfix/pnash-shaped deviations."

---

## Item 8 — Run the EXP-015 60-cell router-manipulation grid

**Type:** competitiveness, diagnostic (no promotion). **Risk:** none.
**Time:** ~1 day compute, hooks already shipped.

### Step 8.1 — confirm the CLI flags exist as expected
```bash
cargo run -q -p cham-cli -- self-exploit --help
```
Confirm `--switch-at`, `--router-temp`, `--router-n0` (or whatever the
actual flag names are — use what `--help` prints) are present, per
`docs/BOARD.md`'s EXP-015 DONE entry: *"self-exploit --switch-at/
--router-temp/--router-n0, build_chameleon_with_router"*.

### Step 8.2 — run the grid
```bash
mkdir -p artifacts/exp-015-grid
for switch in 10 20 40 80 150; do
  for n0 in 4 8 16 32; do
    for temp in 0.5 0.7 1.0; do
      out="artifacts/exp-015-grid/switch${switch}-n0${n0}-temp${temp}.json"
      cargo run -q -p cham-cli -- self-exploit \
        artifacts/agent/robust artifacts/buckets-tiny config/abstraction-tiny.toml \
        --deals 2000 --switch-at "$switch" --router-n0 "$n0" --router-temp "$temp" \
        > "$out" 2>&1
      echo "done: switch=$switch n0=$n0 temp=$temp -> $out"
    done
  done
done
```
(If `self-exploit`'s positional/flag argument order in `--help` differs
from the invocation above, fix the command to match.)

### Step 8.3 — aggregate results into a heatmap table

Write a small script that extracts the adaptive manipulator earn rate from
each `artifacts/exp-015-grid/*.json` and produces a `switch_at × (N0, temp)`
table:
```bash
python3 - << 'EOF'
import glob, re
rows = []
for f in glob.glob("artifacts/exp-015-grid/*.json"):
    m = re.search(r"switch(\d+)-n0(\d+)-temp([\d.]+)", f)
    switch, n0, temp = m.groups()
    with open(f) as fh:
        data = fh.read()
    for line in data.splitlines():
        if "manipulator earns" in line:
            rows.append((switch, n0, temp, line.strip()))
for r in sorted(rows):
    print(r)
EOF
```
(Grep `self_exploit.rs`'s source for the exact print format string —
`"self-exploit adaptive: manipulator earns {:+.1} ± {:.1} mb/seating vs
live full..."` — before relying on this parser; adjust the match string if
it differs.)

### Step 8.4 — write the report
Create `docs/reports/exp-015-router-manipulation-grid.md` with the full
heatmap table and a verdict following exactly the rule already
pre-registered in `experiments/PREREG-EXP-015.toml`:
```bash
cat experiments/PREREG-EXP-015.toml   # re-read the pre-registered follow-up rule before writing the verdict
```
Do not promote any single winning hyperparameter cell without also running
it through a standard `EXP-001`-style ladder check — the pre-registration
file's `follow_up` field says exactly this; follow it literally.

### Step 8.5 — verify
```bash
cargo run -q -p cham-cli -- lint-ledger --entry exp-015-router-manipulation-grid
```

### Pass condition
All 60 cells produce a result (or a clearly logged failure per cell, not a
silent gap); report committed; `docs/BOARD.md`'s EXP-015 TODO line updated
to DONE with a one-line summary of the verdict (dominant cell found /
no cell beats default / default confirmed near-optimal).

---

## Item 9 — O(1) LRU for the river-subgame cache (conditional)

**Type:** perf, conditional on Item 4's Step 4.5 benchmark showing real
contention. **Risk:** low-medium (touches a shared data structure under
concurrency — needs careful testing). **Time:** ~1 day.

**Do not start this item unless Step 4.5's multi-threaded benchmark showed
a measurable regression from `touch()`'s O(n) cost under realistic `ab`
concurrency.** If the benchmark showed no measurable difference, close this
item as `[SKIP — Item 4's fix was sufficient, O(n) cost not measurable
under realistic load]` in `docs/BOARD.md` and stop here.

### Step 9.1 — if proceeding, the target design

Replace the `Mutex<VecDeque<u64>>` LRU order with a monotonic-tick
approach (lower-dependency-risk than pulling in an LRU crate, since extra
deps are explicitly NOT whitelisted per `docs/BOARD.md`'s note on B-9):

```rust
// crates/cham-search/src/cache.rs
struct Cache {
    map: Mutex<HashMap<u64, (Arc<Subgame>, u64 /* last-touched tick */)>>,
    tick: AtomicU64,
    hits: AtomicU64,
    misses: AtomicU64,
}

fn global() -> &'static Cache {
    static GLOBAL: OnceLock<Cache> = OnceLock::new();
    GLOBAL.get_or_init(|| Cache {
        map: Mutex::new(HashMap::new()),
        tick: AtomicU64::new(0),
        hits: AtomicU64::new(0),
        misses: AtomicU64::new(0),
    })
}

pub fn cached_build(
    hero_classes: Vec<Class>, villain_classes: Vec<Class>,
    pot_bb: f64, stack_bb: f64, bet_fracs: &[f64], abstraction_hash: u64,
) -> Result<Arc<Subgame>, SearchError> {
    let key = cache_key(&hero_classes, &villain_classes, pot_bb, stack_bb, bet_fracs, abstraction_hash);
    let cache = global();
    let hit_opt = {
        let map = cache.map.lock().expect("cache");
        map.get(&key).map(|(sg, _)| Arc::clone(sg))
    };
    if let Some(hit) = hit_opt {
        cache.hits.fetch_add(1, Ordering::Relaxed);
        let t = cache.tick.fetch_add(1, Ordering::Relaxed);
        // O(1): bump this entry's own tick under a short, separate lock.
        if let Some(entry) = cache.map.lock().expect("cache").get_mut(&key) {
            entry.1 = t;
        }
        return Ok(hit);
    }
    cache.misses.fetch_add(1, Ordering::Relaxed);
    let built = Arc::new(Subgame::build(hero_classes, villain_classes, pot_bb, stack_bb, bet_fracs)?);
    let mut map = cache.map.lock().expect("cache");
    if let Some((existing, _)) = map.get(&key) {
        return Ok(Arc::clone(existing));   // raced insert, keep first winner
    }
    if map.len() >= CACHE_CAP {
        // O(n) scan for min tick — but this now runs ONLY on a MISS that
        // needs to evict, not on every hit. Misses are rare relative to
        // hits in a warm cache (189us warm vs 35ms cold implies most
        // lookups are hits), so this moves the O(n) cost to the path
        // where it's actually infrequent.
        if let Some((&oldest_key, _)) = map.iter().min_by_key(|(_, (_, t))| *t) {
            map.remove(&oldest_key);
        }
    }
    let t = cache.tick.fetch_add(1, Ordering::Relaxed);
    map.insert(key, (Arc::clone(&built), t));
    Ok(built)
}
```

This still takes two short lock acquisitions on a hit (one to read, one to
bump the tick) rather than one — but each is O(1) and short, versus one
lock held for an O(n) scan+shift. Under contention this is a clear win;
uncontended it's roughly a wash (slightly more lock overhead, much less
work per lock).

**Note:** this changes `cache_persist.rs`'s snapshot/hydrate code too,
since it reads `with_global_map` which currently assumes
`HashMap<u64, Arc<Subgame>>`, not `HashMap<u64, (Arc<Subgame>, u64)>`.
Update `cache_persist.rs`'s snapshot logic to unpack the tuple
(`.map(|(k, (sg, _))| (k, sg))`-shaped) before this compiles — **do not
skip updating `cache_persist.rs`, the build will fail without it**, which
is the signal you have every call site.

### Step 9.2 — build, let the compiler find every call site
```bash
cargo build -p cham-search 2>&1 | grep -A2 "error\["
```
Fix every resulting type error — this is the safest way to find every
place that assumed the old shapes.

### Step 9.3 — verify
```bash
cargo test -p cham-search
cargo clippy -p cham-search --all-targets -- -D warnings
cargo bench -p cham-search --bench trigger_cache -- --baseline before-touch-fix
```

### Pass condition
All `cham-search` tests green (including `cache_persist` round-trip tests
and `solve_cached_equals_fresh`, which must be untouched by this change
since it's still a pure content-keyed memo); the multi-threaded contention
benchmark from Item 4 Step 4.5 shows a measurable improvement over the
Item-4-only fix. Update `docs/BOARD.md`.

---

## Final check — run this after every item above lands

```bash
cargo nextest run --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run -q -p cham-cli -- lint-ledger --all
git log --oneline -15   # confirm each item landed as its own reviewable commit
```

All four must be clean before considering this runbook complete. Update
`docs/BOARD.md`'s DONE section with a `### v5-deepdive-audit fixes` block
listing items 1–9 (or 1–8 if Item 9 was skipped) with their one-line
measured outcomes, matching the style already used for the EXP-012..019
entries.
