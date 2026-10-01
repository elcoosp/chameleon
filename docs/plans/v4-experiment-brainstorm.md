# CHAMELEON — New Experiment Brainstorm (post v3-execution-roadmap)

> **Metric note (2026-10-01):** LBR figures in this doc use the clairvoyant `lbr::lbr_vs`; the corrected infoset-consistent value is 6-10x smaller (`docs/plans/F1-CORRECTED-METRIC-2026-10-01.md`).


> **Status:** proposal. Written for an implementing agent with repo write
> access and no prior context beyond this file, `docs/BOARD.md`, and
> `docs/HANDOFF.md`. Every experiment below states exact files, exact
> function signatures taken from the current tree, exact CLI commands, and
> a pass/fail rule you write into a `PREREG-EXP-0NN.toml` before running —
> same discipline as `experiments/PREREG-EXP-001.toml`.
> **Grounding:** `docs/reports/p1-fallback-diagnosis-20260926.md` (fresh,
> unresolved), `worklog.md`'s `v3-execution-roadmap implementation` entry,
> `artifacts/ledger/ledger.jsonl` (post-implementation ladder rows, all
> still fallback-contaminated per their own `notes` field), and the v3
> infra that now actually exists: `FrozenAgent`/`self-exploit`,
> `benches/exploitability.rs`, Bayesian router fusion
> (`RouterRuntime::weights_for_hand` / `posterior_variance`), `--profile
> exact` EMD buckets, the LRU river cache, `lint-ledger`.

## 0. Where this picks up

The v3 roadmap shipped (worklog: *"Implemented `docs/plans/v3-execution-roadmap.md`
§1–§6"*). But the ladder rows it produced are explicit about the current
blocker — every row in `artifacts/ledger/ledger.jsonl` from this cycle
carries a note like:

```
"WARNING: ladder[fast]:full fell back on 2010/7540 decisions (26.7%) —
 artifacts missing or stale, strength numbers are meaningless"
```

and `docs/reports/p1-fallback-diagnosis-20260926.md` (measurement-only,
fresh this session) found **why**, precisely:

- **Cause A — training-reachability gap**: on `jamfix` and
  `pnash:overfold:0.15`, *all four experts miss the same infosets*ámy — the
  training opponent distribution never produced those trajectories.
- **Cause B — fallback-order over-report**: on `callbot` and
  `arch:station`, the mixture *has real expert mass*, but a uniform robust
  contribution still flips `fallback_used`, so recoverable decisions get
  counted as full fallbacks.
- A **third bug**: `robust-only` reports `fallback_used` from a mixture
  path it computes and then discards — its 13.1%–25.6% "fallback rate" in
  the ledger is not measuring what it claims to.

The diagnosis report proposes three fixes (R1 widen training distribution,
R2 skip-not-substitute + renormalize, R3 move `fallback_used` to the
decision path) but **implements none of them** — it's explicitly
measurement-only. That is the single most valuable place to spend the next
round of experiments: everything downstream (A2 bucket quality, DCFR
tuning, router fusion) is being measured through a 17–27% fallback-contaminated
lens, per the diagnosis's own "What this means for the roadmap" section.

So this document is organized in two tiers:

- **Tier 0 (§1–§3): close the measurement gap.** Three experiments that
  implement R1/R2/R3 and quantify each one's individual effect — not
  bundled, so you know which fix bought what.
- **Tier 1 (§4–§10): new, original experiments enabled by what v3 just
  shipped.** These are not re-runs of v2/v3-brainstorm items; each one uses
  infrastructure that didn't exist before this session (`FrozenAgent`,
  Bayesian `posterior_variance`, `--profile exact`, the LRU cache) to ask a
  question nobody could cheaply ask before.

Every experiment: state a hypothesis, touch named files, ship as a real
`experiments/EXP-0NN-*.toml` + `PREREG-EXP-0NN.toml` pair, and report
through the ledger with `lint-ledger` passing. Nothing here proposes
leaving the constitution (no `unsafe`, no inference-time NN, bit-exact
determinism, 16 GB fence, 150 ms p99 solving self-cap).

---

## Tier 0 — close the fallback measurement gap

### 1. EXP-012 `fallback-r3-decision-path` — fix the free bug first [S, ~1 hr]

**Hypothesis:** `robust-only`, `argmax`, and `bayes` report a
`fallback_used` bit computed by the mixture path they don't actually use to
decide. Moving the field to the routing match arm changes **zero**
decisions (pure telemetry fix) and should make `robust-only`'s reported
fallback rate drop to reflect only *its own* tier's misses.

