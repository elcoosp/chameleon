# cham-router

The classifier + switching policy (SPECS/05). 20 tracker-only features
(hand-frozen — no within-hand leakage, no blueprint-circular dims), softmax
model, session-clustered binary `.rbin` datasets (A/B-dev/B-test/C splits).

Runtime weights, once per hand: `p` = posterior, `w_inst = norm(p^(1/T))`
(sharpening), `w = α·w_inst + (1−α)·w_prev` (hysteresis), drift shield toward
robust. The behavioral mixture itself is reach-weighted and applied by
cham-agent: `σ_mix(a|i) ∝ Σ_k w_k·π_k(i)·σ_k(a|i)`.

Gates: top-1 B-dev ≥ 0.80, per-class recall, ECE ≤ 0.15 on B-test AND family C.
