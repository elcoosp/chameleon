# CHAMELEON — deep dive: what it takes to be competitive vs Nash/GTO on an M1 mini (16 GB)

*Snapshot read: `dump.txt` (HEAD ≈ 2026-10-03, 589 files, 29.6k lines of Rust, 130 plan docs). I read the trainer, traversal, table, encoder, ladder, bucket builder, tabular BR, agent pipeline, search bridge, match engine, opponent pool and ~25 result docs. I could **not** compile or run the Rust (no `cargo` in my sandbox); the one thing I ran is a Python reproduction of the bucket function (§3.2). Every Rust patch below is written against the APIs I read and is **unverified** until you build it.*

Reliability tags: **[V]** verified by reading the code and/or reproduced numerically · **[C]** from code reading only · **[D]** taken from your docs/logs · **[E]** my estimate · **[H]** hypothesis to test.

---

## 0. TL;DR

**Memory is not what limits you. Your tables are ~6 MB; the 6 GB budget holds ~10⁸ rows.** What limits you is (a) what the abstraction can represent, (b) a measurement setup that cannot see Nash-class play, and (c) a few real bugs. Your docs already found many trainer bugs; the ones below are **new** (not in any doc I read):

| # | New finding | Sev. | Tag |
|---|---|---|---|
| 1 | **Search regression root cause found.** `map_to_legal` sends every solver `bet0.5 / bet1 / jam` to *the first aggressive entry of `obs.legal`*, which is the **minimum bet** (the engine's legal list only holds `[Check, Bet(min), Bet(max)]`). The solver "value-bets" 1 bb. This matches your measured loss exactly (callbot 24.8k → 12.3k, station 14.1k → 5.7k) and why *all three solvers give identical results*. Hero strength is also taken from the suit-blind proxy. | **Critical, 1-hour fix** | [V] code, effect size [H] |
| 2 | **The shipped "tiny" buckets are ~98% a suit-blind fallback.** `buckets-tiny` was built from 20,000 sampled orbits (flop has ≈1.29M, turn ≈55M → ≈1.5% / 0.04% coverage). Misses fall back to `strength_now`, which ignores suits: **made flushes and flush draws are invisible**. Reproduced: a made flush on the flop lands in bucket ≈9.5/31, 82% of them *below the median bucket*. | **Critical** | [V] |
| 3 | **Your only "Nash-like" opponent (`pnash`) is a uniform-random bot.** `build()` never receives a strategy source (no caller of `build_with_source` passes `Some`), so `PerturbedNashAgent` tilts `uniform_source` — uniform over `obs.legal` = fold / call / min-raise / jam, plus 15% extra fold. There is **no Nash-like opponent anywhere in the pool.** | High | [V] |
| 4 | **The deployed agent plays pure strategies** (`argmax` routing = mode of the expert's σ), and its exploitability was **never measured** — every BR number is for the `robust` arm sampled as a mixed strategy. | High | [V] |
| 5 | **The corrected metric cannot resolve the differences you decide with.** Your own doc proves a converged BR can't be negative, yet sums are −1.45/−1.29/−0.70/−0.40; the implied SE is ~0.5 bb/seat, and the BR is restricted to the policy's *own* buckets/actions (abstract-game exploitability). All "tiny vs rich-lite vs DCFR" deltas (≤1 bb) are inside the noise. | High | [V]/[E] |
| 6 | **No 3-bet / 4-bet exists in any config.** `raises_per_street_cap` is global; tiny/medium/rich-lite use cap 1 ⇒ BB vs a 3 bb open can only fold / call / **jam 100 bb**. `preflop_open_bb` is dead config. | High | [V] |
| 7 | Parallel trainer returns `NaN` on a cold row and the whole traversal's ancestors skip the update: **selection bias with no telemetry** (rate unknown). | Medium | [C] |
| 8 | Units: `mb/seating` = **milli-bb per hand** (duplicate-averaged). Your ladder mean +8,365 is **8.4 bb/hand ≈ 837 bb/100**, not "+1.7 bb/100" as `SOTA-2026-09-28.md` says. | Doc | [V] |

**Where you really are.** Against 9 scripted bots you win massively (+8.4 bb/hand); against anything adaptive or equilibrium-like you have **zero measurement**, and the structure (1 flop/turn size, no re-raises, suit-blind buckets, pure-strategy play) says a competent GTO bot would beat you heavily. That is a judgement, not a measurement — §5 gives the cheap experiments that settle it.

**Ranked plan (details §5–§6):**
1. Fix search mapping (§3.1) — 1 h + 20-min A/B.
2. Fix/replace the buckets and *refuse sampled tables* (§3.2) — GPU build ≈ hours, you already own the machinery.
3. Build a measurement that can see Nash play: fine-information BR + deployed-agent BR + real Slumbot probe (§3.3).
4. Deploy mixed, budget-capped exploitation instead of pure `argmax` (§3.4).
5. Real tree: preflop levels + 2–3 postflop sizes + history compression (§3.5), trained 15–30 h on the M1.
6. Only then: combo-level river re-solve with a safety gadget (§5).

---

## 1. The numbers that actually exist (from your docs)

### 1.1 Training / throughput [D]

| Quantity | Value | Source |
|---|---|---|
| tiny infosets, pre-F6a → post-F6a | 21,386 → **80,772** (same config/seed) | `F6A-INFOSET-EXPANSION` |
| tiny 5M iters | 62 visits/infoset, ~67 min (box under load 110–170) | same |
| rich-lite 5M iters | **190,158 infosets**, 26 visits/inf, **15 min** (4 threads) | `RICH-LADDER-INFOSET-EXPLOSION` |
| rich (3 sizes, cap 2) | 3.2M infosets, 1.56 visits/inf, ~49 h projected | same |
| per-iteration cost (1 thread) | tiny 0.33 ms, rich-lite 0.46 ms | same |
| table row layout | 8·W + 8 bytes | `README`/spec |

The machine is shared with your other trainers (`pkr-trainer` ×2, `expl_eval`), so wall times are pessimistic.

### 1.2 Strength vs scripted bots (retrained 19-dim bundle, `--agent full`, 2500 deals/pair) [D]

| opp | mb/seating | opp | mb/seating |
|---|---:|---|---:|
| nit | 2,939 | jamfix | 3,696 |
| tag | 5,006 | pnash:overfold | 3,869 |
| lag | 7,870 | famB:tag | 3,458 |
| station | 14,887 | noisy:lag | 7,594 |
| callbot | 25,966 | **mean** | **8,365** |

`mb/seating = (net_A + net_B)/2 /100 × 1000` (`matcheng.rs:212`) ⇒ milli-bb per hand. Mean = **8.4 bb/hand**.

### 1.3 Exploitability (tabular BR, budget 5000/500/30, both seats) [D]

| policy | sum BR(0)+BR(1) bb | note |
|---|---:|---|
| old shipped robust | +15.43 | pre-F3/F4/F6a trainer |
| tiny CFR+ 5M | −1.45 | |
| tiny DCFR(1.5,0,γ2) 5M | −0.70 | |
| retrained robust | −1.29 | |
| rich-lite robust | −0.40 | |
| medium-20M (old trainer) | +22.34 | stale artifact |

### 1.4 Search A/B (locked, current binary) [D]

| opp | OFF | ON | Δ |
|---|---:|---:|---:|
| callbot | +24,797 | +12,326 | −12,471 |
| station | +14,092 | +5,738 | −8,354 |

Rnr / ReachGadget / Fmbr identical within SE ⇒ the loss is *upstream of the solver* (your own conclusion; §3.1 finds where).

### 1.5 Anchors that do **not** exist [D]
No real Slumbot result. No real-game exploitability. No evaluation of the *deployed* (argmax-routed) agent against anything adaptive.

---

## 2. What the numbers mean (and don't)

* The ladder is 9 scripted bots. Winning 8.4 bb/hand against them shows the exploit machinery works on scripts. It says nothing about equilibrium opponents — and `pnash` (the one that looked Nash-like) is random (§3.3).
* The tabular BR is a **same-abstraction** best response: the BR picks one action per *your* infoset key (same 32/16/16 buckets, same ladder). It cannot find any exploit that needs information or actions your abstraction lacks — which is precisely how real HUNL exploits work. So "−1.29 bb ≈ unexploitable" means "converged **inside the abstract game**", not "hard to beat".
* Noise: your own doc gives a combined SE of ≈327 mb for a *difference* of two policies at 2500 deals ⇒ per-policy SE ≈ 231 mb ⇒ **σ_deal ≈ 11.6 bb** [E]. With `test_deals = 500` the BR evaluation SE is ≈ **0.52 bb/seat, ≈0.73 bb on the sum** [E] — and it is not printed. The −1.45/−1.29/−0.70/−0.40 spread is ≈1 SE. Only "+15.4 vs ~0" is a real signal.

---

## 3. Findings and fixes

### 3.1 Search: the action mapping sends every bet to a min-bet — **fix first**

**Evidence [V].** `crates/cham-agent/src/search_bridge.rs::map_to_legal`:

```rust
let aggressive_slot = legal.iter()
    .position(|a| matches!(a, Action::Bet { .. } | Action::Raise { .. }));   // FIRST aggressive entry
...
} else if label.starts_with("bet") || label == "jam" { aggressive_slot }     // all sizes collapse here
```

and `Engine::legal_actions` (`cham-core/src/engine/mod.rs:295`) emits only `[Check, Bet{min_to}, Bet{max_to}]` ("the canonical legal list only shows the extremes", `obs.rs:162`). So `bet0.5`, `bet1`, and `jam` all become `Bet{min_to}` = 1 bb on the river. Against a calling station that is exactly "stop value-betting": your OFF→ON halving. It also explains solver-independence and why a safety gadget alone "would at best revert to OFF" — the selected *action* is never the solver's.

Secondary: hero strength is `strength_now` (suit-blind proxy, §3.2) although on the river exact equity is available; and the solver tree uses `[0.5, 1.0]` while the live river ladder is `[0.5, 1.25, jam]`.

**Fix (UNVERIFIED sketch).**

```rust
// search_bridge.rs — replace map_to_legal; return a distribution over REAL actions
fn label_to_action(obs: &Observables<'_>, label: &str) -> Option<Action> {
    let max_to = obs.max_raise_to;                         // all-in level
    let min_to = obs.min_raise_to.min(max_to);
    match label {
        "check" => Some(Action::Check),
        "jam"   => (max_to > obs.current_bet).then_some(Action::Bet { to: max_to }),
        l if l.starts_with("bet") => {
            let f: f64 = l[3..].parse().ok()?;             // "bet0.5" -> 0.5
            let to = ((f * obs.pot as f64).floor() as i64).clamp(min_to, max_to);
            Some(Action::Bet { to })                       // to_call == 0 guard => Bet only
        }
        _ => None,                                         // unknown label: refuse, never remap
    }
}

fn map_to_legal(obs: &Observables<'_>, solver_dist: &[f64],
                sg: &cham_search::subgame::Subgame) -> Option<Vec<(Action, f64)>> {
    let mut out: Vec<(Action, f64)> = Vec::new();
    for (i, label) in root_action_labels(sg).iter().enumerate() {
        let p = solver_dist.get(i).copied().unwrap_or(0.0);
        if p <= 0.0 { continue; }
        let a = label_to_action(obs, label)?;
        if !is_legal(obs, a) { continue; }
        match out.iter_mut().find(|(b, _)| *b == a) { Some(e) => e.1 += p, None => out.push((a, p)) }
    }
    let tot: f64 = out.iter().map(|x| x.1).sum();
    if tot <= 1e-12 { return None; }
    out.iter_mut().for_each(|x| x.1 /= tot);
    Some(out)
}

// try_solve: exact river equity for hero (suit-aware), not strength_now
let mut b5 = [cham_core::card::Card(0); 5];
b5[..board.len()].copy_from_slice(&board);                 // river => len == 5
let hero_strength = cham_engine::tables::river_equity(obs.hole, &b5);

// pipeline.rs: SAMPLE the solver's mixed strategy (it is an average strategy), don't take its argmax
let probs: Vec<f64> = outcome.distribution.iter().map(|x| x.1).collect();
action = outcome.distribution[sample_index(&probs, rng)].0;
```

Regression test to add: build a river `Observables` (pot 2000, deep stacks, `to_call == 0`) and assert `label_to_action(obs,"bet1") == Bet{to: 2000}`, `"jam" == Bet{to: max_raise_to}`, and that the old behaviour (`Bet{to: min}`) never appears for `bet*`.

**Prediction [H]:** rerun your locked A/B (callbot + station, 2500 deals/pair). ON−OFF should move from −12.5k/−8.4k to ≈0 or better. If it does not, the remaining suspect is the villain range (your plan's Half B) — but you now have a cheap way to know. Keep the gadget (Half A) as the safety floor afterwards; the class-based solver still cannot model blockers/draws (F10).

---

### 3.2 Buckets: ~98% suit-blind fallback in "tiny"; draws and flushes don't exist

**Evidence.**
* `artifacts/buckets-tiny/meta.json`: flop `orbits: 20000, coverage: "sampled"`, turn the same. Canonical orbits: flop ≈ 1.29M, turn ≈ 55M (`build.rs` header) ⇒ hit rate ≈ **1.5% flop, 0.04% turn** [E]. `orbit_bucket` returns `MISS_SENTINEL` on a miss and `Encoder::bucket` then calls `fallback_bucket` = `round(strength_now · (k−1))`. [V]
* `strength_now` ranks via `partial_packed` → `best_nonflush_packed_from_counts`: **rank counts only; suits are never read** [V]. A made flush is scored as its best non-flush hand; a flush draw as high card.
* Reproduction (Python re-implementation of that logic, flop, k=32; script in Appendix A):

```
flop MADE FLUSH   strength_now mean=0.307  -> k=32 bucket mean=9.5
flop FLUSH DRAW   strength_now mean=0.320  -> k=32 bucket mean=9.9
made flushes bucketed below median bucket: 82%
```

  A made flush (true equity vs a random hand ≈ 0.8+) sits in the same bucket as a 30th-percentile hand; nut flush draws are indistinguishable from made flushes (and from air-ish pairs). In self-play this is "consistently blind", so the equilibrium *within* the abstraction looks fine and every internal metric agrees; against an opponent who sees suits it is a large, systematic leak — and the BR metric (same buckets) can't find it.
* The "full/medium" configs use GPU EHS tables (coverage unknown to me — check `coverage` in their `meta.json`). EHS is potential-aware only through the *mean* of final equity; the `river_cdf16` histogram features in `build.rs` are better but only used for the 20k sampled orbits.

**Fix, in three layers.**

*(a) Make the failure loud (30 min).* Count table hits vs fallbacks and refuse sampled tables:

```rust
// encoder.rs — in Encoder: add counters; in bucket(): bump table_hit[street] / fallback_hit[street]
pub struct BucketStats { pub table_hit: [u64; 4], pub fallback_hit: [u64; 4] }

// train-bp / ladder / play startup:
pub fn require_full_coverage(dir: &Path) -> Result<(), EngineError> {
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.join("meta.json"))?)
        .map_err(|e| EngineError::Config(e.to_string()))?;
    for st in ["flop", "turn"] {
        if v[st]["coverage"].as_str().unwrap_or("sampled") != "full"
            && std::env::var("CHAM_ALLOW_SAMPLED_BUCKETS").is_err() {
            return Err(EngineError::Config(format!(
                "{st} bucket table is sampled (fallback = suit-blind strength_now); \
                 rebuild with full coverage or set CHAM_ALLOW_SAMPLED_BUCKETS=1")));
        }
    }
    Ok(())
}
```
Print `table_hit / (table_hit+fallback_hit)` per street at the end of every train/ladder run.

*(b) Make the fallback correct (1 h; changes keys ⇒ retrain).* Add flushes to `partial_packed` (`CAT_FLUSH`/`CAT_STRAIGHT_FLUSH` already exist in `eval/mod.rs`; categories are the high digit of the packed value so `max` orders correctly):

```rust
fn partial_packed(cards: impl Iterator<Item = Card>) -> u32 {
    let mut counts = [0u8; 13];
    let mut by_suit = [0u16; 4];                          // rank bitmask per suit
    for c in cards {
        counts[c.rank() as usize] += 1;
        by_suit[suit_of(c)] |= 1 << c.rank();              // suit_of: use your Card accessor
    }
    let base = best_nonflush_packed_from_counts(&counts);
    let Some(fs) = (0..4).find(|&s| by_suit[s].count_ones() >= 5) else { return base };
    let m = by_suit[fs];
    let sf = STRAIGHT_TABLE.get_or_init(build_straight_table)[m as usize];
    let cand = if sf != 0xff { pack(CAT_STRAIGHT_FLUSH, &[sf]) } else {
        let (mut top, mut n) = ([0u8; 5], 0);
        for r in (0..13).rev() { if (m >> r) & 1 == 1 && n < 5 { top[n] = r as u8; n += 1; } }
        pack(CAT_FLUSH, &top)
    };
    base.max(cand)
}
```
Also worth knowing: `best_nonflush_packed_from_counts` allocates four `Vec`s per call and `strength_now` calls it ~1326× per lookup — a likely hidden throughput cost on every cache miss [C]; profile before/after (§4).

*(c) The real fix: full-coverage, potential-aware tables.* Your GPU path already did exact EHS for all flop/turn orbits (turn ≈1.4 h). The potential-aware feature (`exhaustive_runouts`: the 47/46 next-card equity histogram) is the **same enumeration, keeping the per-card values instead of the mean**, so the cost is the same order. Plan: histogram features for all 1.29M flop orbits; a ~2M-orbit stratified sample for turn centroids, then GPU nearest-centroid assignment of all 55M; write `coverage:"full"`. Start at 128/64/64 buckets (not 32/16/16), keep river = exact-equity quantiles × texture.

---

### 3.3 Evaluation can't see Nash-class play — fix the instrument before tuning anything else

**(1) `pnash` is uniform-random [V].** `factory::build(spec, chart)` = `build_with_source(spec, chart, None)`; `matcheng.rs:258,323` call `build`; nothing in the workspace passes `Some(source)`. `PerturbedNashAgent::tilted` then uses `uniform_source` (uniform over `obs.legal`: fold, call, min-raise, jam). Either wire the blueprint source (a *different-seed*, 2× longer robust run, in **sample** mode) or rename the opponent to what it is. The ladder currently contains no strategy that plays remotely like equilibrium.

**(2) Add the three honest anchors.**

| Anchor | Why | Cost |
|---|---|---|
| Independent sparring partner: robust blueprint, different seed, 2–4× iters, sampled | first opponent that doesn't leak the same way you do | one overnight |
| Learned exploiter: `self-exploit --train-iters 2e6` against the **deployed** agent (real engine, duplicate) | real-game *lower* bound on exploitability; the static `G-SELF` (+39.7 bb) you have is the clairvoyant `lbr_vs` | 1–2 h |
| Slumbot probe, 200 bb, 2–5k hands, AIVAT | only external anchor | client sleeps ≥1 s/action ⇒ ~6 h per 2k hands (run in background) |

Slumbot plays 200 bb; you train at 100 bb. SPR bands top out at 40 so preflop merges at both depths — extend `spr_bands` (e.g. to 80) or retrain at 200 bb before reading the number.

**(3) Fine-information BR (30 lines, big upgrade).** Keep the *policy* on its own encoder, but key the **BR's** infosets with a finer encoder that shares the same ladder. The BR then sees exact-ish cards while still being restricted to the abstract actions, which is much closer to real exploitability:

```rust
// lbr.rs — tab_walk / tabular_br gain `key_enc: &mut Encoder`
let slots = enc.slots(&obs, seq);                       // same ladder => same slots, same seq
let key   = key_enc.key_for(&obs, seq, &slots).0;       // FINER buckets (e.g. 300/200/64, full coverage)
```
`key_enc = Encoder::from_config(fine_cfg, fine_buckets_dir)` with `fine_cfg = cfg.clone()` but `buckets` replaced and `ladder`/`spr_bands`/`seq_history_len` identical. Report the ratio *same-abstraction BR : fine-information BR*; if the second is ≫ the first, the "−1.3 bb" headline is an abstraction artifact.

**(4) Make the estimator able to say "I don't know".** The negative sums mean the learned choice is noise on thin infosets (5000 train deals vs 190k infosets). Add per-infoset visit counts and keep the neutral action unless the evidence beats noise; return an SE:

```rust
struct Cell { w: usize, n: u32, sum: [f64; 12] }                  // n = visits to this infoset
const MIN_N: u32 = 30;                                            // below this: keep passive/previous choice
// when updating `choice`:
if cell.n < MIN_N { continue; }
// evaluation: collect per-deal values, report SE
let vals: Vec<f64> = /* one entry per test deal */;
let se = (vals.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / ((n - 1) * n) as f64).sqrt();
// LbrReport { .., se_bb: se }
```
and parallelise the learn pass over deals (thread-local `Cfv` maps merged by key) so `train_deals` can reach 10⁵–10⁶. Common random numbers (same seeds across policies) are already used; keep them and report **paired** differences.

**(5) Measure the deployed agent.** `action_distribution(&mut self, obs)` exists on `ChameleonAgent`; bridge it into the BR closure:

```rust
let mut agent = ChameleonAgent::new(/* bundle, mode = argmax */);
let mut pol = |obs: &Observables<'_>, seq: &ActionSeq| {
    agent.set_seq_for_tests(*seq);
    agent.action_distribution(obs).unwrap_or_default()
};
let r = tabular_br(&mut pol, seat, cfg, &mut enc, 200_000, 5_000, 30, seed)?;
```
Add a test that for `routing = "argmax"` this returns a **one-hot** on the modal action (if it returns the expert's σ it understates the deployed agent's exploitability). Reset the agent per deal (cold router) and also run with a frozen "worst-routed" expert.

---

### 3.4 Deployment: pure `argmax` + unbounded exploiters ⇒ trivially exploitable by design

**Evidence [V].** `pipeline.rs` `"argmax"` arm: `slots[argmax_of(&sigma)].action // NO rng`. Your F7 A/B shows mode beats sampling by 1,236 mb on the *scripted* pool and you correctly note it is the wrong test for adaptive opponents. Experts are one-sided best responses to scripts, so each is exploitable by construction; playing their mode removes every bluff frequency.

**Fix — bounded exploitation with a sequence-form guarantee.** Because exploitability is convex in the sequence-form strategy, committing to the expert **for a whole hand** with probability λ and to `robust` otherwise gives `ε ≤ (1−λ)·ε_robust + λ·ε_expert`. (Per-infoset mixing does *not* have this guarantee; per-hand commitment does.) Pick λ from router confidence, capped by a measured per-expert budget:

```rust
// pipeline.rs — in start_hand_if_needed, after the router weights are frozen
let k    = argmax_k;
let conf = weights[k];                                                     // router posterior
let lam_cap = (EXPLOIT_BUDGET_MB / expert_exploitability_mb[k].max(1.0)).min(1.0);
let lam  = conf.min(lam_cap);
self.hand_plays_expert = cham_core::rng::next_f64(&mut self.hand_rng) < lam; // whole-hand commitment

// act_impl, "argmax" arm:
let sigma = if self.hand_plays_expert { expert_sigma[k].clone() } else { robust_sigma.clone() }
    .unwrap_or_else(|| uniform(n));
slots[sample_index(&sigma, rng)].action                                    // SAMPLE, never mode
```
`expert_exploitability_mb[k]` comes from §3.3(3) (fine-information BR) — **not** the same-abstraction number. Default `EXPLOIT_BUDGET_MB` small (e.g. 100–300) until the Slumbot/sparring anchors show you can afford more. Add an **open-set** router output ("none of the above → λ = 0"); until the router's ECE gate passes, default λ = 0.

Also stop shipping/scaling the 4-expert bundle on the critical path vs Nash; it only pays against exploitable opponents.

---

### 3.5 Tree: no 3-bet/4-bet anywhere; histories explode; fix both at once

**Evidence [V].** `raises_this_street` counts `ActionClass::Raise` only; the SB open is a `Raise` (it faces the BB blind), so with `cap = 1` the BB's options are `Fold / Call / Jam(100 bb)`; with `cap = 2` (full) a 3-bet exists but then only jam. `preflop_open_bb` is validated and hashed but never read. Postflop with one size, "raise" is either pot-size or jam. A real preflop (open 2–2.5×, 3-bet ≈ 8–9×, 4-bet ≈ 20×, jam) is not representable.

**Why richer trees blew up (your 40× finding).** `key_for` hashes the *full per-street history of all four streets*, so every preflop line multiplies every postflop line. Fixing preflop naively would multiply infosets again. The standard remedy is **imperfect recall of old-street action detail**: keep full detail on the current street, summarise earlier streets. Pot and stack are already in the key via the SPR band, which carries most of what earlier betting implies.

**Patch 1 — per-level preflop ladder (wires the dead config).**

```toml
# config: new, optional
[ladder]
preflop_levels_bb = [[2.5], [8.0], [20.0]]   # level = Raise-class actions already made preflop; beyond => jam
```
```rust
// ladder.rs, facing-a-bet branch, before the raise_fracs loop
if obs.street == Street::Preflop && !self.cfg.ladder.preflop_levels_bb.is_empty() {
    let lvl = raises as usize;
    can_raise = obs.stack > facing && obs.max_raise_to > obs.current_bet
                && lvl < self.cfg.ladder.preflop_levels_bb.len();
    if can_raise {
        for &bb in &self.cfg.ladder.preflop_levels_bb[lvl] {
            let to = ((bb * 100.0).round() as i64).clamp(min_to, max_to);   // 100 chips = 1 bb
            if !out.iter().any(|s| matches!(s.action, Action::Raise { to: t } if t == to)) {
                out.push(AbstractAction { action: Action::Raise { to }, is_all_in: to >= max_to, frac: bb / 100.0 });
            }
        }
    }
}
```
(Keep `all_in_always` jam as the last aggressive option.)

**Patch 2 — history compression in the key.**

```rust
// encoder.rs::key_for — replace the 4-street loop
let cur = obs.street.as_u8() as usize;
for street in 0..4usize {
    if street == cur {                         // FULL detail, exactly as today
        let len = seq.lens[street] as usize;
        bytes[n] = seq.lens[street]; bytes[n + 1] = seq.overflow[street]; n += 2;
        for i in 0..len { let e = &seq.entries[street * 8 + i];
            bytes[n] = e.actor; bytes[n + 1] = e.class.as_u8(); bytes[n + 2] = e.size_bucket; n += 3; }
    } else if street < cur {                   // SUMMARY of a finished street
        let (n_agg, last_aggr) = summarize_street(seq, street);      // aggressive-action count (cap 3), last aggressor actor or 2=none
        bytes[n] = n_agg.min(3); bytes[n + 1] = last_aggr; n += 2;
    }                                          // future streets: nothing
}
```
This changes keys (retrain) and is an imperfect-recall abstraction (no convergence guarantee, standard in practice). **Gate it empirically** with the fine-information BR (§3.3): keep it only if the fine-BR does not worsen at equal wall-clock while infosets drop several-fold.

**Then spend the saved infosets on sizes.** Target (to be tuned by infoset count, §4): preflop levels above; flop `[0.33, 0.75]`, turn `[0.5, 1.0]`, river `[0.33, 0.75, 1.5]`, postflop raise cap 2, raise sizes by street (they currently reuse `raise_fracs`), jam always. Note the translation weights are G&S-correct now (`ph_prob_lower`) but with a ladder of one size + jam every mid-size real bet maps to "min size or shove" (e.g. a 2× pot overbet maps to jam ≈60%) — more sizes is the cure, not more translation.

---

### 3.6 Trainer notes (smaller)

* **NaN discards [C].** When `allow_insert == false` and a hero row is missing, `walk` returns `NaN`; every ancestor then *skips* its update for that sample. Lines through rarely-reached infosets are therefore systematically under-updated, in proportion to how often warmup missed them; the rate is not logged. Cheap first step:

```rust
// Traversal: pub cold_rows: u64;   in the `None =>` branch: self.cold_rows += 1; return f64::NAN;
// trainer: eprintln!("cold-row discards: {:.3}% of hero nodes", 100.0 * cold_rows as f64 / hero_nodes.max(1) as f64);
```
  If it is >1% on rich/medium trees, move to **per-thread shard insert**: a cold key is inserted into a thread-local `RegretTable`, used for the rest of that slice, and merged (sum regrets / strategy sums / visits) into the shared table at the slice boundary. A new row starts at the same uniform-σ state the shared table would give it, so the estimator stays unbiased up to the merge delay.
* **Perf [C].** `RbpConfig::default()` reads an env var on every iteration (constructed per `Traversal`); `CHAM_EXPLORE_EPS` is cached in a process-global `OnceLock` that ignores `cfg.explore_eps` — two sources of truth. Move both into `TrainerConfig`. Profile 60 s with `samply` before and after §3.2: with full tables the 1326× fallback loop (and its Vec allocations) disappears from the hot path.
* **Quantisation [C].** `policy.bin` stores σ as u8 with 2 decimals; actions below 0.5% vanish and mixed strategies are rounded. Irrelevant for play, but **harmful once policies are used as ranges for re-solving** (§5). Use u16 for the shipped artifact (file stays tiny).
* **DCFR.** Your sliced `discount_all` (8 boundaries, clamp at 2M iterations/slice) is an approximation of the per-iteration schedule; fine, but note the clamp silently drops discount for slices >2M iterations (negligible at large t).

### 3.7 Documentation corrections

* `mb/seating` = milli-bb per hand; ladder mean 8.4 bb/hand ≈ 837 bb/100. Fix `SOTA-2026-09-28.md`, `competitiveness.md` ("÷100/÷200").
* `pnash` description (SPECS/03 §5) does not match behaviour (§3.3).
* `search_bridge.rs::try_solve` comment ("agnostic uniform spread") is stale (you noted it).
* Quote every visits/infoset with its trainer era (you already found the 4× shift).

---

## 4. M1 / 16 GB budget math [E, from §1.1]

Rows × bytes: with avg width W≈3, ≈32 B/row ⇒ 10 M rows ≈ 0.3 GB, 100 M rows ≈ 3.2 GB. **Memory permits two orders of magnitude more than you use.** Time is the budget:

| config | rows | iters @ ~60 visits/row | wall @ ~5.5k it/s (4 thr, measured rich-lite) |
|---|---:|---:|---:|
| rich-lite today | 190k [D] | 11 M | ~0.6 h |
| rich-lite + full-coverage 128/64/64 buckets | ~0.5–1.5 M (×3–8) | 30–90 M | 1.5–5 h |
| + preflop levels, 2–3 sizes, cap 2, history compression | ~3–10 M | 180–600 M | 10–30 h |

Assumptions: rows scale ~linearly with buckets per street; per-iteration cost stays 0.3–0.7 ms/thread (it should *fall* once the fallback loop is gone — measure); 60 visits/row is the average, with a long tail of rarely-hit rows. Run on a quiet box (your other trainers currently halve throughput). Checkpoint every slice (already supported) so a 30 h job survives a reboot.

Your GPU path is best used for **table building and vectorised river solves**, not for the MCCFR loop.

---

## 5. Target architecture for "competitive vs Nash" on this hardware

1. **Blueprint** — one *robust* MCCFR policy on the real tree (§3.5), full-coverage potential-aware buckets (§3.2), DCFR(1.5,0,2) (your best measured schedule), sampled (mixed) deployment, 15–30 h.
2. **Translation** — on (it is correct), with enough sizes that it rarely maps to "min or shove".
3. **Real-time river re-solve, done right** — only after §3.1 lands:
   * ranges at **combo level** (1326), obtained by walking the public line through the *blueprint* for each hole combo (`reach[combo] *= σ(a | key(combo, line))`) — your plan's "Half B", but per combo rather than 3 classes;
   * the O(n) blocker-aware showdown kernel from your F10 step 1 (validated);
   * **safety gadget** (Burch/Brown–Sandholm): per-combo opt-out worth the blueprint CFV `v_bp(combo)`; at the root the opponent picks *Follow* vs *Terminate(v_bp)*. Compute `v_bp` by one vector pass of the blueprint over the river tree;
   * u16-precision blueprint rows (§3.6) so ranges aren't rounded.
   * Gate: duplicate match vs blueprint-only ≥ 0 with CI **and** fine-information BR of (blueprint + resolve) ≤ blueprint's.
4. **Exploitation** — per-hand-committed mixture capped by measured budget (§3.4); off by default vs unknown opponents.
5. **Anchors** — sparring partner, learned exploiter, Slumbot (200 bb) run on every promoted bundle.

Turn solving and the v8 value net (needs a `candle` constitution amendment) are phase-3 work; I'd not start them before the river gadget measures positive.

### Roadmap with gates

| Phase | Work | Gate to continue |
|---|---|---|
| **0** (≤1 day) | §3.1 search fix; §3.2(a) coverage guard + hit-rate print; §3.3(4) SE + `MIN_N`; §3.3(5) deployed-agent BR; fix `pnash`; start a 2k-hand Slumbot probe in the background | search ON−OFF ≥ −1 SE on callbot/station; hit-rate numbers known; deployed-agent BR number with SE |
| **1** (1–2 days) | §3.2(b)+(c) tables; retrain robust at rich-lite (≈1 h) with full tables; fine-information BR comparison old vs new | fine-BR(new) < fine-BR(old) by > 2 SE (paired) |
| **2** (2–4 days + 15–30 h run) | §3.5 preflop levels, sizes, history compression; scale iterations by §4 | fine-BR and sparring-partner duplicate match improve; Slumbot probe moves in the right direction |
| **3** (≈1 week) | §5.3 combo river re-solve + gadget | gate above |
| **4** | §3.4 budgeted exploitation; router open-set | loses ≤ ε to sparring partner & learned exploiter while still winning on scripts |

**Targets for the Slumbot probe (aspirational, not predictions):** after Phase 1 "not catastrophically negative", after Phase 2/3 a CI that approaches 0. I have no basis to predict the number, which is exactly why Phase 0 starts the probe now.

### What to stop doing
* Ranking policies by same-abstraction BR deltas below ~1.5 bb (it is inside noise).
* Treating the ladder mean as a strength headline (per-opponent only; and it is vs scripts).
* Comparing abstractions at unequal visits/row or across trainer eras.
* Scaling the 4-expert bundle before the open-set router and exploitation budget exist.

---

## 6. Experiment checklist (with kill criteria)

| # | Experiment | Expected / kill |
|---|---|---|
| E1 | Search A/B after §3.1 (callbot, station, + tag/lag; 2500 deals/pair) | Δ moves from −12k/−8k to ≈0. If still ≪0 ⇒ villain-range/gadget work is real; else celebrate |
| E2 | Bucket hit-rate telemetry on tiny/medium/full | Confirms ≈1–2% flop hits on tiny. If ≫ ⇒ revise §3.2 |
| E3 | Same-abstraction BR vs fine-information BR on the retrained robust | Ratio ≫ 1 ⇒ the −1.3 bb headline is an abstraction artifact |
| E4 | Deployed (argmax) agent BR vs robust-arm BR, with SE | Deployed ≫ robust ⇒ §3.4 is urgent |
| E5 | Sparring partner (diff seed, 2–4× iters): shipped agent in `argmax` vs `sample` mode, 20k duplicate deals | `argmax` loses more than `sample` ⇒ confirms the pure-strategy leak |
| E6 | Learned exploiter (`self-exploit --train-iters 2e6`) vs deployed agent | first honest real-engine lower bound |
| E7 | Slumbot 200 bb, 2–5k hands, AIVAT | first external anchor; repeat per promoted bundle |
| E8 | Full-coverage histogram buckets (128/64/64) vs fallback, matched visits, fine-BR | must win by > 2 SE or §3.2(c) isn't worth the build |
| E9 | History compression on/off at equal wall-clock | keep only if fine-BR doesn't worsen while rows drop ≥3× |
| E10 | Cold-row discard rate on rich-lite/medium | >1% ⇒ implement shard insert |

---

## Appendix A — reproduction of the bucket check

`/home/claude/work/bucket_check.py` (Python re-implementation of `partial_packed` / `strength_now` as read from `cham-core/src/eval/mod.rs`, flush ignored exactly as in the Rust; flop only, 120 random made-flush and 120 flush-draw boards, k = 32, bucket = `round(s·(k−1))`):

```
flop MADE FLUSH   strength_now mean=0.307  -> k=32 bucket mean=9.5
flop FLUSH DRAW   strength_now mean=0.320  -> k=32 bucket mean=9.9
made flushes bucketed below median bucket: 82%
```
Limits: my transcription of the Rust logic (not the Rust itself); sampled boards; the 1.5%/0.04% coverage is an estimate from orbit counts, not a measured miss rate — E2 measures it.

## Appendix B — what I did not verify
* No Rust compiled or run; all patches are sketches against the APIs I read (`suit_of`, `ChameleonAgent::new` args, `Cfv` plumbing, `hand_rng` etc. may need adjusting).
* I did not read the router training, tracker, GPU kernels or `cham-search` solver internals beyond the bridge.
* The σ_deal ≈ 11.6 bb and BR SE (~0.5 bb/seat) are inferred from the SE printed in `RETRAIN-19DIM-RESULTS`; the BR tool does not print its own SE (add it, §3.3(4)).
* The claim that a competent GTO bot beats the current agent heavily is a structural judgement; E3–E7 turn it into numbers.
* Search fix effect size (E1) is a hypothesis; the mapping bug itself is certain from the code.