**Files:**
- `crates/cham-agent/src/pipeline.rs` — find the `match mode { ... }`
  routing arm (the place that picks `robust.strategy(...)` directly for
  `robust-only`/similar modes) and the place the mixture path sets
  `trace.fallback_used`.
- `crates/cham-agent/src/trace.rs` — `DecisionTrace` struct (already has
  `expert_missed: [bool; 4]`, `robust_missed: bool`, `reach_mass_zero: bool`,
  `mix_zero: bool` from the P1 diagnosis instrumentation).

**Code sketch:**
```rust
// pipeline.rs — routing match arm, illustrative shape (match the real arm names):
let (chosen_sigma, tier_missed) = match mode {
    RoutingMode::RobustOnly => {
        let (sigma, missed) = robust.strategy(obs, enc, seq);
        (sigma, missed)               // robust's OWN miss, not the mixture's
    }
    RoutingMode::Argmax | RoutingMode::Bayes => { /* same pattern for these arms */ }
    RoutingMode::Full | RoutingMode::Mixture => {
        let (sigma, mix_fallback) = compute_mixture(...);
        (sigma, mix_fallback)          // mixture path keeps its own bit
    }
};
trace.fallback_used = tier_missed;    // was: always the mixture path's bit
```

**Run:**
```bash
cargo test -p cham-agent
DIAG_DEALS=60 cargo run -q -p cham-cli -- probe --diag-fallback --bundle artifacts/agent --agent robust-only
DIAG_DEALS=60 cargo run -q -p cham-cli -- probe --diag-fallback --bundle artifacts/agent --agent full
```

**Pass/fail (write into `PREREG-EXP-012.toml` before running):**
- `robust-only`'s reported fallback rate strictly decreases (it was
  inflated by construction).
- `full`'s reported fallback rate is **unchanged** (R3 doesn't touch the
  mixture-path modes) — this is the regression check that R3 is scoped
  correctly.
- No change to `cargo nextest run --workspace` pass count.

### 2. EXP-013 `fallback-r2-skip-not-substitute` — the real behavior change [M, ~1 day]

**Hypothesis:** today, a missed expert tier has its σ *replaced* by
robust's σ inside the mixture; a missed robust gets replaced by uniform.
R2's fix: **drop** the missed tier's weight entirely and renormalize the
remaining tiers, only falling back to uniform when the mixture is
genuinely empty (`mix_zero`). This should **reduce** the reported fallback
rate on Cause-B opponents (`callbot`, `arch:station`) without changing
anything on Cause-A opponents (`jamfix`, `pnash`), because Cause A already
has all four experts + robust missing (nothing left to renormalize over —
`mix_zero` fires correctly either way). This is the cleanest possible A/B:
same trained artifacts, same opponent pool, one mixture-composition change.

**Files:**
- `crates/cham-agent/src/pipeline.rs` — the mixture composition function
  (wherever `expert_missed[k]` currently triggers "substitute robust's σ").
- `crates/cham-router/src/runtime.rs` — `weights_for_hand` returns `[f64; 5]`
  (4 experts + robust); the renormalization needs to happen *after* zeroing
  out missed tiers, using the router's existing weights as priors over the
  *available* tiers only.

**Code sketch:**
```rust
// pipeline.rs — mixture composition
let router_w = router.weights_for_hand(&features, trend_z);   // [f64; 5]
let mut sigma_mix = [0f64; MAX_ACTIONS];
let mut mass = 0.0;
for k in 0..4 {
    if expert_missed[k] { continue; }               // R2: DROP, don't substitute
    let sigma_k = experts[k].strategy(obs, enc, seq);
    for a in 0..sigma_k.len() { sigma_mix[a] += router_w[k] * sigma_k[a]; }
    mass += router_w[k];
}
if !robust_missed {
    let sigma_r = robust.strategy(obs, enc, seq);
    for a in 0..sigma_r.len() { sigma_mix[a] += router_w[4] * sigma_r[a]; }
    mass += router_w[4];
}
let mix_zero = mass <= 1e-9;
if mix_zero {
    sigma_mix = uniform(legal_actions.len());        // only NOW is it a true fallback
} else {
    for a in sigma_mix.iter_mut() { *a /= mass; }     // renormalize over available tiers
}
trace.fallback_used = mix_zero;                       // was: expert_missed[k] || robust_missed
```

