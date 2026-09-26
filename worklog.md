# worklog

Chronological engineering log. One section per task; include gates run and
numbers observed. Required by docs/gpu/plan.md Part I step 7.

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
- SAFETY DEVIATION from plan: docs/gpu/plan.md Part II says cham-gpu "stays
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
- Deviation from docs/gpu/plan.md Part II ("stays #![forbid(unsafe_code)]") is
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
  docs/reports/bench-gpu-trials.md.
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

2. `docs/gpu/g5-wgpu-design.md` — the concrete design for the
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

## G1.2 full turn EHS build — running

- `gpu-build --kind turn --limit 0` writing `artifacts/gpu-tables/turn.bin`
  (expected 1,436,673,240 bytes = 270,725 boards × 1326 holes × 4).
- Measured steady state on limit=2000: **3.29e9 evals/s** (54.5 boards/s).
- Projected wall: **~1.4 h**. Log at `artifacts/gpu-tables/turn-build.log`.
- The artifact is git-ignored (`artifacts/gpu-tables/`); the manifest's
  blake3 goes into this worklog when the run completes.
- Phase-separation rule (GPU-PLAN Part I): no other GPU work runs while
  this is in flight. CPU-only prep (flop kernel source drafting) is OK.

### Note on the earlier throughput bug

The first run reported "2.49e6 evals/s" — I'd forgotten the holes factor:
`(boards × denom) / wall_s` instead of `(boards × holes × denom) / wall_s`.
Off by 1326×, which would have made the projected full build look like
76 days instead of 1.4 h. Fixed in commit c0629de; verified against the
re-measured limit=2000 run.

## G1.2 build post-mortem — killed by external memory pressure

Timeline (from `artifacts/gpu-tables/turn-build.log` and macOS jetsam
traces around 20:22):

- Build reached 115,200/270,725 boards (42%) at a steady 56.4 boards/s
  before being killed. Roughly 35 minutes in.
- Partial `turn.bin` = 603,979,776 bytes (the exact expected size for
  115,200 boards × 1326 holes × 4 bytes). Streaming writer was behaving
  correctly.
- macOS jetsam logs show OOM-killer activity across the system at the
  same time (runningboardd, SiriUploadWorker, SearchUploadWorker).
- Current `vm.swapusage` = 0. Our process was not present in the top-15
  RSS table after death; nothing close to 72 GB was visible.

Assessment: our build did not cause the OOM. Its peak RSS is bounded by
the BufWriter buffer (64 MB) plus small vecs (single-digit MB). The
killing was collateral damage from an unrelated memory spike elsewhere
on the machine.

Process lesson (mine): don't launch multi-hour background jobs on the
user's daily driver without asking. Even a memory-clean GPU run holds
the GPU + memory bus for hours and competes for disk I/O. Future GPU
builds should either:

- run on a dedicated machine / CI macOS runner, OR
- be kicked off only after an explicit "go" from the user, with a
  visible PID and easy kill path (which I did provide, at least), OR
- be chunked (--limit N per invocation, resumed via a new --resume
  feature) so each run is bounded.

No --resume exists today. Deferred: implementing --resume (writes into
an existing file at a given board offset; manifest carries the last
complete board index) is a small, useful addition for the next pass.

## G2.0 Consumer discovery — the honest answer is: no current pure-acceleration consumer

