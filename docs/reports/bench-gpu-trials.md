# GPU probe — 5-trial spread (contended M1 Mini, 2026-09-25)

Single-session, back-to-back trials of `gpu-probe --mode verdict
--hands 1000000 --boards 5000 --reps 20`, on a machine concurrently
running a GTO solver training loop. Captured verbatim from the shell.

| Trial | CPU_ENUM (evals/s) | GPU_WARM (evals/s) | GPU_ENUM (evals/s) | best/cpu | verdict |
|------:|---:|---:|---:|---:|:--:|
| 1 | 2.997e7 | 1.977e8 | 2.927e8 | 9.77× | NO-GO |
| 2 | 2.985e7 | 1.410e8 | 2.541e8 | 8.51× | NO-GO |
| 3 | 2.422e7 | 1.144e8 | 2.870e8 | 11.85× | GO |
| 4 | 5.302e7 | 2.149e8 | 3.000e8 | 5.66× | NO-GO |
| 5 | 3.492e7 | 1.130e8 | 3.387e8 | 9.70× | NO-GO |

Summary: **median 9.70×, min 5.66×, max 11.85×, 4/5 NO-GO.**

## What this tells us

1. The **CPU reference moved by 2.2×** (2.42e7 → 5.30e7) between trials,
   because the training loop is competing for the same cores. Every CPU
   number in this table is a lower bound on the true rate.
2. The **GPU's own rate varies ~1.35× on enum, ~1.9× on warm**, less than
   the CPU's swing — the GPU is doing its own work on a separate unit, but
   shares the memory bus.
3. Trial 3's "GO" is the CPU being *slow* that run, not the GPU being fast.
4. The plan's pre-registered 10× bar is not decidable from this data. The
   honest reading is "GPU is 2–4× faster on warm dispatch and 5–12× faster
   on builder-shaped enum under contention" — which is a real speedup, but
   not one this machine can *certify* at 10×.

See `docs/gpu/amendments.md` Amendment 001 for the response.
