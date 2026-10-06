# Insight run: safe search + adaptive exploiter (2026-10-06)

All via direct binaries (no nextest) under heavy external load.

## 1. Adaptive exploiter cannot beat the deployed agent

`self-exploit --train-iters 0` (static + adaptive, no BR trainer):

| victim snapshot | static G-SELF (clairvoyant) | adaptive manipulator |
|---|---:|---:|
| tiny-full | +14594 ± 700 | **-122.6 ± 198.1** |
| rlf-g2 | +16194 ± 472 | -122.6 ± 198.1 |

The **adaptive manipulator earns -122.6 ± 198.1** vs the live agent — negative,
within 1 SE of zero. Two independent snapshots agree. The deployed agent is
NOT beaten by an adaptive exploiter (at this budget). (Static G-SELF is the
clairvoyant metric, 6-10x high; ignore its absolute value.)

## 2. Safe search still loses on the scripted ladder

Full 9-opponent pool, clean (no CHAM_SLOT_BUCKET leak):

| | mean mb/seating |
|---|---:|
| search OFF | 8260.7 |
| search ON (gadget) | 6157.2 |
| delta | **-2103.6** |

The gadget makes search SAFE (unit test: opponent gap 27.06 -> 0.013), but
it bounds hero's river play by the BLUEPRINT (robust), replacing the exploit
EXPERT's river decisions. Against scripted bots the expert exploits; the
blueprint-bounded strategy does not. So safety costs exploitation — by
design, not a bug.

## The honest bottom line

- The **gadget works** (bounded, verified).
- Search, even safe, is **neutral-to-negative on the scripted ladder** — it
  cannot win there because the ladder rewards pure exploitation.
- The **adaptive exploiter cannot beat the deployed agent** — a positive
  signal for the shipped policy.
- Search's value would show ONLY vs an adaptive opponent, measured ON vs
  OFF. Neither run measured that (the adaptive test used OFF).

## Caveats

- Single seed; adaptive result is 1 SE.
- The queue's first ladder run was contaminated by a CHAM_SLOT_BUCKET=1 leak
  (26.6% fallback); the clean re-run is the one quoted.
- Heavy external load (pkr-sota trainers) slowed everything; wall times are
  pessimistic.
