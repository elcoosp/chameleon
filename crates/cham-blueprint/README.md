# cham-blueprint

The trainer (SPECS/04). Correct ES-MCCFR: sample chance + opponent actions,
ENUMERATE hero actions; no reach multipliers, no importance weights, no
baselines. Regret-based pruning (Pluribus θ-schedule); delayed linear
averaging; per-iteration seat randomization; bb-normalized values.

- `table.rs` — open-addressing regret table; Deterministic (bit-identical) and
  Hogwild (atomic, interleaving-dependent) backends; f32 renorm rule.
- `modes.rs` — Exploit (one-sided vs analytic scripts), ExploitBayes (belief
  bins in the key), Robust (two-sided CFR+ with linear discounting).
- `warmstart.rs` — key-exact robust warm-start (depth ladder = EXP-006 only).
- `policy.rs` — quantized u8 strategy artifacts (mmap + bytemuck), visit-based
  confidence c = v/(v+64), provenance with blake3 hashes.
- `lbr.rs` — local best response = the honest exploitability number.
