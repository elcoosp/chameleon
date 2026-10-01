# Stale release binary caused a false F1 A/B (2026-10-01 16:18)

## What happened

After landing the F1 follow-up (`cb08041`, 16:15), I launched the F1
A/B ladder (`scripts/ladder-search-ab-2026-10-01.sh`) without
rebuilding `target/release/chameleon`. The binary's mtime was 13:55 —
before the F1 commits at 16:04 and 16:15.

The `--search` flag does not exist in the 13:55 binary, so the second
run of the A/B (`ladder --fast --agent full --search`) would have
failed at argument parsing. The first run (`search OFF`) was a
legitimate measurement, but the pair would have been non-informative.

## The fix

Killed the A/B, ran `cargo build --release -p cham-cli`, verified
`--search` appears in `ladder --help` and `probe --help`, relaunched.

## Second occurrence of this class today

- 2026-09-30: `full-hedged` missing from TRAINED_AGENTS → CallBot
- 2026-10-01 13:41: stale binary missed `sample-expert` → CallBot
- 2026-10-01 16:18: stale binary missed `--search` → A/B would fail

Each time the fix is "rebuild release after every commit that changes
the CLI surface." The permanent fix (partly landed in `d8dd528`:
unknown-agent refusal in ladder/probe) does not catch missing *flags*
because Clap refuses them at parse time with a nonzero exit — the
pipeline log shows the failure but the *previous* successful run's
result can still look valid.

## Recommendation

Add a version/hash stamp to the CLI startup banner:

    chameleon <version> (built <mtime>)

so the pipeline logs show which binary ran. Cheap and catches the
class. Not done in this session.

## Artifacts

- `scripts/ladder-search-ab-2026-10-01.sh` — the A/B script
- `artifacts/ladder-f1-ab-pipeline.log` — the corrected run