**Run:**
```bash
cargo test -p cham-agent -p cham-router
DIAG_DEALS=60 cargo run -q -p cham-cli -- probe --diag-fallback --bundle artifacts/agent --agent full
DIAG_DEALS=60 cargo run -q -p cham-cli -- probe --diag-fallback --bundle artifacts/agent-full --agent full
cargo run -q -p cham-cli -- ab full robust-only --deals 5000
```

**`experiments/EXP-013-fallback-renorm.toml`:**
```toml
id = "EXP-013"
slug = "fallback-renorm"
gate = "G-fallback"
tier = "ab"
a = "full"          # post-R2 pipeline
b = "full"           # SAME artifacts; b runs the pre-R2 code path behind a
                       # feature flag (`--fallback-mode {substitute,renorm}`)
pool = ["callbot", "arch:station", "jamfix", "pnash:overfold:0.15"]
deals_per_opp = 10000
seeds = [1, 2, 3]
margin_mb = 0.0
sprt = false
[expect]
direction = "sensitivity_report"   # this is a mechanism check, not a promotion gate yet
```

**Pass/fail:**
- `callbot` / `arch:station` fallback rate drops materially (Cause B was
  the whole point).
- `jamfix` / `pnash:overfold:0.15` fallback rate is statistically
  unchanged (confirms R2 doesn't paper over Cause A).
- `expert_missed[k]` / `robust_missed` counts in the trace are **identical**
  before/after (R2 only changes what happens *after* a miss is detected,
  never whether one is detected) — this is the correctness invariant to
  assert in a unit test, not just eyeball.
- Add a `cham-agent` unit test: construct a trace where expert 0 misses and
  experts 1–3 + robust don't; assert the returned `sigma_mix` equals the
  renormalized 4-tier mixture, not the old substitute-with-robust value.

**Kill criterion:** if the Cause-B fallback rate doesn't move, the
diagnosis mis-attributed the cause — revert and re-open the P1 diagnosis
rather than shipping a mixture-semantics change for nothing.

### 3. EXP-014 `curriculum-widen-r1` — the actual fix for Cause A [L, multi-day]

**Hypothesis:** Cause A opponents (`jamfix`, `pnash:overfold:0.15`) produce
bet sequences the training opponent distribution never generated, so no
expert's encoder key is ever populated for those trajectories — this is a
*training data* gap, not a router or mixture problem, and R2/EXP-013 cannot
fix it (confirmed by EXP-013's own kill criterion above). The fix is to
widen what the specialists train against.

**Files:**
- `crates/cham-blueprint/src/trainer.rs` — wherever the opponent sampling
  distribution for `TrainMode::Exploit`/self-play is configured.
- `config/agents/*.toml` — the specialist training configs (`argmax.toml`,
  `robust-only.toml`, etc. — check which config drives which specialist's
  opponent mix).
- `config/pool.toml` — the eval-time pool is already the target
  distribution (`jamfix`, `pnash:overfold:0.15` are in it); the gap is that
  **training** doesn't sample from an equivalent distribution.

**Plan (this is v3-brainstorm's B1 opponent grid, scoped down to exactly
what P1 needs):**
1. Add `jamfix`-shaped and `pnash:overfold:{0.05,0.15}`-shaped opponents to
   the specialist training rotation — not necessarily the eval pool's exact
   archetypes (that would be training on the test set), but the same
   *shape* of deviation (shove-heavy preflop trees, overfolding postflop)
   at different seeds/parameters than the eval pool uses.
2. Retrain the 4 specialists + robust with the widened rotation.
3. Re-run the P1 diagnosis harness (`probe --diag-fallback`) on the new
   bundle. Cause A's signature (`e0_miss == e1_miss == e2_miss == e3_miss`
   on the same infosets) should shrink or disappear where the new training
   opponents cover those trajectories.

**Run:**
```bash
cargo run -q -p cham-cli -- train-bp --config config/agents/full.toml \
    --buckets artifacts/buckets-tiny --out artifacts/agent-widened --thread-mode deterministic
DIAG_DEALS=60 cargo run -q -p cham-cli -- probe --diag-fallback --bundle artifacts/agent-widened --agent full
cargo run -q -p cham-cli -- ab full full --deals 10000   # widened vs original, via --bundle override if the CLI supports it; else two `ladder` runs compared by hand
```

**`experiments/EXP-014-curriculum-widen.toml`:**
```toml
id = "EXP-014"
slug = "curriculum-widen"
gate = "G-fallback"
tier = "ab"
a = "full"            # trained with widened opponent rotation
b = "full-baseline"   # original artifacts/agent bundle (frozen copy, kept for comparison)
pool = ["jamfix", "pnash:overfold:0.05", "pnash:overfold:0.15", "arch:nit", "arch:tag", "arch:lag", "arch:station"]
deals_per_opp = 10000
seeds = [1, 2, 3]
margin_mb = 0.0
sprt = true
[expect]
direction = "a_ge_b"
primary_ci_lower_mb = 0.0
```

**Kill criterion:** if fallback on `jamfix`/`pnash` doesn't shrink by at
least half after widening, the trajectories opening those matches are more
varied than a fixed additional opponent set can cover — escalate to a
curriculum that samples bet-sequence *shapes* directly (adversarial
sequence sampling) rather than fixed named opponents, which is a bigger
follow-up, not this experiment's scope.

---

## Tier 1 — new experiments enabled by what v3 just shipped

### 4. EXP-015 `router-manipulation-hparam-sweep` — tune the router against the adaptive attacker, not just raw EV

**Why this is new:** before this session, there was no way to cheaply ask
"how manipulable is the router" as a *number you can sweep against* —
`self-exploit`'s adaptive audit (`switch:arch:nit->arch:lag@40`) only
existed as of the v3 implementation. `RouterRuntime`'s Bayesian fusion
(`N0` = `prior_strength`, `temp`, `shield_beta`, `shield_z`) is also brand
new. Nobody has swept these hyperparameters against the thing they're
actually meant to defend against: a session-level manipulator. Every prior
router tuning idea in this repo (v2/v3 brainstorms) optimizes raw ladder EV
— this optimizes **resistance to the specific attack the router's own
Bayesian-fusion design doc calls out**.

**Files:**
- `crates/cham-router/src/runtime.rs` — `RouterRuntime::new(model, temp,
  prior_strength, shield_beta, shield_z)`.
- `crates/cham-cli/src/cmd/self_exploit.rs` — the adaptive audit path
  (`opp_id = "switch:arch:nit->arch:lag@40"`); parameterize the switch
  point instead of hardcoding `@40`.

**Code sketch — parameterize the switch point and hyperparams:**
```rust
// self_exploit.rs — add CLI args
pub fn run(
    snapshot: &str, buckets: &str, config: &str, deals: u64,
    train_iters: u64, out: &str,
    switch_at: u64,                 // NEW, default 40
    router_overrides: Option<(f64, f64, f64, f64)>,  // NEW: (temp, N0, shield_beta, shield_z)
) -> i32 {
    let opp_id = format!("switch:arch:nit->arch:lag@{switch_at}");
    // ... if router_overrides is Some, construct the `full` victim with a
    // RouterRuntime built from the override tuple instead of the checked-in
    // defaults — needs a small constructor hook in cmd/hero.rs's
    // build_hero("full", ...) path to accept an optional RouterRuntime.
}
```

**Grid:** `switch_at ∈ {10, 20, 40, 80, 150}` (early vs late manipulation)
× `N0 ∈ {4, 8, 16, 32}` (evidence weight) × `temp ∈ {0.5, 0.7, 1.0}`
(sharpening) — 60 cells, each a `self-exploit --deals 2000` adaptive run
(cheap: this is exactly the parallel-friendly shape `EXP-011`'s grid
already established a pattern for).

**Run:**
```bash
for switch in 10 20 40 80 150; do
  for n0 in 4 8 16 32; do
    for temp in 0.5 0.7 1.0; do
      cargo run -q -p cham-cli -- self-exploit \
        artifacts/agent/robust artifacts/buckets-tiny config/abstraction-tiny.toml \
        --deals 2000 --switch-at $switch --router-n0 $n0 --router-temp $temp \
        >> artifacts/router-sweep.log
    done
  done
done
```

**`PREREG-EXP-015.toml`:**
```toml
id = "EXP-015"
slug = "router-manipulation-sweep"
gate = "G-SELF-sweep"
metric = "adaptive manipulator earn rate (mb/seating), lower is better (more resistant)"
grid = { switch_at = [10, 20, 40, 80, 150], n0 = [4, 8, 16, 32], temp = [0.5, 0.7, 1.0] }
deals_per_cell = 2000
[expect]
direction = "report_only"
follow_up = "if a hyperparameter set dominates the checked-in default across ALL switch_at values (not just one), open an EXP-0NN promotion test on ladder EV to confirm it doesn't trade away raw performance for manipulation-resistance"
```

**Pass/fail:** report the full grid (heatmap: `switch_at` × `(N0, temp)` →
manipulator earn rate). Do **not** promote a single winning cell without
also checking it against the standard `EXP-001`-style ladder — a router
that's maximally manipulation-resistant by being maximally conservative
(always robust) trivially "wins" this metric while losing the mixture's
whole thesis; the follow-up rule above exists specifically to catch that
degenerate optimum.

**Kill criterion:** if no hyperparameter combination beats the default by
more than the grid's own noise floor (bootstrap CI over the 2000-deal
samples), the checked-in defaults (`temp=0.7`, `N0=8`) were already
reasonable — stop, don't over-fit hyperparameters to one attack shape.

