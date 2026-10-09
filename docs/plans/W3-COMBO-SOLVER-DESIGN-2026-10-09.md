# W3 combo-level solver — design (2026-10-09)

**Why this doc exists.** On 2026-10-09 an attempt to write the combo
solver directly as a single Rust file failed to compile and, on
inspection, contained a stubbed `hero_to_villain` that admitted in a
comment it was approximating. It was reverted. This doc lays out the
design the code needs before another attempt.

## What W3 is (from the plan, §2 target architecture)

> Replace the toy search with a **combo-level, position-correct, safe
> real-time solver built from the same kernels**.

The plan's C-1 finding is that the current river search collapses each
range to 3 classes, which cannot represent a balanced strategy. W3 keeps
both ranges as explicit combo vectors (up to 1326 each) and solves with
the same O(n) card-removal kernels `fullgame.rs` uses.

## Reuse from the walker

The full-game VBR walker (`cham-search/src/fullgame.rs`) is the
reference implementation. Its structure is:

    walk(node, st, seq, hero_reach, villain_reach, last_action, last_actor)
        -> (hero_cfv, villain_cfv)

Two crucial properties the combo solver needs to replicate:

1. **Both CFVs are weighted by the OPPONENT's reach.** Hero's CFV at a
   node is `sum_j villain_reach[j] * disjoint(i,j) * ev(i,j)`; villain's
   is the mirror. The `fullgame` fix at commit `53053f6` was exactly
   this — the original used hero's own reach and produced garbage.

2. **Fold winners are determined by the LAST ACTION**, not by stacks.
   `State::stacks()` at a fold terminal does not reflect pot
   resolution. `fullgame` records `(last_action, last_actor)` through
   the recursion and reads it at the terminal.

Any combo CFR that does not inherit these two properties will be wrong
in the same way the pre-`53053f6` walker was.

## What is genuinely new versus the walker

The walker takes a **max** at hero nodes (it computes BR). CFR takes a
**regret-weighted sum** and updates a per-infoset regret table. That is
the whole delta, plus:

- The tree is a **subgame** (river only), not the full 4-street game.
  Smaller, but the terminal set is different: no runouts, no earlier
  streets.
- The safe-resolving **gadget** (Burch/Brown–Sandholm) sits at the
  subgame root: the opponent may "terminate" and receive their
  blueprint CFV. The plan requires it (W3 gate: "VBR(blueprint+resolve)
  ≤ VBR(blueprint)").

## Tree

River-only, HU, hero = BB acts first postflop (position-correct). Ladder
per the abstraction config: hero {check, bet(f), jam}; villain
{check-behind, bet(f), jam} after a check, {fold, call, raise-to-jam}
facing a bet. Reduce the tree to the tiny ladder's actual action set
before writing code; do not hard-code.

## Algorithm

External-sampling CFR+ is the wrong tool here (its variance needs
millions of iterations per subgame). Use **vanilla CFR over the small
river tree** with the exact kernels — the tree is small enough (tens of
nodes) that enumerating both players' actions every iteration is
feasible.

Per iteration:
1. Compute `hero_reach[i] = w_hero[i]` at root, `villain_reach[j] = w_v[j]`.
2. Recurse: at hero nodes, for each action, sigma = regret-match, and
   propagate `hero_reach * sigma[a]`. At villain nodes, same for
   `villain_reach`. At terminals, use the kernels (see "Reuse").
3. Update regret: `R[a] += cfv[a] - node_cfv`, CFR+ clamp at 0.
4. Accumulate strategy sum with linear weighting `t`.

At convergence, `strategy_sum / total_weight` is the average strategy.

## Gadget

Root prepend: an opponent decision `[terminate | play]`. "Terminate"
pays the opponent `v_bp[j]` for combo `j` (their blueprint CFV, which
the caller supplies from `BlueprintPolicy`). "Play" enters the tree.
This bounds the resolved strategy's exploitability by the blueprint's
(Burch/Brown–Sandholm 2014). The gate is `VBR(resolved) ≤ VBR(blueprint)`.

## Testing (all three must pass before this ships)

1. **Single-combo Nash.** One combo each side, deterministic showdown.
   The equilibrium is analytic (fold when losing, call when winning).
   Same shape as `vbr_nash_toy.rs` — that test is the template.
2. **VBR ≤ blueprint.** Solve with the gadget on a real board with real
   ranges, then run `fullgame_vbr` against the resolved strategy. The
   result must be ≤ the blueprint's VBR. This is the W3 gate.
3. **Exploitability floor.** Without the gadget, the solver must find a
   strategy whose VBR is ≤ the blueprint's within iteration noise — else
   the solver is worse than doing nothing.

## Estimated effort

Per the plan, 8–12 agent-days. The reverted attempt spent ~1 agent-hour
on the wrong shape. The design above is ~1 more day; the code ~3; the
tests + tuning ~3; the live wiring (ranges from tracker, latencies) ~3.

## Do not do this next without reading the walker again

Every bug this session in the walker was in the reach/terminal math.
Copy the walker's shape; do not reinvent. If the solver's `hero_cfv`
or `villain_cfv` at a terminal does not have the same formula as
`fullgame.rs::terminal_ev`, stop and diff.
