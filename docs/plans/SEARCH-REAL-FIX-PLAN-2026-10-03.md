# Making search safe AND good — the real fix (2026-10-03)

## Why search loses (measured, not guessed)

`F1-SEARCH-CORRECTED` + the 2026-10-03 re-measurement: search is
**net-negative** on calling-heavy opponents (callbot -12 471,
station -8 354 vs OFF). Two independent causes:

1. **No safety gadget.** A re-solved subgame strategy is an *unbounded
   deviation* from the blueprint. Without a gadget it can be
   arbitrarily worse, and it is.
2. **Heuristic villain range.** `villain_range_from_tracker` maps
   marginals to 3 strength classes; against a station it still models
   villain too strong, so the solver under-bets hero's value.

## The fix — two halves, both required

### Half A: the safe re-solving gadget (in `cham-search`)

Burch/Brown-Sandholm 2014: at the subgame root, give the **opponent**
an opt-out action worth their **blueprint counterfactual value**
(`v_bp(class)`). Then the re-solved strategy guarantees the opponent at
most their blueprint value ⇒ the combined strategy is **no more
exploitable than the blueprint**. Search can no longer lose.

Concretely:
- `Subgame` gains `opponent_optout: Option<Vec<f64>>` — `v_bp` per
  villain class (None = gadget off = today's behavior).
- `Subgame::tree()` gains a **root villain decision** when the gadget is
  on: `[terminate → class-dependent terminal(v_bp[c]), play → hero_node]`.
- A new `TerminalKind::OpponentTerminates` pays `v_bp(villain_class)`
  (class-only, not class-pair — the one new terminal shape).
- `solve()` treats it as an ordinary terminal; CFR+ needs no change.

`v_bp` computation (in `cham-agent`, where the blueprint lives):
evaluate the subgame tree with the **prior strategies** (`prior.rs`) for
both players, accumulate the villain CFV per class. One evaluation pass.

### Half B: blueprint-reach villain range (in `cham-agent`)

Replace `villain_range_from_tracker`'s heuristic with: walk the public
action sequence, reweight villain classes by the blueprint's action
probabilities from villain's seat. The tracker marginals become a
*sanity check* on the derived range, not the range.

(Chosen over F10's combo-level solver: this keeps the class abstraction
and is ~1-2 days, not 1-2 weeks.)

## Order

1. **Gadget** (Half A) — makes search **safe** (≥ blueprint, at worst a
   no-op). Measure: search ON vs OFF should be ≥ 0, not -12000.
2. **Range** (Half B) — makes search **good** (beats blueprint). Measure:
   search ON vs OFF > 0 on callbot/station.
3. Only then consider F10 (combo-level solver) for more gain.

## Retrain-safety

`cham-search` / `cham-agent` are NOT deps of `cham-blueprint`, and the
retrain's ladder step uses the pre-built release binary. So Half A can be
implemented + unit-tested now; the **release rebuild + ladder A/B wait
until the F6c-lite retrain finishes**.

## Non-goals

- Not F10 (combo solver) — that's the follow-up if A+B still leave EV.
- Not shipping `--search` before A+B measure positive.