### 5. EXP-016 `shadow-ladder-via-frozen-agent` — regression gauntlet, now cheap to build

**Why this is new / why now:** v2-roadmap §3.1 proposed a champion/challenger
regression gauntlet but scoped it as needing new opponent-spec
infrastructure. That infrastructure now exists and was built for a
different purpose (`FrozenAgent` / `OpponentSpec::Frozen`, shipped for M6
self-exploit) — this experiment is the "oh, we already have this" follow-up
nobody would have proposed before M6 landed. Every promoted baseline
becomes a frozen opponent for free.

**Files:**
- `crates/cham-opponents/src/frozen.rs` — `FrozenAgent`, `FrozenRows`
  (already shipped).
- `crates/cham-cli/src/cmd/ab.rs` / new `crates/cham-cli/src/cmd/shadow.rs`
  — a thin wrapper: on every promotion (`ab --promote`), snapshot the
  outgoing champion's policy rows via `BlueprintPolicy::export_rows()`
  (already shipped, used by `self_exploit.rs`) into
  `artifacts/shadow/<promotion-hash>/`.

**Code sketch:**
```rust
// crates/cham-cli/src/cmd/shadow.rs (new)
pub fn snapshot_champion(policy_dir: &str, out_dir: &str) -> Result<String, String> {
    let policy = cham_blueprint::BlueprintPolicy::load(std::path::Path::new(policy_dir), 0)
        .map_err(|e| format!("{e}"))?;
    let rows: std::collections::BTreeMap<u64, Vec<f64>> = policy.export_rows().into_iter().collect();
    let bytes = std::fs::read(std::path::Path::new(policy_dir).join("policy.bin")).unwrap_or_default();
    let hash = format!("shadow:{}", blake3::hash(&bytes).to_hex());
    // persist `rows` + hash under out_dir/<hash>/ (postcard, matching the
    // rest of the artifact format conventions)
    Ok(hash)
}

pub fn run_gauntlet(challenger_agent: &str, shadow_dir: &str, deals: u64) -> i32 {
    // list the last 3 promoted shadow snapshots (by mtime), wrap each as
    // OpponentSpec::Frozen (needs a `frozen:<path>` id variant alongside
    // the existing `frozen:<label>` used by self-exploit — extend
    // OpponentSpec::parse's "frozen:" branch to accept a path lookup),
    // and run `ab challenger_agent vs each frozen shadow` via the now-parallel
    // AbRunner::run_shared from v3-execution-roadmap §1.1.
    todo!("wire into cham_eval::AbRunner::run_shared with pool = shadow snapshots")
}
```

