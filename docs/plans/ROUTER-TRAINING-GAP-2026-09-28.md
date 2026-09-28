# Router training gap (2026-09-28)

## What I found

Two things at once:

### A real bug: `train-router` never produced a loadable router

`train-router` wrote `model.bin`. Every agent path
(`hero.rs`, `play.rs`, `probe.rs`, `audit_buckets.rs`) reads `router.bin`.
So even after `train-router` succeeded, the bundle was never picked up —
the fallback `SoftmaxModel::new(20, 4)` (deterministic small init) served
every mixture measurement we have ever taken.

Fixed: `train-router` now writes BOTH `model.bin` and `router.bin`.
Verified: the mixture ladder re-runs with materially different numbers
after installing the freshly-trained `router.bin` in the bundle.

### A larger gap: the router is trained on synthetic data

`collect.rs` — the subcommand that produces the training dataset — is
explicitly marked as a stub:

    /// Deterministic feature synthesizer (stand-in for the full instrumented
    /// session driver; the real producer is `ladder --instrument` at M3).
    struct TrackerStub { ... }

    fn next_features(&mut self, rng, arch) {
        ...
        // class signal: archetype k elevates its signature EWM stat
        let sig = 1 + (arch % 4);
        f[sig] = (f[sig] + 0.45).min(1.0);
        f
    }

The "trained router" reported `top1_b_dev = 1.000, top1_b_test = 1.000,
recall = [1.0, 1.0, 1.0, 1.0]`. A real classifier over real opponents'
behavior does not hit 100 %. That perfect score is the signature of
training on a feature vector that literally encodes the label.

## What the numbers show (with the caveat above)

| opponent | mixture (no router) | mixture (synthetic router) | argmax (no router) |
|---|---:|---:|---:|
| arch:nit | +1 692 | +1 593 | +2 536 |
| arch:tag | +2 501 | +3 185 | +3 671 |
| arch:lag | +4 360 | **+5 567** | +4 042 |
| arch:station | +4 366 | **+12 746** | +6 691 |
| callbot | +10 157 | +11 170 | +15 195 |
| jamfix | −449 | −387 | **+4 753** |
| pnash | +353 | +222 | **+4 134** |
| famB:tag | +1 186 | +635 | **+2 850** |
| noisy | +3 310 | **+4 770** | +4 178 |

The synthetic-trained mixture beats no-router mixture on 6/9 (station is
a large win: +4 366 → +12 746). But **argmax still wins on 6/9 and by a
larger margin on the aggregate** — including a +4 753 vs −387 swing on
jamfix and a +4 134 vs +222 swing on pnash.

## What this means

1. The router file-path bug was **silent and total**: no measurement
   taken before today was ever consulting a router.
2. Fixing the path gets us a *synthetic-trained* router, which helps a
   bit but does not beat argmax.
3. The **real** gap is that `collect` synthesizes data. Until there is a
   real instrumented producer, the router cannot learn the actual
   feature patterns of the archetypes in the pool.
4. **Argmax remains the shipped default** until a real-data router beats
   it in a head-to-head.

## What unblocks a real router

The SPECS call for `ladder --instrument` — run the ladder, record the
tracker's live feature vector at each decision, label the row with the
session's opponent id, write the `.rbin`. That is a real but bounded
piece of engineering:

1. `ladder` writes per-decision features during a match (it already has
   a `DecisionTrace`; the tracker features are computed in
   `ChameleonAgent::start_hand_if_needed`).
2. Rows are session-clustered by opponent id (one session per match).
3. Output is a `.rbin` in the same format `collect` currently writes.

Once that exists, `train-router` sees real data, and the mixture has a
chance. Until then, argmax is the honest choice.
