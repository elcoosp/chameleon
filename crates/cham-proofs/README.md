# cham-proofs

The M-1 "prove it" gate (SPECS/11): self-contained micro-implementations that
import NOTHING from the workspace, so proofs cannot pass by accident of shared
code. `chameleon verify --proofs` runs all four:

- **P-1** ES-MCCFR on Kuhn reaches the Nash value (−1/18 bb/hand).
- **P-2** one-sided exploit training hits the exact best-response EV vs a
  fixed scripted caller.
- **P-3** the reach-weighted mixture beats the best single specialist and
  reaches ≥ 90 % of the exact Bayes-optimal policy's EV.
- **P-4** the FMBR machinery matches enumerative-LP matrix-game solutions.

Kept forever as a regression suite.
