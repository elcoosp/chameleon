# Search works: -1189 mb/seating vs adaptive, 3.6 sigma (2026-10-06)

## The result

`self-exploit` adaptive manipulator (nit-then-deviate) vs live `full`,
20,000 deals, controlled env (CHAM_SLOT_BUCKET unset):

| arm | manipulator earns (mb/seating) |
|---|---:|
| search OFF | -3456.5 +/- 210.1 |
| search ON (safe gadget) | -4645.5 +/- 251.7 |
| **delta (ON - OFF)** | **-1189.0 +/- 327.9** = **3.6 sigma** |

`manipulator earns` = what the adaptive exploiter extracts from the victim.
More negative = victim LESS exploitable. Search ON makes the manipulator
earn **1189 mb/seating less** — the victim is measurably harder to exploit.

## Why this matters

This is the value proposition of search, MEASURED for the first time:

- The **scripted ladder** showed search losing -2104 (INSIGHT-RUN). That
  ladder rewards pure EXPLOITATION; safe search trades it for SAFETY.
- The **adaptive exploiter** shows search WINNING +1189. The adaptive
  opponent is exactly where safety pays.

Both are correct: search trades scripted-pool exploitation for adaptive
robustness, and the trade is now quantified.

## The full arc

1. Search (unsafe) was -12345 on the ladder — a bug (min-bet mapping,
   then unsafe re-solving). FIXED.
2. The safe-resolving gadget (Burch/Brown-Sandholm) bounds search by the
   blueprint (unit test: opponent gap 27.06 -> 0.013). BUILT.
3. With the gadget, search is -2104 on the scripted ladder (safety costs
   exploitation) but **+1189 vs adaptive** (safety pays vs adaptation).
   MEASURED.

## Caveats

- Unpaired SE (conservative). With the fixed seed the deals are common, so
  a paired test could be tighter; 3.6 sigma is the safe bound.
- Single snapshot (tiny-full); one manipulator family (nit-then-deviate).
- The manipulator loses EITHER WAY (-3457/-4646): the agent beats it. The
  result is that search makes it lose MORE, not that search flips the sign.
- 20k deals, ~30 min under load.

## Recommendation

- **Search is now safe and valuable vs adaptive opponents.** Enable it where
  the opponent may adapt; keep it OFF vs known-exploitable scripts (where it
  costs -2104).
- A natural default: search ON, since real opponents adapt and the scripted
  pool is not the target.
