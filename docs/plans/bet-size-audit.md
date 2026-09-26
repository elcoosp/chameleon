# Bet-size abstraction audit A4 (roadmap §3.5) — execution checklist

Run AFTER §2.3 (exploitability bench) exists; gate every addition on it.

## Constraint

`ArrayVec<LegalAction, 12>` caps the action set per infoset. Every added
size multiplies blueprint state count — each candidate below must clear
the 16 GB memory fence via the byte-budget table (Constitution §3) AND
move the §2.3 benchmark beyond its noise floor, or the leaner tree stays.

## Procedure (per street: preflop / flop / turn / river)

1. Record the current geometric sizing template (sizes as pot fractions +
   jam) from the encoder config.
2. For each candidate added size (one at a time — no bundles):
   a. Estimate state-count multiplier from the abstraction config
      (branching factor × street reach).
   b. Check against measured encode rates (`encode_flop` 7.09M keys/s,
      `encode_river` 5.25M keys/s, P3 gate) — encoding must stay under gate.
   c. Train the tiny profile, read abstraction-local exploitability off the
      §2.3 bench.
3. Keep the size iff (b) passes AND (c) improves beyond noise. Kill
   criterion: exploitability deltas below the variance floor → keep the
   leaner tree, close the audit for that street.

## Sequencing note

A4 sits behind A2 (§4.1): size additions multiply the state count OF the
bucket partition, so audit sizes against the rebuilt (exact-feature)
abstraction, not the MC-feature one — otherwise the audit is re-measured
after the rebuild anyway.
