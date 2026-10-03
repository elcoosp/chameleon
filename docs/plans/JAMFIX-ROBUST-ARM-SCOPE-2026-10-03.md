# Scoping the "robust-arm" jamfix fix (2026-10-03)

**Request:** fix the jamfix regression at the robust arm (the correct
layer, per the expert-mixture negative result).

**Finding:** there is no "train robust against jamfix" retrain. Robust
mode is pure self-play.

## Why

`traversal.rs`: at an opponent node in Robust mode, the opponent is

    "the other seat's CURRENT strategy sampled from its own rows"

Robust has no external-opponent hook. `TrainMode::Robust` carries no
opponent field (`trainer.rs`: `TrainMode::Robust => None` in the
opponent-id match). Giving it an external opponent would convert the
robust arm from a self-play Nash fallback into an exploit policy — a
conceptual change, not a retrain.

## So what is the jamfix fix, actually?

Three honest options, in order of cleanliness:

### Option A — abstraction coverage (recommended)
jamfix's failure is that its shove lines produce infoset keys the
experts have no rows for, so robust covers. The real fix is to make the
abstraction *cover* those lines: the F6c work (richer ladder +
slot-index bucket), already scoped in
`F6C-SIZE-BUCKET-DESIGN-2026-10-01.md` and
`SIZE-BUCKET-DEGENERACY-2026-10-02.md`. A shove is an off-tree size
under the tiny ladder; the rich ladder + translation is what brings it
on-tree.
**This is the correct fix, and it is already the F6c roadmap.**

### Option B — a shove-aware expert (limited value)
Train one of the 4 experts (e.g. station, which is closest to a
shove-heavy type) against `mix:<w>:arch:station~jamfix`. This is what
the negative experiment tried with nit. It failed because jamfix
routes to expert 0 in the full pool but the expert still *misses* the
shove rows (coverage, not policy). A shove-aware expert only helps if
it has rows for the shove keys — back to Option A.

### Option C — give Robust an external opponent (largest change)
Add an opponent field to `TrainMode::Robust` and a parallel-safe
external-opponent path. This turns robust into "self-play + exploit
vs a chosen opponent", which blurs the Robust/Exploit distinction the
architecture is built on. Not recommended.

## Recommendation

**Leave jamfix accepted for now.** It is one out-of-family shove-bot
opponent, −1193 mb/seating, against an 8/9-opponent, +2989-mean win.
The correct fix (Option A) is the F6c abstraction-coverage work, which
is already scoped and does not need a separate jamfix effort. Revisit
jamfix specifically only if a shove-heavy opponent class becomes a
target.

Source: `JAMFIX-REGRESSION-2026-10-03.md`,
`JAMFIX-MIX-EXPERIMENT-NEGATIVE-2026-10-03.md`.
