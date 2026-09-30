# 5M expert retrain: 3 of 4 experts failed silently (2026-09-30)

## What happened

`scripts/retrain-tiny-5M-experts-2026-09-30.sh` (launched 14:46)
trained `nit` correctly (453 KB, ~2h40m serial, finished 17:27), but
`tag`, `lag`, and `station` each "finished" in 6-7 seconds and produced
308-byte stub `policy.bin` files.

The script uses `> log 2>&1` without checking exit codes. Whatever
failed during the tag run didn't stop the loop.

## Root cause (hypothesis)

The script has a skip guard:

    if [ -f "$sub/exploit-$SEED/policy/policy.bin" ]; then
      log "$opp: already trained, skipping"
      continue
    fi

But the log shows `=== training expert: tag ===` — so the guard
did not skip. The training must have started and failed. The stub
files are 308 bytes, which matches the "empty/default" policy.bin
size we saw earlier when the parallel-exploit path produced
`infosets=0`.

**The likely cause:** a wr.sh restart killed the process group while
the tag expert was mid-run. The trainer wrote a stub on early exit;
the script's loop continued with the stale 308-byte file.

## What was laddered anyway

The pipeline assembled `artifacts/agent-honest-5M-experts/` with
nit=real (453 KB), tag/lag/station=stubs (308 B each) and ran
`ladder --fast`. Those results are meaningless and should be
discarded:

- `artifacts/ladder-5M-experts-full.log`
- `artifacts/ladder-5M-experts-mixture.log`

## The fix

Add `|| exit 1` (or `|| return 1`) after every `chameleon train-bp`
call in the retrain script. Also validate the produced `policy.bin`
size (> 100 KB) before proceeding to the next expert.

## The good news

The 19-dim honest router ladder SOTA (from `LADDER-19DIM-SOTA-2026-09-30.md`)
does not depend on this retrain. The `agent-honest-19dim` bundle uses
500k experts + 500k robust + the honest router, and it beats the
synthetic router at both argmax and mixture.

## Related

- `LADDER-19DIM-SOTA-2026-09-30.md` — the current SOTA
- `EXPLOIT-MODE-IS-SERIAL-2026-09-30.md` — why this took 2.5h per expert
- `scripts/retrain-tiny-5M-experts-2026-09-30.sh` — the buggy script
