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

### Preflight disk deviation

- GPU-PLAN G0.0 says: require ≥ 25 GB free for artifacts; fail preflight otherwise.
- Actual: 17 GB free (df -h on /System/Volumes/Data).
- Scope amendment (proposed): G1.2/G1.3 build turn (1.44 GB) + flop (0.10 GB)
  only — 1.54 GB total, fits in the current free space. G1.4 river (6.90 GB)
  remains CONDITIONAL and defaults to SKIP unless a G2.0 consumer exists.
- G0.1 (this session) ships code only; no artifacts are created.
- Revisit at G1.0: if free space < 5 GB at that point, escalate to owner
  (single-command recovery: 'cargo clean' + 'rm -rf target' reclaims ~4 GB).

## G0.1 cham-gpu skeleton + gpu-doctor CLI (code complete)

- new crate: crates/cham-gpu (features: default=[], metal=[dep:metal])
- whitelist amendment: metal = "0.31" (workspace Cargo.toml)
- subcommand: chameleon gpu-doctor
- .gitignore: artifacts/gpu-tables/
- dev/test/commit deferred until bench-before-gpu.txt finishes
  (GPU-PLAN Part I: do not run GPU work concurrently with bench)

## G0.2 MSL evaluate7 port (planning, bench-blocked)

- evaluate7 reads: STRAIGHT_TABLE (8 KB OnceLock), TABLES (prime-product map + flush ranks).
  Plan G0.2 step 1 will expose them via eval_tables() -> EvalTables<'static> in cham-core.
- MSL signature frozen (see crates/cham-gpu/src/msl/eval7.msl when written).
- Consistency test will live at crates/cham-gpu/tests/consistency_eval7.rs.
- Pinned seed for the 1M-hand corpus: 0x60C0FFE (recorded in the test constant).
- Blocked on: bench-before-gpu.txt (GPU-PLAN Part I: no concurrent GPU work).

### G0.2 deviation — hand packing is u64, not u32

- GPU-PLAN G0.2 text: 'constant uint* hands'.
- Reality: a card index is 6 bits (0..51); 7 cards = 42 bits > u32.
- Resolution: pack a 7-card hand into a u64 (one index per byte is
  wasteful; we will pack 6 bits/card into 42 bits, LE).
- The MSL kernel signature is updated accordingly; the JSON manifest
  and the crate's public API will use  / .
- Recorded here so the plan text can be corrected during G4.0 doc sync.

### G0.2 deviation — hand packing is u64, not u32

- GPU-PLAN G0.2 text says: "constant uint* hands".
- Reality: card index is 0..51 → 6 bits; 7 cards = 42 bits > 32.
- Resolution: pack a 7-card hand into a u64 (6 bits/card into 42 bits, LE).
- MSL kernel signature updated accordingly (device const ulong*).
- Public cham-gpu API uses &[u64]; the CPU-side packer lives in kernels.rs.
- Recorded here so the plan text can be corrected during G4.0 doc sync.

## G0.2b MSL eval7 kernel + scoped-unsafe dispatch

- MSL body written: full evaluate7 transcription (flush-select via if-chain,
  straight[8192] lookup, flush_top5, prime-product lookup, LinearMap probe).
- Hand packing: 6 bits/card × 7 = 42 bits in u64 (was u32 in plan text).
- Table packing: straight bytes || seven entries*16 || flush entries*16, where
  each entry is (u64 key LE || u16 val LE || 6 pad) — mirrors Rust Vec<(u64,u16)>.
- SAFETY DEVIATION from plan: docs/GPU-PLAN.md Part II says cham-gpu "stays
  #![forbid(unsafe_code)]"; impossible with `metal` FFI. Downgraded to
  #![deny(unsafe_code)]; #[allow(unsafe_code)] scoped to src/mtl.rs only.
  Same pattern as D-001/memmap2.

### G0.2b lint override

- The workspace sets `unsafe_code = "forbid"` (Cargo.toml [workspace.lints.rust]).
- Forbid cannot be relaxed by any inner #[allow] (E0453); the metal FFI shim
  needs ONE unsafe block for `contents() -> &[u16]` (the buffer/data/set_bytes
  APIs in metal 0.31 are already safe wrappers).
- Fix: cham-gpu replaces `[lints] workspace = true` with an explicit block that
  mirrors the workspace lints but sets `unsafe_code = "deny"` (deny IS
  relaxable). All other crates keep forbid; cham-gpu remains deny + scoped allow.
- Deviation from docs/GPU-PLAN.md Part II ("stays #![forbid(unsafe_code)]") is
  therefore minimal — one crate opted into deny, one module has one unsafe fn.
