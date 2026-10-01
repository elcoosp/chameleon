# CHAMELEON — Competitiveness audit vs. Nash-class opponents (M1 Mac mini, 16 GB)

*Basis: full read of the `dump.txt` snapshot (HEAD around 2026-09-30), targeted reading of the trainer, traversal, table, encoder, ladder, LBR, search and agent code, plus numbers extracted from `artifacts/` logs and `docs/`. Where I could run something, I did (Python reproductions, listed in Appendix A).*

---

## 0. TL;DR

**Verdict.** The project is a carefully engineered tabular MCCFR system, but its headline "competitive" numbers do not measure competitiveness against equilibrium play, and the shipped configuration is, by the repo's own docs, a best response to a scripted LAG archetype. Against a real Nash-class opponent (e.g. Slumbot) it should be expected to lose heavily. **No real Slumbot result exists anywhere in the dump.** Neither the ledger nor the worklog contains one, and the `slumbot` command is described as a mock with `--real` gated.

**The 16 GB limit is not what holds you back.** The trained tables are tiny (21k infosets for the "tiny" abstraction, a 380 KB policy file). By the repo's own row layout (8W+8 bytes) you could hold on the order of 10⁸ infosets in the 6 GB table budget. The binding constraints are:

1. **Measurement.** The exploitability metric is clairvoyant (F1). Nobody knows how exploitable the policy really is.
2. **Action abstraction.** The tree is so coarse that a 100bb game has essentially no raising (F6).
3. **Training-algorithm defects** that bias the average strategy (F3, F4, F7). The DCFR "negative" results don't test DCFR (F5).
4. **Throughput.** The bot sees 10⁶–10⁷ iterations where the game needs far more.

**Ranked actions (details in §3–§6):**

| # | Action | Cost | Why |
|---|---|---|---|
| 1 | Replace the clairvoyant LBR with an infoset-consistent best response, and use the existing `self-exploit --train-iters` learned exploiter as the headline number (F1) | 1–2 days | Everything else is steered by this metric |
| 2 | Move the average-strategy accumulation to the standard site and make the sums f64 (F3, F4) | ~1 day | Removes a documented recency distortion; a possible root cause of the "RM+ freeze" |
| 3 | Make the tree real: raises/3-bets exist, ≥2–3 sizes per street, wire off-tree translation (F6) | 2–4 days | Today the "game" being solved is not HUNL |
| 4 | Fix the parallel trainer's missing-key `return 0.0` bias (F7) | hours | Biases ancestor regrets |
| 5 | Rebuild the river solver in vector form with card removal and safe re-solving (F10) | 1–2 weeks | The only place real-time compute helps on an M1 |
| 6 | Re-run the abstraction-size question at *equal visits per infoset* with the fixed metric (§4) | overnight jobs | Prior conclusions came from undertrained runs and a broken metric |
| 7 | Gate every exploitation deviation by a measured safety budget (§5.4) | 1 week | Against a Nash opponent exploitation can only cost you |

I could not compile Rust in my sandbox (no `cargo`), so **all Rust below is unverified sketch code written against the APIs I read.** The algorithmic claims it implements were validated in Python (Appendix A).

---

## 1. How reliable is each claim in this report?

| Tag | Meaning |
|---|---|
| **[V]** | Verified by me: read in code and reproduced numerically |
| **[C]** | Established from code reading alone |
| **[D]** | Taken from the repo's own logs/docs (I did not re-run) |
| **[H]** | Hypothesis worth a cheap test |

I read the following in depth: `traversal.rs`, `trainer.rs` (config, averaging, main loop), `table.rs` (atomics, sigma, renorm), `lbr.rs`, `ladder.rs`, `encoder.rs` (key), `cham-proofs`, the head of `solve.rs`/`subgame.rs`, the agent's `on_public_action`, and about 40 plan/report documents. I did **not** read the engine, tracker, router and eval internals line by line, so conclusions about them rest on the docs.

---

## 2. Where the project actually stands

### 2.1 What the numbers say [D]

| Quantity | Value | Source |
|---|---|---|
| Tiny abstraction infosets | ~21,450 | `par-20M.log` |
| Tiny robust, 5M iters, serial | 5,977 s (≈837 it/s) | `retrain-tiny-5M.log` |
| Tiny robust, 20M iters, Hogwild×4 | 6,128 s (≈3,260 it/s) | `par-20M.log` |
| Full abstraction infosets | 123k (robust, 500k iters), 85–101k per expert | `full-agent.log` |
| Full abstraction, 9M parallel | ≈3.6 h per expert (≈690 it/s) | `FULL-9M-RESULT` |
| Policy artifact size (tiny) | 380 KB | `policy.bin` |
| Shipped ladder mean (argmax + degenerate router) | +7,136 mb/seating | `SESSION-SUMMARY-2026-09-30` |
| robust-only ladder mean | +720 | same |
| LBR (clairvoyant, see F1) | tiny 5M: 15,040 / 12,050; full 9M: 18,079 / 14,298 mb/hand | `FULL-9M-RESULT` |

### 2.2 What the numbers do *not* say

* **The ladder pool is scripted bots** (nit/TAG/LAG/station/callbot/jamfix, a perturbed-Nash, family-B, noisy). Winning +24,962 mb/seating against a calling station shows exploitation works on a calling station. It says nothing about a Nash opponent.
* **The shipped router is degenerate** [D]: `SYNTHETIC-ROUTER-IS-DEGENERATE` shows it picks class 2 (LAG) on every decision. So `--agent full` means "play the LAG exploiter every hand", a policy trained one-sidedly against a jittered scripted archetype (README). That policy is a best response to a script, not a defence against anything else.
* **The honest router gate has never passed** [D]: top-1 0.80 at best, TAG/LAG recall ≈ 0.5, ECE 0.30–0.36 (gate ≤ 0.15), across 10-, 11- and 19-dim feature sets.
* **The `bb/100` conversions are inconsistent across docs** [C]: `competitiveness.md` divides mb/seating by 100, `COMPETITIVE-RESULT` by 200. If mb are milli-bb per hand, 1,692 mb is 1.69 bb/hand, i.e. ≈169 bb/100, not 8.5. Check `stats.rs` before quoting any bb/100 figure externally.

