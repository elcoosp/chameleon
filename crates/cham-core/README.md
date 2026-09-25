# cham-core

Foundation crate (SPECS/01): cards, hand evaluation, HUNL rules engine, RNG,
observables, and the leak-proof agent surface. Zero policy logic; depends on
nothing but `rand`/`rand_chacha`/`serde`/`arrayvec`.

- `eval/` — `evaluate7`/`evaluate5` (1..=7462 ordering), `best5`, exact and MC
  equity vs a `Range` (1326 bits = 21×u64), golden-vector + property tested.
- `engine/` — `State` is `Copy` (≤ 128 B, no heap): `legal_actions` fills a
  caller `ArrayVec<LegalAction, 12>`; canonical `Action` (all-in = Bet/Raise to
  cap); min-raise / short-all-in-no-reopen / uncalled-return semantics pinned
  by named tests. `HandHistory` (full info) vs `PublicHistory` (agent-visible,
  folded holes impossible — invariant I9).
- `obs.rs` — `Observables<'a>` borrowed view + the `Agent` trait with the
  analytic `action_probs` oracle (default `NotProbabilistic`).
- `rng.rs` — one RNG everywhere: `ChaCha8Rng`, explicit `&mut Rng`, `child()`
  seed derivation.
