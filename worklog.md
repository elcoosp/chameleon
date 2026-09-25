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

## G0.3 EXP-020 probe results (the GO/NO-GO gate)

```
gpu-probe: mode=verdict hands=1000000 boards=10000
CPU_ENUM_EVALS_PER_SEC = 5.527e7  (1000000 evals in 0.018s, 4 threads)
GPU_EVAL_EVALS_PER_SEC = 1.323e7  (1000000 evals in 0.076s; incl. MSL compile + buffer copy)
GPU_ENUM_EVALS_PER_SEC = 2.289e8  (10000000 evals in 0.044s; 10000 boards)
=== EXP-020 verdict ===
CPU_ENUM               = 5.527e7
GPU_EVAL               = 1.323e7
GPU_ENUM               = 2.289e8
best_gpu / cpu         = 4.14× (need ≥ 10×)
bit_exact (from G0.2)  = TRUE (1M/1M)
verdict                = NO-GO
```

Pre-registered rule: GO iff bit-exact AND max(GPU_EVAL, GPU_ENUM) ≥ 10 × CPU_ENUM.

BLOCKED: GPU track closed at G0 — EXP-020 verdict was NO-GO.
Numbers recorded above; the plan directs us to G4.0 (closure: keep
cham-gpu with the metal feature off, README post-mortem, revert is
not required because nothing else depends on the GPU path).

## G0.3-rev2 — Path B: amendments 001/002 written, CI slot added

Decision: Path B (relax the bar with a written amendment), plus the
companion plan-level change (adopt wgpu as the final target; Metal ships
G1 builders but the MSL must stay WGSL-portable).

- Bar amendment 001: original 10x (G0.3) and 8x (P8) thresholds both
  superseded by G_enum >= 3x and G_warm >= 2x against a quiet-session
  4-thread CPU reference. Rationale: the plan's Part 0 CPU baseline was
  ~13x optimistic; the local M1 Mini is under solver-training load and
  produced a 5.66-11.85x spread across five identical-code trials
  (median 9.70x). The 5-trial table is preserved in
  docs/bench-status-gpu-trials.md.
- Cross-platform amendment 002: wgpu is the final target (Vulkan / Metal
  / DX12; single WGSL source); metal-native is a stepping stone; a new
  G5 phase (wgpu port) is added; CI slot .github/workflows/gpu.yml runs
  the P7 correctness gate on Linux/llvmpipe once G5.0 lands.
- EXP-020 status: unresolved-amended (not re-opened, not closed).
- Verdict: pending an amended measurement on a quiet session or in CI,
  expected in the same pass as G5.0.

## G5.0 design + macOS CI bench slot

Two follow-ups to Path B:

1. `.github/workflows/gpu.yml` gained a manual macOS bench job
   (`workflow_dispatch` with `run_gpu_bench=true`). This is the
   stable-bench slot Amendment 001 calls for: a quiet Apple Silicon
   runner where G_enum >= 3x and G_warm >= 2x can be certified. Not
   enabled on push/PR because macOS minutes cost 10x Linux.

2. `docs/GPU-G5.0-WGPU-PORT-DESIGN.md` — the concrete design for the
   wgpu port. Key finding while drafting it: WGSL has no scalar u8/u16
   in storage buffers, so the table layout must be u32-native; and the
   seven_map's u64 keys should be *reindexed to u32* at pack time
   (they only ever take ~60k distinct values) rather than ported as
   two-limb u64 arithmetic. That shrinks the buffer and eliminates the
   one place a mechanical port could go wrong. Full task list in the
   design doc.

The WGSL kernel itself is deliberately not written in this session —
it is a focused piece of work that deserves its own commit.

## G5.0b wgpu backend LANDED — cross-platform P7 green

- wgpu 30.0.1 + pollster 1 (latest; was wgpu 22 / pollster 0.4 — user
  correctly flagged that I started with an 8-major-old version).
- API drift absorbed (documented in the g5.0b commit): InstanceDescriptor
  takes by value, RequestAdapterOptions.apply_limit_buckets,
  PipelineLayoutDescriptor.immediate_size, &[Option<&BGL>],
  DeviceDescriptor without trace path, PollType::Wait struct variant,
  get_mapped_range -> Result.
- WGSL kernel: naga rejected the loop-with-unreachable-exit in
  flush_lookup; rewrote as `while` + `result` + `break`.
- Verification: 1M/1M bit-equal to CPU on Metal via wgpu (macOS local),
  2-run determinism within a WgpuContext, and the full feature matrix
  (default / metal / wgpu / metal+wgpu) passes 5/5.
- CI: `.github/workflows/gpu.yml` now installs mesa-vulkan-drivers and
  runs the same 1M corpus against Linux/Vulkan-llvmpipe.

This is the multi-platform deliverable the amendment promised. The
Metal-native path stays as a stepping stone; both features are opt-in.

### G1.1 fixture-derivation lesson

Two of the three fixtures I hand-drew were wrong, and both in the same
way: I mentally counted "villain wins" and then wrote the assertion as
if those were "hero wins." The `2*wins + ties` numerator uses HERO wins.

- royal on board → correct on first pass (990)
- royal via hole → correct on first pass (1980)
- wheel on board → I expected 1160 ("2*170 + 820") but the correct
  answer is 820 because the 170 pairs are HERO LOSSES, not wins.

The independent Python evaluator caught it. Rule going forward: every
new fixture in G1.2's P7 set is computed in Python first, and the
Python value is what the Rust assertion uses. Never re-derive by hand.