**Run:**
```bash
# after every promoting `ab` run:
cargo run -q -p cham-cli -- ab full robust-only --deals 25000 --promote
cargo run -q -p cham-cli -- shadow snapshot --policy artifacts/agent/full/policy --out artifacts/shadow
# before the NEXT promotion candidate ships:
cargo run -q -p cham-cli -- shadow gauntlet --agent full-candidate-2 --shadow-dir artifacts/shadow --deals 10000
```

**`experiments/EXP-016-shadow-ladder.toml`:**
```toml
id = "EXP-016"
slug = "shadow-ladder"
gate = "G-shadow"
tier = "ab"
pool = "LAST_3_PROMOTED_SHADOWS"   # resolved at run time from artifacts/shadow/ mtimes
deals_per_opp = 10000
seeds = [1, 2, 3]
margin_mb = 0.0
sprt = true
[expect]
direction = "a_ge_b"
primary_ci_lower_mb = -10.0   # a small negative tolerance: "not meaningfully worse than what we shipped two weeks ago", not strictly monotone improvement
```

**Pass/fail:** every promotion candidate's CI lower bound vs each of the
last 3 shadows must exceed `-10.0 mb/seating`. This is a **gate**, not just
a report — wire it into `cmd/ab.rs`'s `--promote` path so a candidate that
regresses against its own recent history cannot silently promote.

