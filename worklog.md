# worklog

Chronological engineering log. One section per task; include gates run and
numbers observed. Required by docs/GPU-PLAN.md Part I step 7.

## G0.0 Machine preflight + baseline capture

- host: Macmini9,1 / Apple M1 / 8-core GPU / Metal 4
- memory: 16 GB
- disk free: 17 GB
- toolchain: rustc 1.98.1 (48a229cea 2026-09-01), cargo 1.98.1 (797e8a9bc 2026-08-05)
- metal compiler: present (/var/run/com.apple.security.cryptexd/mnt/com.apple.MobileAsset.MetalToolchain-v17.5.188.0.stjtEu/Metal.xctoolchain/usr/bin/metal)
- preflight dump: docs/gpu-preflight.txt

Baseline bench capture: see bench-before-gpu.txt (run separately, long).