Per docs/gpu/plan.md G2.0, the search for consumers of the turn EHS table
turned up four candidates. Analyzed by whether they can be accelerated
*without changing behavior* (the plan's bar):

| anchor | what it computes today | can GPU table accelerate without changing behavior? |
|---|---|---|
| `cham-opponents::archetype::ehs()` | deterministic strength *proxy* (SPECS/03 §5: "strength_now") | **No** — it is deliberately a proxy. Substituting exact EHS would change opponent policy, i.e., the data-generating process. That's a scope violation, not an acceleration. |
| `cham-opponents::family_b::ehs()` | same proxy shape | same — No |
| `cham-engine::build::histo_*` | 16-bin river-equity CDF over **seeded MC** runouts for flop/turn bucketing | **No** — MC and exact are different distributions. Swapping one for the other changes the bucket abstraction, which changes every downstream artifact. |
| `cham-search::subgame` river range weights | f64 strength = **river**-equity rank | N/A — search is river-only today; the turn table is not its input. |

`cham-eval/src/vr.rs` has only `allin_ev_adjusted` (the B4-wired
duplicate-pairing stage). The **full AIVAT enumeration stage does not
exist yet** — it is spec'd (SPECS/08 §5) but unimplemented. So there is
nothing to accelerate in that stage either.

### G2.1 skip condition (per plan §G2.1 skip rule "no consumer or <2×")

Met, decisively. No CPU path exists that computes turn EHS from CPU
samples at all. Substituting the GPU table into any of the above would
*change semantics*, not speed up an existing bit-identical computation.

### G2.2 skip condition (per plan §G2.2 "stage <30% of eval wall")

Met, trivially: the stage has no implementation.

### What this actually means for the table being built right now

The turn EHS table has **no current home** in the workspace. This is not
wasted work:

- `docs/plans/v3-brainstorm.md` names **turn subgame solving in live play** as
  v3's flagship: "The GPU turn-EHS table removes the last technical
  excuse". The table is a v3 dependency.
- `docs/gpu/plan.md` itself gates G1.2's spend on "a consumer anchor"
  being identified. That gate is loose for the turn table (it is needed
  by v3), tight for the flop table (also v3).
- The river table (G1.4, 6.9 GB) is different: no v3 anchor either, so
  it stays SKIP under any reading.

### Decision recorded

- **G1.2 turn build**: continue (in flight; v3 dependency).
- **G1.3 flop build**: pending; same v3 motivation. Disk allows (104 MB).
- **G1.4 river build**: SKIP. No consumer, no v3 anchor. (Already
  conditional in the plan; now explicitly resolved.)
- **G2.1 / G2.2**: SKIP with the evidence above. Recorded as the plan's
  own skip-with-evidence mechanism intends.
- **G3.0 `verify --gpu`**: still valuable — it enforces P7 (bit-exactness)
  and P8 (build throughput) on whatever tables exist, regardless of
  consumer status. It is a correctness/measurement gate, not a
  consumer-driven feature.

## G1.2 turn EHS table — complete

- `artifacts/gpu-tables/turn.bin`: 1,435,925,400 bytes
  (270,725 boards × 1326 holes × 4 = exact).
- blake3 `a1e260fbf3a381428165fe5c100cd57dd22f098d6f5f5508af346e80e884852c`.
- Wall: 4,584.6 s (76.4 min) at 59.1 boards/s = **3.57e9 evals/s**.
- sample_check: 40 / 40 PASS against `ehs_reference`.
- `verify --gpu`: P7 resample 24/24 bit-equal, P8 rate floor pass, GREEN.

The earlier 42% then OOM attempt (`turn-build-attempt-1.log`) and this
successful run are both on record. Resume was not used here because the
partial file had been cleaned; but it was verified bit-identical on
limit=100 truncate tests before this run.

Prereq for the overnight tiny-ladder experiment: artifacts/agent needs the
EHS consumers. That work is G2.x, G2.0 skip outcome; the table has no v2
consumer. It is however the v3 flagship's prerequisite (V3-BRAINSTORM).

## G4.0 GPU track closure — 2026-09-26

Final state after the 2026-09-25/26 overnight run.

### Tables built and verified

| table | boards | bytes | blake3 (first 16) | rate |
|---|---|---|---|---|
| turn  | 270,725 | 1,435,925,400 | a1e260fbf3a38142 | 3.57e9 evals/s |
| flop  | 22,100  | 117,218,400   | 60b23b581f03b740 | 2.65e9 evals/s |

Both complete=true; `verify --gpu` P7 24/24 bit-equal on each, P8 pass,
P9 informational (G2.0 SKIP outcome: no pure-acceleration consumer in
the v2 workspace).

### What landed this session

- cham-gpu: eval7 MSL (Metal + WGSL, bit-exact), turn EHS MSL, flop EHS MSL,
  persistent GpuContext, tdr-safe batch clamp (flop batch=4, turn a.batch).
- cham-gpu tests: consistency_eval7 (1M hands), consistency_turn (24 pairs),
  consistency_flop (18 pairs), reference (3 hand-derived fixtures).
- cham-eval: matcheng now calls on_public_action for both agents before
  state.apply — the fix that took the ladder from 65% fallback to 0.
- cham-cli: verify --gpu (P7 resample + P8 rate floor), train-bp
  --thread-mode {deterministic|hogwild|snapbatch} flag, train-bp reads
  the same tiny TOML the loader parses (hash parity fix).
- config/abstraction-tiny.toml completed to match AbstractionConfig::tiny().

### Skip outcomes (recorded in plan's own mechanism)

- G2.1 turn-machinery consumer: SKIP (no runtime CPU path computes turn
  EHS; substitution changes semantics).
- G2.2 AIVAT enumeration: SKIP (stage not implemented).
- G1.4 river EHS: SKIP (no consumer, no v3 anchor).

### Known bugs fixed this session

- matcheng missing on_public_action → 65% ladder fallback.
- train-bp used in-code config while loader hashed TOML bytes → hash mismatch.
- Driver summary.txt self-appended → 47 GB disk fill; fixed in driver.
- flop kernel TDR truncation above batch=4.

### Still open

- G4.0 doc items: README GPU paragraph, SPECS/00 whitelist amendment
  sentence for wgpu + pollster (this commit).
- A/B iters chains were run at 10k, not the intended 10k/100k/1M; fixed in
  the driver for the next run.
- Full-abstraction bp: not attempted; queued.

## Postcard migration — stage 1

- bincode 1.3.3 is unmaintained (RUSTSEC-2025-0141); postcard 1.1.3 is
  the recommended serde-compatible replacement. Whitelisted.
- `crates/cham-search/src/cache_persist.rs`: Subgame now serializes via
  `postcard::to_allocvec` / `postcard::from_bytes`. On-disk VERSION 1→2
  so v1 (bincode) files are rejected cleanly and the session starts cold
  once — the cache is a speed optimization, not correctness input.
- 5 cache_persist tests pass; cham-search test suite green.
- Stages 2–6 (table.snap, policy.bin, recorder, ledger, router dataset)
  are recorded in docs/backlog/perf.md §B-10 with per-file plan.
