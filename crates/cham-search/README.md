# cham-search

Inference-time river solving (SPECS/06). Extensive-form river subgames over
prior-weighted ranges (pseudo-harmonic off-tree mapping + visit-confidence
flattening, card removal, showdown memoization).

Three solvers behind one `solve()`:
- **FMBR** — best response to the prior strategy (max exploitation),
- **RNR(p)** — opponent plays the prior with prob p, freely with 1−p
  (CFR+ on the free branch) — the principled safety knob,
- **ReachGadget** — the conservative robust arm.

`budget.rs` is the ONLY `Instant::now()` outside cham-rec; evaluation runs
`Iterations` budgets (byte-identical). Independent-oracle suite: Kuhn/Leduc
equilibria + in-repo LP river spots (postflop-solver is a dev-time-only AGPL
procedure, never linked).