**Kill criterion:** none needed structurally, but if shadow snapshots start
piling up storage (`policy.bin` at 200 KB–8 MB per the worklog's own
numbers), cap retained shadows at the last 5 and prune older ones — cheap,
add this to `shadow snapshot` directly.

### 6. EXP-017 `emd-bucket-ab-with-exploitability-and-audit` — validate `--profile exact` two ways at once

**Why this is new:** `train-buckets --profile exact` (the A2 EMD rebuild)
and `benches/exploitability.rs` both shipped this session but haven't been
run *against each other* yet, and v2-roadmap §3.4's bucket-quality audit
(realized-EV variance within vs between buckets) was never implemented —
it's a good complement here because exploitability alone can't distinguish
"the abstraction is genuinely better" from "the abstraction just moved
error somewhere the benchmark's sampled subgames don't cover." Running both
checks on the same rebuild is the original part.

**Files:**
- `cham-engine/src/build.rs` — `BuildParams::exact()` /
  `next_street_cdf_exact` (already shipped).
- `crates/cham-blueprint/benches/exploitability.rs` (already shipped).
- New: `crates/cham-engine/src/audit.rs` — the bucket-quality audit (v2
  §3.4, not yet built): from real match data (`cham-rec` flight records or
  a fresh `ladder --instrument`-style run), compute within-bucket vs.
  between-bucket variance of realized showdown EV for a sample of
  same-bucket hands.

**Code sketch (bucket audit):**
```rust
// crates/cham-engine/src/audit.rs (new)
pub struct BucketAuditReport {
    pub within_bucket_var: f64,
    pub between_bucket_var: f64,
    pub ratio: f64,   // between/within — higher is better (buckets separate real EV differences)
}

pub fn audit_bucket_quality(
    hands: &[(u32 /* bucket_id */, f64 /* realized showdown EV */)],
) -> BucketAuditReport {
    use std::collections::HashMap;
    let mut by_bucket: HashMap<u32, Vec<f64>> = HashMap::new();
    for &(b, ev) in hands { by_bucket.entry(b).or_default().push(ev); }
    let grand_mean = hands.iter().map(|&(_, ev)| ev).sum::<f64>() / hands.len() as f64;
    let within: f64 = by_bucket.values().map(|v| {
        let m = v.iter().sum::<f64>() / v.len() as f64;
        v.iter().map(|&x| (x - m).powi(2)).sum::<f64>()
    }).sum::<f64>() / hands.len() as f64;
    let between: f64 = by_bucket.values().map(|v| {
        let m = v.iter().sum::<f64>() / v.len() as f64;
        v.len() as f64 * (m - grand_mean).powi(2)
    }).sum::<f64>() / hands.len() as f64;
    BucketAuditReport { within_bucket_var: within, between_bucket_var: between, ratio: between / within.max(1e-9) }
}
```

**Run:**
```bash
# baseline (mean-EHS) buckets, already built:
cargo bench -p cham-blueprint --bench exploitability -- --save-baseline pre-emd

# EMD rebuild:
cargo run -q -p cham-cli -- train-buckets --profile exact --out artifacts/buckets-emd
cargo run -q -p cham-cli -- train-bp --buckets artifacts/buckets-emd --out artifacts/agent-emd
cargo bench -p cham-blueprint --bench exploitability -- --baseline pre-emd

# bucket-quality audit on both:
cargo run -q -p cham-cli -- ladder --agent full --pool config/pool.toml --instrument-audit artifacts/buckets-tiny > audit-baseline.json
cargo run -q -p cham-cli -- ladder --agent full --bundle artifacts/agent-emd --pool config/pool.toml --instrument-audit artifacts/buckets-emd > audit-emd.json
```

**`experiments/EXP-017-emd-bucket-audit.toml`:**
```toml
id = "EXP-017"
slug = "emd-bucket-audit"
gate = "G-A2"
tier = "ab"
a = "full-emd"
b = "full"          # mean-EHS baseline, same specialist recipe
pool = ["arch:nit", "arch:tag", "arch:lag", "arch:station"]
deals_per_opp = 10000
seeds = [1, 2, 3]
margin_mb = 0.0
sprt = false
[expect]
direction = "a_ge_b"
primary_ci_lower_mb = 0.0
secondary_metric = "exploitability_benchmark_delta_pct"
secondary_threshold = 10.0   # matches v3-brainstorm A2's own kill criterion
tertiary_metric = "bucket_audit_ratio_delta"
tertiary_expect = "positive"  # EMD buckets should raise between/within ratio
```