---

## 3. Findings

### F1 — The LBR metric is clairvoyant (critical) [V]

**What the code does.** `lbr.rs::br_walk` recurses on a `State` that contains *both* hole cards and the full board. At a BR-seat node it takes `max` over actions *inside each sampled deal*:

```rust
// BR seat: enumerate the abstraction slots, take the max
for s in slots.iter() { ... let v = br_walk(&mut s2, ...); if v > best { best = v; } }
```

A real best response must pick **one action per information set** (the BR player does not know the opponent's cards). Choosing per deal is a perfect-information player who sees the opponent's hand. The docstring says "tabular BR… on our abstraction", but the implementation is the clairvoyant bound.

**Reproduction.** On Kuhn poker (where exact exploitability can be enumerated), I trained a near-Nash strategy (300k iterations) and compared the true exploitability with the repo-style "LBR":

| Quantity | Value |
|---|---|
| True exploitability (sum of exact BRs) | **0.0048** |
| Repo-style per-deal-max "LBR", seat 0 | +0.328 (Nash value −0.056) |
| Repo-style "LBR", seat 1 | +0.222 (Nash value +0.056) |
| Sum of the two "LBR" values | **0.550** (≈115× the true value) |

On a ~Nash strategy this metric reports a huge number. The bias grows with the amount of hidden information, which in HUNL is large.

**Consequences, all consistent with what the repo observed:**

* Absolute numbers (13,000–40,000 mb/hand, "uniform 37,756") are not exploitability. A *good* Nash approximation would still show a large figure.
* LBR and the ladder are "decoupled" (`LBR-VS-LADDER`, `20M-LADDER-NEGATIVE`): the metric is dominated by a perfect-information term unrelated to play quality. The whole freeze investigation (RM+, DCFR α, delay0/eps) was optimising this number. Docs already show those gains do not transfer to the ladder.
* The same bias hits `G-SELF static` (≈39–43 bb/hand in the ledger).
* Note also the opposite bias: the BR is restricted to the abstraction's ladder slots, so it underestimates exploitability in the real game. Neither number is "the" exploitability.

**Fix A (no new code, do this first): use the learned exploiter you already have.** `chameleon self-exploit --train-iters N` trains a best-response blueprint against the frozen shipped policy, which plays real engine actions in duplicate matches. Its earn rate in the real engine is a legitimate lower bound on exploitability (with a CI). Make it the headline number.

**Fix B: infoset-consistent tabular best response.** Choose one slot per infoset key by maximising reach-weighted counterfactual values on training deals, then evaluate on held-out deals (otherwise you overfit the sample). Sketch:

```rust
// crates/cham-blueprint/src/br_tabular.rs  (UNVERIFIED sketch)
use std::collections::HashMap;
use cham_core::card::Deck;
use cham_core::engine::{Action, State, config::EngineConfig};
use cham_core::obs::{Observables, Player};
use cham_core::rng::{child, Rng};
use cham_engine::encoder::ActionSeq;

type Cfv = HashMap<u64, (usize, [f64; 12])>;      // key -> (width, Σ reach_opp * v[a])
type Choice = HashMap<u64, usize>;                // key -> chosen slot

fn passive_slot(slots: &[cham_engine::ladder::AbstractAction]) -> usize {
    slots.iter().position(|s| matches!(s.action, Action::Check | Action::Call)).unwrap_or(0)
}

#[allow(clippy::too_many_arguments)]
fn walk<F>(
    st: &mut State, br: usize, pol: &mut F, enc: &mut cham_engine::Encoder,
    seq: &mut ActionSeq, choice: &Choice, reach_opp: f64, learn: &mut Option<&mut Cfv>,
) -> f64
where F: FnMut(&Observables<'_>, &ActionSeq) -> Vec<(Action, f64)> {
    if st.is_terminal() { return st.payoffs()[br] as f64 / 100.0; }
    let p = st.to_act();
    let obs = Observables::view(st, Player::from_usize(p));
    if p != br {
        let dist = pol(&obs, seq);
        let tot: f64 = dist.iter().map(|(_, q)| *q).sum::<f64>().max(1e-12);
        let mut ev = 0.0;
        for (a, q) in &dist {
            let q = q / tot;
            if q <= 1e-12 { continue; }
            let (mut s2, mut seq2) = (*st, *seq);
            enc.record(&obs, Player::from_usize(p), *a, &mut seq2);
            if s2.apply(*a).is_err() { continue; }
            ev += q * walk(&mut s2, br, pol, enc, &mut seq2, choice, reach_opp * q, learn);
        }
        return ev;
    }
    let slots = enc.slots(&obs, seq);
    let key = enc.key_for(&obs, seq, &slots).0;
    let pick = *choice.get(&key).unwrap_or(&passive_slot(&slots));
    let mut v_pick = 0.0;
    for (i, s) in slots.iter().enumerate() {
        if learn.is_none() && i != pick { continue; }      // eval: follow the fixed choice only
        let (mut s2, mut seq2) = (*st, *seq);
        enc.record(&obs, Player::from_usize(p), s.action, &mut seq2);
        if s2.apply(s.action).is_err() { continue; }
        let v = walk(&mut s2, br, pol, enc, &mut seq2, choice, reach_opp, learn);
        if let Some(c) = learn.as_mut() {
            let e = c.entry(key).or_insert((slots.len(), [0.0; 12]));
            e.1[i] += reach_opp * v;                       // counterfactual weight = opponent reach only
        }
        if i == pick { v_pick = v; }
    }
    v_pick
}

pub fn tabular_br<F>(
    mut pol: F, br: usize, cfg: EngineConfig, enc: &mut cham_engine::Encoder,
    train_deals: u32, test_deals: u32, seed: u64,
) -> f64 where F: FnMut(&Observables<'_>, &ActionSeq) -> Vec<(Action, f64)> {
    let mut choice = Choice::new();
    for _sweep in 0..12 {                                  // ≥ BR decision depth; stop when stable
        let mut cfv = Cfv::new();
        for d in 0..train_deals {
            let rng = &mut child(seed, &format!("brtrain{d}"));
            let mut st = State::new(cfg, Deck::shuffled(rng)).unwrap();
            let mut seq = ActionSeq::default();
            let mut l = Some(&mut cfv);
            walk(&mut st, br, &mut pol, enc, &mut seq, &choice, 1.0, &mut l);
        }
        let mut changed = 0;
        for (k, (w, v)) in &cfv {
            let best = (0..*w).max_by(|a, b| v[*a].partial_cmp(&v[*b]).unwrap()).unwrap();
            if choice.insert(*k, best) != Some(best) { changed += 1; }
        }
        if changed == 0 { break; }
    }
    let mut tot = 0.0;                                     // held-out evaluation, fixed choices
    for d in 0..test_deals {
        let rng = &mut child(seed, &format!("brtest{d}"));
        let mut st = State::new(cfg, Deck::shuffled(rng)).unwrap();
        let mut seq = ActionSeq::default();
        let mut none = None;
        tot += walk(&mut st, br, &mut pol, enc, &mut seq, &choice, 1.0, &mut none);
    }
    tot / test_deals.max(1) as f64                         // bb/hand, BR seat
}
```

Report **(BR₀ + BR₁)/2** as the exploitability, per hand. For a Nash policy BR₀ + BR₁ = 0 in the zero-sum game, so the per-seat numbers the repo currently prints also contain positional game value. A tighter bound uses a finer encoder for the BR player (e.g. `river_eq_bins = 64`, more flop/turn buckets). I validated the same procedure (infoset-aggregated BR with sweeps) in Python on Kuhn and Leduc, where it reproduces exact exploitability.

**Fix C (real-game bound): classic LBR** (Lisý & Bowling 2016): the BR player estimates action values with equity rollouts against the opponent's range, assuming it checks/calls to showdown afterwards. This does not depend on your abstraction and is cheap.

---

### F2 — There is no external anchor [C/D]

* Slumbot appears only in docs and a mock client. The ledger has no real Slumbot entry.
* Slumbot plays **200 bb**; the shipped bundle is trained at `depth_bb=100`. Keys are SPR-banded, so reuse may partly work, but that has never been tested. `ab-depth-200` exists, but only as a mechanical A/B.
* A real match also needs the translation in F6, because Slumbot bets arbitrary sizes.

**Do:** a mock-free 5–10k-hand match at 200 bb with the existing AIVAT-style variance reduction, reported with CI. Verify Slumbot's endpoint is up before relying on it. In parallel, run an **independent sparring partner**: train a robust blueprint with a different seed and 2× the iterations, and play the shipped agent against it in duplicate. That costs one overnight job and needs no network.

---

### F3 — Average strategy is accumulated at the wrong nodes [V]

**What the code does** (`traversal.rs`, hero node):

```rust
sink.add_strat(table, off, w_slots, a, (w_t * sigma[a]) as f32);   // "NO reach factor"
```

In external-sampling MCCFR the traverser enumerates its own actions, so its nodes are reached with probability proportional to the *opponent and chance only*. The average strategy must be weighted by the traverser's **own** reach. The standard way to obtain that for free is to accumulate at the **opponent's** nodes, where the sampling itself supplies the weight. The code and the in-code note ("sampling IS the reach weighting") are right for the *regret* update but the strategy sum is a different object.

**Evidence.** Kuhn is too small to show it (`opp` 0.0093, `trav` 0.0107, `trav+reach` 0.0106 mean exploitability, 100k iters × 3 seeds; indistinguishable). On Leduc (exact exploitability, 60k iters, 8 seeds):

| Scheme | mean | median | max | seeds > 0.40 |
|---|---:|---:|---:|---:|
| accumulate at opponent nodes (standard) | 0.189 | 0.191 | 0.197 | 0 |
| accumulate at traverser nodes, no reach (production) | 0.283 | 0.202 | **0.645** | 2 of 8 |
| same + traverser reach weight | similar to production | | | |

At 120k iters × 3 seeds the same ordering held (0.135 vs 0.222, one outlier at 0.383). The median gap is small, but the production scheme has a **heavy tail of bad runs**. Adding reach weighting at the traverser node did *not* fix it, so the remedy is the accumulation site, not a missing factor. This is a toy; treat it as a strong smell, not a measured HUNL effect. It costs ~15 lines to remove.

**Patch (Robust mode only).** Accumulate at the opponent's row; keep visits at the updating seat so inference-time confidence semantics don't change:

```rust
// traversal.rs — opponent node, Robust branch (UNVERIFIED)
if self.mode == TrainModeTag::Robust {
    let slots = enc.slots(&obs, seq);
    let key = enc.key_for(&obs, seq, &slots);
    let w_slots = slots.len();
    match self.table.as_ref().find(key.0) {
        Some(off) => {
            let sigma = self.table.as_ref().sigma_rms(off, w_slots);
            if !self.warmup_only {                                    // NEW
                for a in 0..w_slots {
                    sink.add_strat(self.table.as_ref(), off, w_slots, a, (w_t * sigma[a]) as f32);
                }
                sink.add_weight(self.table.as_ref(), off, w_slots, w_t as f32);
            }
            /* existing: build `dist` from sigma */
        }
        None => { /* existing uniform */ }
    }
}

// traversal.rs — hero node: skip the strat/weight accumulation in Robust mode
if self.mode != TrainModeTag::Robust {                                // NEW guard
    for a in 0..w_slots { sink.add_strat(self.table.as_ref(), off, w_slots, a, (w_t * sigma[a]) as f32); }
    sink.add_weight(self.table.as_ref(), off, w_slots, w_t as f32);
}
sink.add_visit(self.table.as_ref(), off, w_slots);                    // unchanged
```

Exploit modes (one-sided vs a scripted opponent) have no opponent row; leave them as they are. The `kuhn_mccfr` proof (P-1) uses the *same* hero-node accumulation, so it exercises the production scheme rather than checking it. Add the opponent-node variant to the proof and require it to hold at Leduc scale (a Leduc proof P-5 is cheap: 140 lines).

---

### F4 — The snapshot "renorm" biases the average; f32 sums are marginal at ≥20M [V]

**What happens.** At every snapshot (10 per run) `renorm_row` rescales `strat_sum` and `avg_weight` by 2⁻ᵏ when they exceed 2²². The docs call this lossless because "normalized strategies are scale-invariant". That is true only at the instant of scaling. Afterwards, new increments (`w_t·σ`, which keep growing) are added at **full** weight, so everything accumulated before the scaling is down-weighted by 2⁻ᵏ relative to what follows. The linear average becomes recency-biased. A run of ≥100k iterations triggers this at the first post-delay snapshot.

**Simulation** (f32 CAS adds as in `Arena::add_f32`, delay T/4, snapshot every T/10, T = 1M, drifting iterate where action 1 rises 0.2 → 0.9):

| Accumulator | final average strategy |
|---|---|
| exact f64 linear average | (0.725, 0.255, 0.020) |
| f32, **no** renorm | (0.725, 0.255, 0.020) |
| f32 + snapshot renorm (**current code**) | **(0.866, 0.114, 0.020)** |

A stationary strategy shows no error, which is why the unit test `renorm_preserves_strategy` passes. Real CFR iterates drift.

**Link to the "freeze" [H].** The repo found the average collapsing toward the last iterate (mean max-prob 0.45 → 0.86 at the T/4 delay mark) and the effect disappearing under `delay0`/`avguniform`. Linear weights already favour late iterates; renorm adds an extra recency tilt at each snapshot. This could explain part of the "average collapses to the current iterate" observation that months of effort tried to fix on the regret side (eps floor, DCFR). It is cheap to test: re-run one 5M job with the fix below and compare the soft-row fraction and the *fixed* BR metric.

**Why not simply drop the renorm on f32?** Per-add relative increment is ≈ 2/(0.75·T). f32's half-ulp is 6·10⁻⁸ relative, so increments reach the rounding floor around T ≈ 4·10⁷ (borderline at 20M, lost at 50M). The repo ran a 50M job (`retrain-tiny-50M.log`), so f32 sums are a risk exactly in the regime where you want to go.

**Fix: f64 strategy sums.** Memory is a non-issue (tiny: ~21k rows; even 10⁸ rows × W=4 × 8 B ≈ 3 GB for the sums alone).

```rust
// table.rs (UNVERIFIED sketch): parallel f64 arena for strat_sum + avg_weight
struct Arena64 { cells: Vec<AtomicU64> }
impl Arena64 {
    #[inline] fn add(&self, i: usize, d: f64) {
        let c = &self.cells[i];
        let mut cur = c.load(Relaxed);
        loop {
            let new = (f64::from_bits(cur) + d).to_bits();
            match c.compare_exchange_weak(cur, new, Relaxed, Relaxed) {
                Ok(_) => return, Err(o) => cur = o,
            }
        }
    }
}
// RegretTable: add `strat64: Arena64` with the SAME row offsets as `arena`; strat_add / add_weight
// write to strat64 (delta as f64); avg_strategy() and the policy builder read strat64.
// Remove the renorm pass from trainer.rs snapshot cadence; bump the snapshot version byte.
```

Regression test to add:

```rust
#[test]
fn drifting_iterate_average_matches_f64_reference() {
    // simulate T=1e6 iterations with σ_a(t) drifting; table average must match an f64 reference to 1e-3
}
```

---

### F5 — The DCFR experiments did not test DCFR [C]

`--regret-discount 0.9` multiplies the accumulated positive regret by a **constant 0.9 at every visit** (`new = old*discount + delta`). `DCFR-ALPHA09-NEGATIVE` itself computes the effective window as 1/(1−0.9) = 10 visits and observes a near-uniform policy. Brown & Sandholm's DCFR discounts at iteration *t* by t^α/(t^α+1) (→ 1 as t grows; recommended α = 1.5, β = 0, γ = 2), so memory is long. These experiments show that "exponential forgetting with a 10-visit window" is bad, which nobody doubted. They say nothing about DCFR or Linear CFR. The `DCFR-ALPHA05-PREDICTION` note and the conclusion "DCFR is documented negative twice" should be struck from the next-steps lists.

**What to try instead** (cheap, standard; from my memory of the papers, check the exact variant against Brown & Sandholm 2019 and the Pluribus supplement):

* **Linear MCCFR as in Pluribus:** do *not* floor regrets at zero (use plain regret matching, positive part of R for σ), discount accumulated regrets at checkpoints, and weight the strategy sum by *t*. Floor negative regrets at a large negative bound (negative-regret pruning) to keep f32 range.
* The repo's `regret_add` (unfloored) already exists.

```rust
// table.rs (UNVERIFIED): checkpoint discount; call at slice boundaries of the parallel trainer
pub fn discount_all(&self, t_prev: u64, t_now: u64, alpha: f64, beta_factor: f64) {
    // Π_{s=t_prev+1..t_now} s^α/(s^α+1)  ≈  exp(-Σ s^-α)
    let sum: f64 = if (alpha - 1.0).abs() < 1e-9 {
        ((t_now as f64) / (t_prev.max(1) as f64)).ln()
    } else {
        ((t_prev.max(1) as f64).powf(1.0 - alpha) - (t_now as f64).powf(1.0 - alpha)) / (alpha - 1.0)
    };
    let fp = (-sum).exp() as f32;                      // positive regrets
    for (_k, off) in self.iter() {
        let w = self.row_width(off);
        for a in 0..w {
            let r = self.regret(off, w, a);
            self.store_regret(off, a, if r > 0.0 { r * fp } else { (r * beta_factor as f32).max(-3.1e8) });
        }
    }
}
```

Run Linear-MCCFR-no-floor and DCFR(1.5, 0, 2) as two arms against the F3/F4-fixed baseline, judged by the fixed BR metric (F1), not the ladder.

---

### F6 — The action abstraction is not a poker game [C], and the off-tree machinery is dead code [V]

**(a) The raise cap counts bets.** `ActionLadder::raises_this_street` returns `count(Raise) + count(Bet)`, and `can_raise` requires `raises < raises_per_street_cap`. With the tiny config (`cap = 1`):

* Preflop: SB can fold / call / raise (≈3 bb) / jam. After that raise the cap is reached, so the BB can only **fold / call / jam**. There is no 3-bet below an all-in.
* Postflop: after any bet (which counts as 1) the responder has **fold / call / jam** only; no raises other than a 100 bb shove.

The default "full" config has `cap = 2`, so one raise exists after a bet. That is still far from the real tree. Policies, LBR and "Nash on the abstraction" for this game say little about HUNL. `COMPETITIVENESS-FINDINGS` concluded "betting-tree coarseness is not the bottleneck" from a richer ladder at **50k iterations** (305k infosets, i.e. well under one visit per infoset) judged by the clairvoyant metric. That experiment could not have shown an effect either way.

**(b) `preflop_open_bb` is dead configuration.** It is validated and hashed (`config.rs`) but never read by the ladder (`rg` finds no consumer). Preflop sizing comes from `raise_fracs`. Either wire it or delete it; as is, "2.2/3.0 open" in `abstraction.toml` is fiction.

**(c) Off-tree translation is never used [V].** `harmonic_weights` and `nearest_slot` are defined in `ladder.rs` and referenced from nowhere else in the workspace. At inference, `Agent::on_public_action` records the opponent's **raw** action (`self.encoder.record(obs, player, action, &mut self.seq)`), and `record_action` encodes `size_bucket = round(12 · bet / effective_stack)` clamped to 1..15. Any size the training tree never produced yields a key stream not in the table → fallback. Coarse 1/12-of-stack quantization absorbs some mismatches by accident, which is why the miss rate was 17–27% rather than ~100%, but that is not a principled translation and it degrades with deeper stacks.

**(d) The weights are not pseudo-harmonic [V].** The implemented `1/(0.01 + (x−f_i)²)` gives P(lower | x = lower) = 0.963 rather than 1, and for A = 0.5, B = 1.0, x = 0.6 gives 0.895 where Ganzfried–Sandholm's mapping gives 0.750.

**Fix (translate into the abstract history before keying):**

```rust
// cham-engine/src/ladder.rs (UNVERIFIED)
/// Ganzfried & Sandholm 2013 pseudo-harmonic mapping: probability of mapping x to the SMALLER size a (a < x < b).
pub fn ph_prob_lower(a: f64, b: f64, x: f64) -> f64 {
    ((b - x) * (1.0 + a)) / ((b - a) * (1.0 + x))
}

impl ActionLadder {
    /// Map a real action to the nearest abstract slot action; `u` is a deterministic uniform in [0,1).
    pub fn translate(&self, obs: &Observables<'_>, seq: &ActionSeq, real: Action, u: f64) -> Action {
        let same = |a: &Action, b: &Action| matches!((a, b),
            (Action::Bet{..}, Action::Bet{..}) | (Action::Raise{..}, Action::Raise{..}));
        if !matches!(real, Action::Bet{..} | Action::Raise{..}) { return real; }
        let x = self.frac_of(obs, real);
        let mut c: Vec<(Action, f64)> = self.slots(obs, seq).iter()
            .filter(|s| same(&s.action, &real)).map(|s| (s.action, self.frac_of(obs, s.action))).collect();
        if c.is_empty() { return real; }
        c.sort_by(|p, q| p.1.partial_cmp(&q.1).unwrap());
        if x <= c[0].1 { return c[0].0; }
        if x >= c[c.len() - 1].1 { return c[c.len() - 1].0; }
        let i = c.iter().rposition(|(_, f)| *f <= x).unwrap();
        let p = ph_prob_lower(c[i].1, c[i + 1].1, x);
        if u < p { c[i].0 } else { c[i + 1].0 }
    }
}

// cham-agent/src/pipeline.rs :: on_public_action (UNVERIFIED)
let u = hash01(self.hand_idx, obs.street.as_u8(), self.seq.lens);   // deterministic: replay stays bit-exact
let abs = self.encoder.ladder().translate(obs, &self.seq, action, u);
self.encoder.record(obs, player, abs, &mut self.seq);                // key uses the ABSTRACT action
```

Also quantize `size_bucket` from the **slot index**, not from the stack fraction, so the bucket is a function of the abstract action alone. Both training and inference then produce the same key by construction.

**Better tree (config, to be tested at equal visits/infoset, §4):**

```toml
[ladder]                           # "rich-tiny", EXPERIMENTAL
raise_fracs        = [0.75, 1.5]   # preflop open / re-raise and postflop raise sizes (pot-after-call fractions)
flop_bet_fracs     = [0.33, 0.75]
turn_bet_fracs     = [0.5, 1.0]
river_bet_fracs    = [0.5, 1.0]
raises_per_street_cap = 3          # bet, raise, 3-bet, then jam only
all_in_always      = true
```

---

### F7 — Parallel trainer: a missing key returns utility 0.0 [C]

In `walk_with_sink`, when `allow_insert == false` (all Hogwild phases) a hero node whose key is not yet in the table does `return 0.0`. That 0.0 flows up as the *value of the action that led there*, so its parent computes regrets against a fabricated break-even outcome. In large pots this is a systematic bias for exactly the rare, deep lines that the warmup missed. The fraction of such nodes shrinks as the table fills but never reaches zero (the doc itself says later slices "catch" them).

**Minimum fix:** return the value of a uniform-random rollout, which is noisy but not biased toward 0.

```rust
None => {
    if !self.allow_insert {
        return self.uniform_rollout(state, hero_seat, enc, seq, rng);   // NEW
    }
    ...
}

fn uniform_rollout(&mut self, st: &State, hero: usize, enc: &mut cham_engine::Encoder,
                   seq: &ActionSeq, rng: &mut Rng) -> f64 {
    let (mut s, mut q) = (*st, *seq);
    while !s.is_terminal() {
        let p = s.to_act();
        let obs = Observables::view(&s, Player::from_usize(p));
        let slots = enc.slots(&obs, &q);
        let a = slots[(cham_core::rng::next_f64(rng) * slots.len() as f64) as usize % slots.len()].action;
        enc.record(&obs, Player::from_usize(p), a, &mut q);
        s.apply(a).expect("slot action is legal");
    }
    s.payoffs()[hero] as f64 / 100.0
}
```

A cleaner long-term answer is a lock-free insert (CAS on the slot's key word, then claim arena space with `fetch_add`), which removes the warmup/slice machinery altogether.

---

### F8 — "Full" abstraction has been running with 16 river bins, not 64 [D]

`RIVER-EQ-EDGES-MISMATCH`: `buckets-full/meta.json` carries 17 edges while the config declares `river_eq_bins = 64`. Every full-abstraction result (including the 9M run and the "full vs tiny at equal visits" comparison) used 16 river bins. Conclusions such as "full wins by 22% on seat 0" are conclusions about a bucket set that differs from the one claimed, measured with the F1 metric. Rebuild with 65 edges before any further full-vs-tiny claim.

---

### F9 — Minor correctness/performance issues

* **`std::env::var` in hot paths [C].** `CHAM_EXPLORE_EPS` is read at **every opponent node** in `walk_with_sink`; `averaging_weight_gamma` reads two env vars **every iteration**; `RbpConfig::default` reads one per `Traversal`. `env::var` takes a global lock and allocates. Read once into the `Traversal`/`TrainerConfig`. I did not measure the cost, so benchmark before and after.
* **Per-node allocation [C].** `dist: Vec<(Action,f64)>` at each opponent node, `computed: Vec<usize>` and `sigma: Vec<f64>` at each hero node. Use `ArrayVec<_, 12>` (already a dependency).
* **Quantized policy [C]:** probabilities are stored in a u8 with 2 decimals (`policy.rs`). Actions below ~0.5% vanish, and mixed strategies are rounded, which matters when you later use blueprint strategies as **ranges** for re-solving. Use u16 (4 decimals) if the footprint allows.
* **Same-seed alternation in Robust mode** uses `t % 2` as the traversing seat while `deals[...]` is drawn independently; fine for HUNL, but note that the Kuhn proof comment warns about a coupled deal/seat bias. Keep the test in the Leduc proof.

---

### F10 — The search layer is a simplified river toy [C]

`cham-search` collapses ranges to **weighted 1-D strength classes** (D-012: "deterministic strength ordering decides showdowns, which makes every solver exactly LP-verifiable"), with a reduced bet tree. That choice makes testing easy, but it throws away card removal and the multi-dimensional structure of real ranges, and it is not safe-resolving (no gadget, no blueprint-CFV constraint; ranges come from the routed blueprint's reach, then "confidence flattened"). The 13 ms / 400-iteration latency numbers are therefore for a much smaller problem than a real river solve.

**What a real river subgame costs on an M1 [V for the kernel].** With ≤1326 combos per player and ~20–50 nodes, one CFR+ iteration is a few ×10⁵ flops, well within a 250 ms clock for several hundred iterations. The one non-trivial piece is the showdown utility with blockers in O(n) rather than O(n²). I validated this kernel in Python against brute force (1,081 combos with many ties, max error 5.7·10⁻¹³):

```rust
// cham-search/src/showdown.rs (UNVERIFIED, port of the validated Python kernel)
/// hands sorted ascending by strength; rank[i] equal for ties.
/// out[i] = Σ_j opp_reach[j] · sign(rank_i − rank_j) · [hands i and j share no card]
pub fn showdown_cfv(hands: &[[u8; 2]], rank: &[u32], opp_reach: &[f64], out: &mut [f64]) {
    let n = hands.len();
    let mut win = vec![0.0; n];
    let (mut below, mut below_c) = (0.0, [0.0f64; 52]);
    let mut i = 0;
    while i < n {
        let mut j = i; while j < n && rank[j] == rank[i] { j += 1; }
        for k in i..j { let [a, b] = hands[k]; win[k] = below - below_c[a as usize] - below_c[b as usize]; }
        for k in i..j { let [a, b] = hands[k]; let r = opp_reach[k];
            below += r; below_c[a as usize] += r; below_c[b as usize] += r; }
        i = j;
    }
    let (mut above, mut above_c) = (0.0, [0.0f64; 52]);
    let mut i = n as isize - 1;
    while i >= 0 {
        let mut j = i; while j >= 0 && rank[j as usize] == rank[i as usize] { j -= 1; }
        for k in (j + 1)..=i { let [a, b] = hands[k as usize];
            out[k as usize] = win[k as usize] - (above - above_c[a as usize] - above_c[b as usize]); }
        for k in (j + 1)..=i { let [a, b] = hands[k as usize]; let r = opp_reach[k as usize];
            above += r; above_c[a as usize] += r; above_c[b as usize] += r; }
        i = j;
    }
}

/// Fold terminals need Σ_j reach[j]·[disjoint] by inclusion–exclusion:
/// total − card_tot[a] − card_tot[b] + reach[hand i itself]
```

**Vector-form river CFR+ outline:** for each node keep `regret[action][hand]` and `strat[action][hand]`; pass reach vectors down, CFV vectors up (hero nodes: Σ_a σ_a·v_a; opponent nodes: sum of children); terminals call the kernel (showdown) or the fold formula.

**Making it safe.** Replace "ranges from blueprint reach, flattened" by the Burch/Brown–Sandholm gadget: at the subgame root the opponent chooses per hand *Follow* (enter the subgame) or *Terminate* and receive the blueprint's counterfactual value v_bp(h) for that hand. The solved subgame then guarantees the opponent gets at most v_bp(h) per hand, so the combined strategy is no more exploitable than the blueprint (up to approximation error in v_bp). On the river the blueprint CFVs can be computed exactly by evaluating the blueprint's strategies over the same tree (1326 key lookups per node, a few ms). Start with *unsafe* re-solving to see whether the vector solver helps at all, then add the gadget.

**Beyond the river.** The repo's `v8-leaf-value-net` proposal (DeepStack-style value net, needs `candle`) is the principled route to turn/flop solving but is a multi-week effort and a constitution amendment. A cheaper intermediate: solve the **turn** with the vector solver, using the exact river solve (or a river CFV table indexed by bucket) at depth-limit leaves, then measure whether it beats blueprint-only play in duplicate against the corrected BR. Do the river first.

---

## 4. Compute reality on the M1 and what to train

**Memory is not binding; throughput is.** Using the repo's own numbers:

| Setup | infosets | iterations | wall | visits/infoset | Note |
|---|---:|---:|---:|---:|---|
| tiny, 5M serial | 21k | 5M | 1.66 h | 238 | |
| tiny, 20M Hogwild×4 | 21k | 20M | 1.70 h | ~950 | f32 sums at the margin (F4) |
| full (really 16 river bins), 9M | ~380k/expert | 9M | 3.6 h | 24 | |

The repo's own comparison shows that visits per infoset matter more than abstraction size at these budgets (tiny at 238 visits beats full at 24 visits). The practical rule: **choose the abstraction whose infoset count × target visits fits your calendar**.

Rough budgeting (my estimate, not measured): a richer tree with ~3·10⁵ infosets at a target of ≥200 visits needs ~6·10⁷ iterations. At a plausible 1k it/s for the richer tree on 4 workers, that is ~17 hours, an overnight-to-one-day job per blueprint. The whole bundle (robust + experts) multiplies that, so **drop the four expert blueprints from the critical path** until a router can be trusted (§5.4) and spend that compute on a single stronger robust blueprint.

**Profile before optimising.** Run `samply`/`cargo flamegraph` on `train-bp` for 60 s. Items to look for: env-var lookups and Vec allocations (F9), `river_equity` cache misses (one equity evaluation is ≈990 `evaluate7` calls ≈ 30 µs), key hashing. I have not profiled, so I will not claim a speedup factor. A 2–3× win from F9 alone would not surprise me, but treat that as a guess.

**GPU use.** The repo's wgpu/Metal track is excellent for **table building** (turn EHS in 1.4 h). The same machinery could produce better flop/turn features (potential-aware / EMD histograms) cheaply. The existing EMD-exact build (`exp-017`) showed only a +5% audit-ratio improvement at higher sample counts, so treat bucket quality as second order until the tree (F6) and metric (F1) are fixed.

---

## 5. Recommended architecture and roadmap

### 5.1 Target design (what "competitive vs Nash" can mean on this hardware)

1. **Blueprint:** one robust MCCFR policy on a real tree (F6), with corrected averaging (F3/F4) and a defensible training algorithm (F5), trained for ≥200 visits/infoset.
2. **Real-time solving:** vector-form river solving with card removal, then turn, with the safe gadget (F10), seeded from the blueprint's ranges.
3. **Translation** in and out of the abstraction (F6c).
4. **Exploitation as a bounded deviation**, never a replacement (§5.4).

Against a Nash opponent you should expect roughly "break-even minus abstraction/translation losses". Beating Slumbot-class bots by a margin on one M1 is unlikely; being close to them is the realistic aim, and the metric work in Phase 0 is what tells you how close.

### 5.2 Phases and gates

| Phase | Work | Gate to proceed |
|---|---|---|
| **0 — Measure** (1–2 d) | F1 (tabular BR + learned exploiter), F2 (independent sparring partner, Slumbot probe), fix `bb/100` conversion | Corrected BR number for the current shipped robust policy, with CI, recorded in the ledger |
| **1 — Fix the trainer** (2–4 d) | F3, F4 (f64 sums), F5 (Linear MCCFR / DCFR(1.5,0,2) as arms), F7, F9 | A/B at fixed iterations: new trainer's corrected-BR < old by more than the CI; 3 seeds |
| **2 — Real tree** (3–5 d + overnight jobs) | F6 (cap, translation, config), F8 (rebuild buckets), rich-tiny at equal visits | Corrected BR of rich-tiny < tiny at matched visits/infoset; fallback-rate < 2% on a pool of *off-tree* bettors |
| **3 — Search** (1–2 wk) | F10: vector river, then safe gadget, then turn | Duplicate match vs blueprint-only is positive with CI; corrected BR of blueprint+resolve ≤ blueprint |
| **4 — Exploitation** (1 wk) | §5.4 | Loses ≤ ε bb/100 to the Nash sparring partner and the learned exploiter while still winning on the scripted pool |

### 5.3 What to stop doing

* Tuning against the clairvoyant LBR (eps floors, DCFR α<1, delay0/avguniform). The docs already show it does not transfer to the ladder.
* Treating the ladder mean as strength. Report it per opponent; never as an aggregate headline.
* Scaling the 4-expert bundle before the router gate passes.
* Comparing abstractions at unequal visits per infoset.

### 5.4 Safe exploitation (so the exploiter doesn't lose to Nash)

Right now the exploiters are one-sided best responses to scripts, so by construction they are exploitable. Two measurable controls:

1. **Per-expert exploitability budget.** For each expert, measure the corrected BR (F1). Allow routing weight on it only up to a level where `weight × exploitability_expert ≤ budget` (e.g., budget = 50 mb/hand).
2. **Open-set rejection in the router.** A router that always picks a class cannot say "this looks like Nash". Add a "none of the above → blueprint" output; gate on its calibration (ECE ≤ 0.15 is already the repo's gate; it has never passed). Until it passes, ship `robust-only` (with search) as the default, and use experts only under an explicit flag.

---

## 6. First-week experiment checklist

| # | Experiment | Expected outcome / kill criterion |
|---|---|---|
| E1 | Corrected tabular BR on `par-5M` robust vs the old LBR | Absolute value drops a lot; ranking across 500k/5M/20M variants may change. If rankings match, the old conclusions survive |
| E2 | `self-exploit --train-iters 2e6` on `agent-honest` | Real-engine earn rate with CI vs shipped agent. This is the first honest headline |
| E3 | Shipped agent vs independent robust sparring partner (2× iters), 20k duplicate deals | Negative and large ⇒ the exploiters are the problem; run again with `robust-only` |
| E4 | F3+F4 fixed trainer, 5M tiny, 3 seeds, same corrected metric | Lower BR and lower seed variance, or the hypothesis in F4 is wrong |
| E5 | Linear-MCCFR-no-floor vs DCFR(1.5,0,2) vs fixed-RM+ baseline | Keep the best by corrected BR; kill the rest |
| E6 | Off-tree pool: opponents betting 0.25/0.6/1.5 pot; fallback rate before/after translation | Fallback rate should fall toward 0 and ladder results on those opponents should rise |
| E7 | Rich-tiny tree at ≥200 visits/infoset | Compare to tiny at equal visits using E1's metric. This replaces the 50k-iteration conclusion |
| E8 | Vector river solver vs the class-based solver on 1000 sampled rivers, with a brute-force exploitability check | Solver exploitability → 0 with iterations; blockers matter in a measurable fraction of spots |

---

## Appendix A — Reproductions I ran (Python)

| Script | What it shows | Result |
|---|---|---|
| `kuhn.py` | Averaging at opponent nodes vs traverser nodes (+/− reach) on Kuhn, exact exploitability by enumerating all 64 pure strategies | 0.0093 / 0.0107 / 0.0106 mean (100k iters, 3 seeds): no difference |
| `kuhn_lbr.py` | Per-deal-max "LBR" vs exact exploitability on a ~Nash Kuhn strategy | 0.550 vs 0.0048 |
| `leduc.py` | Same schemes on Leduc (exact BR by infoset-aggregated sweeps) | 60k iters × 8 seeds: opp-node mean 0.189 (max 0.197); traverser-node mean 0.283 (max 0.645); 120k × 3 seeds: 0.135 vs 0.222 |
| `f32avg.py` / `f32big.py` | f32 CAS accumulation with snapshot renorm vs f64 | Drifting iterate: renorm gives (0.866, 0.114, 0.020) vs exact (0.725, 0.255, 0.020); no-renorm f32 matches exact. Stationary case: no difference, even at 4M iterations |
| `kernel.py` | O(n) showdown kernel with blockers vs brute force; pseudo-harmonic vs the repo's weights | max error 5.7·10⁻¹³ over 1,081 combos with ties; repo weights give 0.963 / 0.895 vs exact 1.0 / 0.750 |

**Limits of these checks.** They are toy games (Kuhn, Leduc with a 2-raise cap), written by me in Python, not the Rust production path. They support the *direction* of F1, F3 and F4, and they validate the kernel and the mapping formula. They do not quantify the effect in HUNL.

## Appendix B — What I did not verify

* I could not compile or run any Rust (no `cargo` in my environment). Every Rust snippet is untested and may need small API adjustments (e.g. `ladder()` accessor on `Encoder`, `store_regret`, `hash01`).
* I did not read the engine, tracker, router training or eval internals in depth, nor the GPU crate.
* I did not verify the claimed throughput of search (13 ms) or the benchmark figures; they are quoted from the repo.
* The assertion that the shipped bundle will "lose heavily to a Nash-class bot" is a judgement from the facts above (coarse tree, degenerate router, exploiter-trained policy, no external anchor), not a measurement. E2/E3 and a real Slumbot probe will settle it quickly.
* Papers cited from memory (Ganzfried–Sandholm 2013, Brown–Sandholm 2019, Lisý–Bowling 2016, Burch et al. 2014, Pluribus supplement): confirm the exact parameterisations before relying on them.
