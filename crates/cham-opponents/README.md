# cham-opponents

All opponents implement `cham_core::obs::Agent` (SPECS/03). Scripts are
probability oracles: exact equities, fresh per-decision draws, so
`action_probs` is analytic — a hard requirement for one-sided CFR reach
products.

- `archetype.rs` — nit/TAG/LAG/station point + jittered scripts.
- `family_b.rs`, `perturbed.rs`, `noisy.rs` — the OUT-OF-FAMILY evaluation
  trio (anti-circularity): structurally different scripts, tilted robust
  policies, human-like noise wrapper.
- `baselines.rs` — CallBot/RaiseBot/JamBot/RandomBot/FishBot; `drift.rs` —
  SwitcherBot; `factory.rs` — `OpponentSpec` parse/id round-trips.