**Kill criterion:** unchanged from the original A2 spec (<10% exploitability
improvement → feature program wasn't worth it) **plus** a new one: if the
bucket-audit ratio doesn't improve even though exploitability does, the
exploitability benchmark's sampled subgames aren't representative — widen
its sampling before trusting either number in isolation.

### 7. EXP-018 `empirical-meta-strategy-over-agent-zoo` — solve, don't guess, which mode to ship

**Why this is new:** the ledger now has real post-implementation A/B rows
across `full`, `robust-only`, `argmax`, `bayes` (visible in the current
`artifacts/ledger/ledger.jsonl`), and `cham-search::oracle::solve_matrix`
(supports up to 5×5 zero-sum matrix games by exact support enumeration) has
existed since the search crate's earliest spec — but nobody has pointed it
at the agent zoo's own pairwise ledger data. v2-roadmap §3.3 proposed this
and scoped it as "hand-rollable... no new deps" — it needed the ledger to
actually have enough pairwise rows, which it does now.

**Files:**
- `crates/cham-eval/src/stats.rs` — where to add the payoff-matrix builder
  (reads `ledger.jsonl`, extracts `(a, b, delta_mb)` triples for every mode
  pair that's been A/B'd).
- `crates/cham-search/src/oracle.rs::solve_matrix` — reuse directly; the
  agent zoo has ≤ 8 modes (`full`, `no-search`, `argmax`, `robust-only`,
  `bayes`, `fmbr`, `rnr`, `reach`), so either restrict to the best-covered
  5, or extend `solve_matrix`'s `rows > 5 || cols > 5` guard — cheap since
  the existing support-enumeration algorithm is `2^n`-bounded and n=8 is
  256 support sets per player, still fast.

**Code sketch:**
```rust
// crates/cham-eval/src/stats.rs (new fn)
pub fn build_payoff_matrix(ledger_path: &Path, modes: &[&str]) -> Vec<Vec<f64>> {
    let entries = crate::ledger::Ledger::read_all(ledger_path).expect("ledger");
    let n = modes.len();
    let mut sum = vec![vec![0.0; n]; n];
    let mut count = vec![vec![0u32; n]; n];
    for e in entries.iter().filter(|e| e.kind == "ab") {
        let (Some(a_mode), Some(b_mode)) = (mode_of(&e.a), e.b.as_ref().and_then(mode_of)) else { continue };
        if let (Some(i), Some(j)) = (modes.iter().position(|&m| m == a_mode), modes.iter().position(|&m| m == b_mode)) {
            if let Some(d) = e.delta_mb {
                sum[i][j] += d;  sum[j][i] -= d;
                count[i][j] += 1; count[j][i] += 1;
            }
        }
    }
    (0..n).map(|i| (0..n).map(|j| if count[i][j] > 0 { sum[i][j] / count[i][j] as f64 } else { 0.0 }).collect()).collect()
}
```

```bash
# new subcommand, or a one-off in `cham-eval`'s test/example harness:
cargo run -q -p cham-cli -- meta-solve --modes full,robust-only,argmax,bayes,fmbr --ledger artifacts/ledger/ledger.jsonl
```

**Output:** the Nash mixture over the 5-mode empirical game
(`oracle::solve_matrix`'s `row_strategy`), printed as e.g.
`full: 0.62, argmax: 0.20, bayes: 0.18, robust-only: 0.00, fmbr: 0.00`.

**`PREREG-EXP-018.toml`:**
```toml
id = "EXP-018"
slug = "meta-strategy-zoo"
gate = "G-meta"
metric = "empirical-game Nash mixture over ledger pairwise data"
[expect]
direction = "report_only"
follow_up = "if the solved mixture puts >20% weight on a mode other than the currently-shipped default, open a real EXP-0NN A/B between 'ship full alone' and 'ship the meta-strategy mixture' — mixing deployed modes per-session is a bigger engineering lift (needs a top-level mode-selector, not just router-level mixing) so this should only be pursued if the payoff gap looks real, not noise"
```

**Pass/fail:** this is diagnostic by design (matches v2-roadmap's own framing
— "tells you, principled and computed rather than guessed, whether `full`
is actually dominant"). If the ledger doesn't yet have enough distinct-mode
pairwise rows to populate most of the 5×5 matrix, report that explicitly
and stop — don't fill missing cells with assumptions.

**Kill criterion:** if `full` dominates every off-diagonal cell (which the
existing `EXP-001` framing already half-expects), the meta-strategy is
`full` alone and this experiment confirms current practice rather than
changing anything — a negative result here is still useful, it closes the
question.

### 8. EXP-019 `preflop-equity-table-vs-live-cost` — was the AIVAT preflop table worth it?

**Why this is new:** the v3-execution-roadmap's §2.1 shipped "preflop
all-ins now adjusted via memoized exact `vr::preflop_equity` (C(48,5) once
per pair, HashMap after)" — a runtime memoization, not the offline
GPU-table version the roadmap's step 3 actually proposed (*"enumerate the
1,712,304 (hero, villain) preflop combos once, store exact equities in a
table... instead of computing them per-match"*). Nobody has measured
whether the cheaper runtime-memoized version already captures the win, or
whether the offline GPU table is still worth building. This is a direct,
overdue follow-up measurement on work that's half-done.

**Files:**
- `crates/cham-eval/src/vr.rs` — `preflop_equity` (memoized, as shipped).
- `crates/cham-eval/benches/` — add a micro-bench: cold-HashMap (first
  seating of a fresh process) vs warm-HashMap (steady state) cost per
  preflop all-in adjustment.

**Run:**
```bash
cargo bench -p cham-eval --bench match_throughput -- preflop_equity
# compare: first-hit cost (cold HashMap, effectively the C(48,5) enumeration)
# vs 1000th-hit cost (HashMap lookup) — this tells you the amortization curve.
```

**`PREREG-EXP-019.toml`:**
```toml
id = "EXP-019"
slug = "preflop-equity-cost"
gate = "G-C1-followup"
metric = "cold vs warm preflop_equity call cost (ns), and total wall-clock added to a 25000-deal ab run with preflop all-ins present"
[expect]
direction = "report_only"
follow_up = "if cold-call cost materially affects a fresh-process ab/ladder run's wall-clock (i.e., the first N preflop all-in pairs seen this session pay the full C(48,5) cost), build the offline GPU table from v3-execution-roadmap §2.1 step 3 and ship it pre-populated; if the memoized version is already cheap enough in practice (few distinct pairs per session), the GPU table isn't worth building — close the roadmap item as 'sufficient as shipped'"
```

**Kill criterion:** if amortized cost is negligible relative to total match
wall-clock (likely, since preflop all-ins are a small fraction of hands and
the memo persists for the process lifetime), explicitly mark the GPU-table
step of §2.1 **[SKIP — memoization sufficient, measured]** in `docs/BOARD.md`
so nobody re-proposes it without new evidence.

---

## Sequencing

| Order | Experiment | Why here |
|---|---|---|
| 1 | EXP-012 (R3 fix) | Free, ~1 hr, and every other measurement in this doc is cleaner once `robust-only`/`argmax`/`bayes` fallback telemetry is trustworthy. |
| 2 | EXP-013 (R2 fix) | Needs #1's clean telemetry to interpret correctly; directly testable, cheap. |
| 3 | EXP-019 (preflop cost) | Independent, cheap, closes a half-shipped item — do opportunistically alongside 1–2. |
| 4 | EXP-014 (R1 curriculum) | The expensive one; do after #1–#2 so you're not re-measuring fallback through stale telemetry semantics. |
| 5 | EXP-017 (EMD bucket audit) | Independent of fallback work; can run in parallel with #4 once bucket-rebuild machinery is free. |
| 6 | EXP-015 (router manipulation sweep) | Needs a fallback-clean `full` bundle (post #1–#2) so the adaptive audit isn't measuring fallback noise instead of router behavior. |
| 7 | EXP-016 (shadow ladder) | Wire in once a promotion actually happens post-fixes — it needs a "before" snapshot to be meaningful. |
| 8 | EXP-018 (meta-strategy solve) | Purely diagnostic, no dependencies — run whenever, but most informative once the ledger has clean (non-fallback-contaminated) rows to draw from. |

## Guardrails (unchanged)

Every experiment above stays inside the existing constitution: no `unsafe`
in core crates, no inference-time neural network, bit-exact determinism for
artifacts, the 16 GB memory fence, the 150 ms p99 solving self-cap, Slumbot
diagnostic-only. Every experiment ships a `PREREG-EXP-0NN.toml` before it
runs and reports through `lint-ledger`. If a kill criterion fires, stop and
record the negative result in `docs/BOARD.md`'s SKIP/DEAD section — a clean
negative result (like EXP-019 possibly closing the GPU-table item) is as
valuable as a positive one.
