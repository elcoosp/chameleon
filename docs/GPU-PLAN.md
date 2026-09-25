# GPU-PLAN — M1 Metal Accelerator Track (Agent Runbook, execute BEFORE V2-DEV-PLAN)

You are a coding agent working in the CHAMELEON repo root (Rust workspace, edition 2024).
Your job: add a **feature-gated Apple Metal (GPU) accelerator track** that speeds up
exact-equity enumeration and eval-harness stages WITHOUT changing any bot behavior,
any CPU benchmark, or any existing gate. Rationale: `GPU-BRAINSTORM.md` (same folder).
This plan is self-contained; you do not need the brainstorm to execute it.

**Execution order:** PERF-PLAN (T1..T8, done) → **GPU-PLAN (this file)** → V2-DEV-PLAN.
When this plan finishes green, V2-DEV-PLAN Phase 1 starts.

**Machine:** Apple M1 Mini, 16 GB unified memory, 8-core GPU, macOS. CI is Linux
(everything GPU must be skippable there — see Part I step 4b).

**Kill-switch philosophy:** one cheap probe (G0) decides GO/NO-GO for the whole track.
NO-GO costs ~2 sessions and leaves one inert crate behind. Every later conditional task
has a SKIP rule backed by a recorded measurement — never skip without writing the number down.

---

## PART 0 — WHAT THIS TRACK DOES (and honest corrections vs the brainstorm)

Deliverables:
1. New crate `cham-gpu` (12th workspace crate), feature `metal`, macOS-only kernels,
   pure-Rust table I/O + CPU reference implementations that work everywhere.
2. Exact-equity enumeration tables (integer, bit-reproducible) as hash-verified artifacts:
   | Table | Enumeration (7-card evals) | CPU 4-thread @~4e8/s | GPU target | Artifact |
   |---|---|---|---|---|
   | Turn EHS (C(52,4)=270,725 boards × 1,326 holes) | 1.64e13 | ~11 h | ≤ 2.3 h | u32 "2w+t", 1.44 GB |
   | Flop EHS (C(50,3)=19,600 boards × 1,326) | 2.78e13 | ~19 h | ≤ 4 h | u32 "2w+t", 104 MB |
   | River EHS (C(52,5)=2,598,960 × 1,326) — conditional | 3.42e12 | ~2.4 h | ≤ 30 min | u16 "2w+t", 6.9 GB |
3. `chameleon verify --gpu` enforcing three new gates **P7/P8/P9** (Part V).
4. At most two conditional consumers (turn-machinery exact utilities, AIVAT enumeration
   stage), each behind a measured SKIP rule.

**Corrections vs GPU-BRAINSTORM.md (do not re-import its optimism):**
- The brainstorm's "turn BR grid 30–75×" was WRONG: with per-river rank vectors
  (46 × 1,326 evals per turn board ≈ 61k evals, then integer rank comparisons), exact
  turn-level BR is already CPU-cheap. There is **no GPU BR-grid task** in this plan.
- "RNR 10–100×" was overstated: river re-solve is already cheap on CPU
  (`solve_rnr_400` = 6.81 ms). River EHS table is therefore CONDITIONAL (G1.4).
- The genuine GPU value is bulk enumeration (10^12–10^13 eval scale) — that is what
  G1 builds and what P8 gates.

---

## PART I — THE DEV LOOP (non-negotiable, run after EVERY task)

```text
1. PRE-FLIGHT (once per session)
   git status                      # clean tree, work branch per phase (gpu/g0, gpu/g1, ...)
   cargo build --workspace

2. QUALITY GATE (always)
   cargo fmt --all
   cargo clippy --workspace --all-targets -- -D warnings      # 0 warnings
   cargo nextest run --workspace                              # 141+ passed, 0 failed

3. QUALITY EXTRA (only if the task table says PROOFS / DETERMINISM / GPU-CONSIST)
   PROOFS:      cargo run -q -p cham-cli -- verify --proofs   # P-1..P-4 GREEN
   DETERMINISM: run the task's named replay/determinism test explicitly
   GPU-CONSIST: run the task's named CPU-vs-GPU bit-exactness test explicitly
                (macOS + --features metal only; on Linux print SKIP and move on)

4. PERF GATE (always)
   cargo bench --workspace > bench-after-<task-id>.txt
   FAIL if any existing benchmark regresses > 5% (criterion, p < 0.05).
   NOTHING in this track budgets a CPU regression. GPU code must be inert
   (feature off, tables absent) wherever CPU paths are measured.

4b. GPU GATE (only tasks tagged GPU-*; macOS with --features metal)
   cargo run -q -p cham-cli -- verify --gpu        # after G3.0 exists
   Before G3.0: run the task's own gate command (given in the task text).
   On Linux/feature-off: the command must print "SKIP: metal unavailable" and exit 0.

5. INVARIANT GATE (always)
   cargo run -q -p cham-cli -- verify --perf --count-infosets --proofs   # GREEN

6. EVAL GATE (only tasks tagged [POLICY-AFFECTING])
   This track ships NO policy changes. If you ever find yourself changing a decision
   path (not just measurement), STOP: that is out of scope — register an EXP (Part VI)
   and leave the change unimplemented.

7. COMMIT + LOG
   git commit -m "<type>(<phase.task>): <one-line>"   # types: correctness|measure|engineering|research|govern
   Append one section to worklog.md (what, gates run, numbers observed).

FAILURE RULE: a gate fails → fix and re-run. Two consecutive failures on the same
gate → STOP, append "BLOCKED: <reason>" to worklog.md, ask the human. No third workaround.
```

**PHASE-SEPARATION RULE (GPU-specific, hard):** never run GPU builders (G1.*) at the same
time as training workers, `cargo bench`, ladders, or matches. They share one 68 GB/s memory
bus and one power rail. Builders run exclusive. Verify with
`sudo powermetrics --samplers gpu_power,cpu_power -i 1000` (optional, record one screenshot
of numbers in worklog during G1.2).

---

## PART II — HARD RULES

**From SPECS/00 (unchanged, apply to all code you write):**
- No `unsafe`, no nightly features. (`metal` and its `objc2*` transitive deps contain
  internal unsafe at the FFI boundary — same scoped pattern already documented for
  memmap2; cham-gpu's own code stays `#![forbid(unsafe_code)]`.)
- No RNG-stream changes. Enumeration is exhaustive and uses **zero RNG**. Consistency-test
  sampling derives from `cham_core::rng` seeded paths, seeds recorded in manifests.
- No wall-clock in eval paths (builders are offline tooling; wall-clock in their
  manifests/logs is fine; live decision paths keep `budget.rs` as the only clock).
- No infoset-key composition changes. GPU tables are keyed by **card-combinatoric indices**
  (G1.0), never by infoset keys.
- Artifact formats: this track ADDS a new artifact family (GPU tables) with its own
  version constant + blake3 content hash. Existing artifact formats untouched.
- `ThreadMode::Deterministic` and single-threaded MCCFR math: UNTOUCHED. The trainer
  never sees GPU code.

**Whitelist amendment (pre-authorized by the plan owner):** add exactly ONE new direct
dependency: `metal = "0.31"` (or the latest 0.x at execution time — record the exact
version). Its transitive `objc2`/`block2` family comes with it; list the full transitive
closure from `Cargo.lock` in the G0.1 commit message. cham-gpu's `Cargo.toml`:
`metal = { version = "0.31", optional = true }` + `[features] metal = ["dep:metal"]`.
All other crates may depend on cham-gpu (pure Rust, tiny) but NEVER on the `metal` feature.

**GPU never-list (each item is a fireable offense):**
1. Do NOT port ES-MCCFR traversal, table hashing, RM+ updates, or anything in
   `cham-blueprint/src/traversal.rs` / `trainer.rs` / `table.rs` to the GPU.
2. Do NOT put floating point into any kernel whose output is persisted or compared in a
   gate. Integer wins/ties enumeration only (schema in G1.0).
3. Do NOT use GPU atomics in persisted kernels — threadgroup tree reduction
   (`simd_sum` + threadgroup sum) in fixed order only.
4. Do NOT make live play depend on GPU presence: every consumer falls back to the
   existing CPU path when `--tables` is absent or the device is missing. GPU failure
   must degrade to today's behavior, never crash the agent.
5. Do NOT add wgpu, ANE/CoreML, AMX/Accelerate, or any second GPU dependency.
6. Do NOT modify `cham-core`'s `evaluate7` semantics or its existing tests; expose its
   lookup tables additively (G0.2) instead.
7. Do NOT build tables into `target/` or the repo tree; artifacts live in
   `artifacts/gpu-tables/` and are git-ignored (add `.gitignore` line in G1.0).
8. Do NOT run GPU builders concurrently with anything else (Part I phase-separation rule).

---

## PART III — TASK TABLE

| Phase.Task | Name | Tag | Extra gates | Skip rule |
|------------|------|-----|-------------|-----------|
| G0.0 | Machine preflight + baseline capture | [MEASUREMENT] | — | — |
| G0.1 | Whitelist amendment + cham-gpu skeleton | [GOVERNANCE][ENGINEERING] | — | — |
| G0.2 | MSL `evaluate7` port + 1M bit-exact test | [CORRECTNESS][ENGINEERING] | GPU-CONSIST | — |
| G0.3 | CPU_ENUM baseline + GPU probe + EXP-020 verdict | [MEASUREMENT] | GPU-PERF | NO-GO → jump to G4.0 closure |
| G1.0 | Card indexers + table schema + manifest/blake3 | [CORRECTNESS] | DETERMINISM | — |
| G1.1 | 4-thread CPU reference enumerator | [MEASUREMENT][ENGINEERING] | DETERMINISM | — |
| G1.2 | Turn EHS builder (270,725 boards) | [ENGINEERING] | P7+P8 | P8 fail ×2 → BLOCKED |
| G1.3 | Flop EHS builder (19,600 boards) | [ENGINEERING] | P7+P8 | P8 fail ×2 → BLOCKED |
| G1.4 | River EHS builder (2,598,960) — CONDITIONAL | [ENGINEERING] | P7+P8 | no consumer (G2.0) → SKIP |
| G2.0 | Consumer discovery + profiling | [MEASUREMENT] | — | — |
| G2.1 | Turn-machinery exact utilities — CONDITIONAL | [ENGINEERING] | P9+DETERMINISM | no consumer or <2× → SKIP |
| G2.2 | AIVAT enumeration stage — CONDITIONAL | [ENGINEERING] | P9+DETERMINISM | stage <30% of eval wall → SKIP |
| G3.0 | `verify --gpu` (P7/P8/P9) + justfile recipe | [ENGINEERING] | — | — |
| G4.0 | Final verification + README + governance sync | [GOVERNANCE] | all | — |

Conditional tasks: run the measurement FIRST, write the number down, then apply the
skip rule. A SKIP is a valid outcome — commit it with `engineering(<id>): SKIP <reason+number>`
and a worklog entry.

---

## PART IV — PHASES

### Phase G0 — Probe (the kill gate; ~2 sessions)

**G0.0 Machine preflight** — record in worklog.md:
- `sysctl -n hw.memsize hw.ncpu machdep.cpu.brand_string`
- `system_profiler SPDisplaysDataType | rg -i "chipset|metal"` (GPU core count)
- Disk free: `df -h .` — require ≥ 25 GB free for artifacts (fail preflight if less)
- `cargo bench --workspace > bench-before-gpu.txt` (the PERF GATE reference for the whole track)

**G0.1 cham-gpu skeleton** — Files: `Cargo.toml` (workspace members), new
`crates/cham-gpu/` (`Cargo.toml`, `src/lib.rs`, `src/device.rs`, `src/noop.rs`).
1. Workspace member `cham-gpu`; `#![forbid(unsafe_code)]`; deps: `serde`, `blake3`,
   `thiserror`, `anyhow`, `metal` (optional, feature `metal`).
2. `device.rs`: with feature+macOS, `Metal::create_system_default_device()`; expose
   `pub enum GpuDevice { Available { name: String }, Unavailable(String) }`.
   Without feature / on other OS: cfg-gated stubs — the crate compiles everywhere.
3. New CLI subcommand `chameleon gpu-doctor` (in `crates/cham-cli/src/cmd/mod.rs` +
   `main.rs`): prints OS, feature state, device name or reason, whitelist-amendment note.
   Exit 0 in all cases (it is diagnostic).
4. Tests: `gpu_crate_compiles_without_feature` (asserts the noop path constructs),
   feature-gated `gpu_device_present` (macOS CI-local; `#[cfg(not(target_os="macos"))]`
   variant prints SKIP).
Acceptance: quality gate green on macOS AND on Linux (feature off); `gpu-doctor` works.

**G0.2 MSL eval port (bit-exact by construction)** — Files: `crates/cham-core/src/eval/mod.rs`
(additive only), new `crates/cham-gpu/src/msl/eval7.msl` (include as `include_str!`),
`crates/cham-gpu/src/kernels.rs`, `crates/cham-gpu/tests/consistency_eval7.rs`.
1. Read `evaluate7` end to end. Add `pub fn eval_tables() -> EvalTables<'static>` in
   cham-core exposing every static lookup table it uses as byte slices (additive, no
   behavior change; existing tests must pass untouched).
2. Transcribe — do NOT redesign — the exact algorithm into MSL: same table bytes into a
   `MTLBuffer` (`storage_mode_shared`), same op order, same index math. Kernel
   `eval7_kernel`: `device const uchar* tables, constant uint* hands, device ushort* out`
   — one thread per 7-card hand, `out[i] = evaluate7_msl(...)` matching cham-core's
   return type/encoding exactly.
3. Consistency test `consistency_eval7`: 1,000,000 random hands from
   `cham_core::rng` seeded with the pinned seed `0x60C0FFE` (derive via the crate's seeded
   path; record the seed in the test constant). CPU `evaluate7` vs GPU output: assert
   bit-equal for ALL hands. This test is the permanent P7 core.
4. Run-to-run determinism: run the kernel twice, assert byte-identical output buffers.
Acceptance: GPU-CONSIST green (1M/1M equal); quality gate green.

**G0.3 Probe + EXP-020 verdict** — Files: new `crates/cham-gpu/src/bin/gpu-probe.rs`,
`experiments/EXP-020-gpu_eval_probe.toml` (template in Part VI).
1. CPU baseline: in `gpu-probe --mode cpu`, evaluate the same 1M hands with
   `evaluate7_batch` (T1 API) on 4 `std::thread` workers; print
   `CPU_ENUM_EVALS_PER_SEC` (expect ~3–4e8; record actual).
2. GPU: `gpu-probe --mode gpu` — dispatch `eval7_kernel` over the same hands; print
   `GPU_EVAL_EVALS_PER_SEC` (host wall-clock around completed command buffer).
3. Enumeration microkernel `gpu-probe --mode enum`: one fixed river board, loop all
   1,326 hole × 990 opp pairs per threadgroup (structure it like the future EHS kernel);
   print `GPU_ENUM_EVALS_PER_SEC` (expect this ≥ raw eval — better locality).
4. Verdict (pre-registered, do not adjust after seeing numbers):
   **GO** iff bit-exactness holds (G0.2) AND `max(GPU_EVAL, GPU_ENUM) ≥ 10 × CPU_ENUM`.
   **NO-GO** otherwise → append `BLOCKED: GPU track closed at G0 (numbers: ...)` to
   worklog.md, set EXP-020 `status = "resolved-no-go"`, skip to G4.0 (closure = keep
   cham-gpu with feature off, README one-paragraph post-mortem).
5. Write all three numbers + verdict into the ledger entry and worklog.
Acceptance: numbers recorded; verdict applied exactly as pre-registered.

### Phase G1 — Enumeration tables (~3–4 sessions, builders run exclusive)

**G1.0 Indexers + schema** — Files: `crates/cham-core/src/eval/mod.rs` (additive),
new `crates/cham-gpu/src/tables.rs`, `crates/cham-gpu/tests/indexers.rs`.
1. Add to cham-core (additive): `pub fn hole2_index(c:[Card;2]) -> u16` (rank over all
   C(52,2)=1,326 pairs: order pair ascending, combinadic rank `C(hi,2)+lo`), and
   `boardk_index` for k=3,4,5 (lexicographic combinadic rank over ascending 5-tuples).
   If cham-core already has an equivalent canonical indexing (see test
   `hand2_canonical_169`), REUSE its convention and document the mapping.
2. `tables.rs`: `pub struct EhsTable { kind: Street, encoding: Encoding, denom: u32,
   data: Vec<u8> }` with `Encoding::TwoWinsPlusTies`; `denom`: turn/flop computed per
   entry class (turn 45,540 = 46×990; flop 1,070,190 = 1,081×990; river 990); loader
   checks: magic, `ARTIFACT_VERSION_GPU_TABLE = 1`, blake3 vs manifest, byte length =
   boards × 1,326 × width. Load failures → typed error, callers fall back to CPU.
3. Manifest JSON per table in `artifacts/gpu-tables/<kind>.json`: kind, git_rev, tool
   version, blake3, bytes, boards, build wall-clock secs, throughput, seed (null — zero
   RNG), encoding, denom.
4. Tests: index bijection by brute force on an 8-card universe (roundtrip + count +
   no dupes for k=2..5 subsets of 8 cards); manifest roundtrip; hash-mismatch refused.
Acceptance: DETERMINISM + quality gates green.

**G1.1 CPU reference enumerator** — Files: `crates/cham-gpu/src/reference.rs`,
`crates/cham-gpu/tests/reference_bitexact.rs`.
1. `pub fn ehs_reference(board: &[Card], hole: [Card;2]) -> u64` — exact integer
   enumeration with the SAME convention the GPU kernel will use: `numerator = 2*wins +
   ties` over every opponent combo (and for flop/turn, over every runout). This is the
   slow-but-obviously-correct oracle; keep the code dumb and readable.
2. 4-thread throughput harness `gpu-probe --mode cpu-enum --street turn --boards 200`:
   boards distributed round-robin by index; print `CPU_ENUM_EVALS_PER_SEC` (this is the
   P8 denominator, measured per machine per session).
3. Test: hand-verified tiny cases — 3-card-board toy universe where enumeration is
   checkable by brute force; river fixture vs an independent one-liner using existing
   cham-eval utilities if available (`rg -n "equity|ehs" crates/cham-eval/src`).
Acceptance: oracle tests green; `CPU_ENUM` number recorded in worklog.

**G1.2 Turn EHS builder** — Files: `crates/cham-gpu/msl/ehs_turn.msl`,
`crates/cham-gpu/src/bin/gpu-build.rs`, `crates/cham-gpu/tests/consistency_turn.rs`.
1. Kernel design: one threadgroup per turn board (up to 1,024 threads). Phase 1: threads
   cooperatively compute `evaluate7` for all 1,326 hero holes on this board+river-less
   context per runout... — concretely: loop the 46 river cards in thread-local strided
   fashion; per river, each thread evaluates a strided slice of the 990 opp pairs;
   accumulate `wins/ties` per (board, hole) in threadgroup memory; tree-reduce at the
   end; write `u32 numerator` to `out[boardk_index(board) * 1326 + hole2_index(hole)]`.
   Hero eval per river is 1 eval/thread — recompute per river, do NOT cache across
   threads. No atomics. Integer only.
2. `gpu-build --kind turn`: iterate boards by index; write output via preallocated file +
   positional writes (byte layout independent of thread scheduling); print throughput;
   write manifest with blake3. Budget ceiling (non-gating): ≤ 3 h wall at GO-grade GPU.
3. Gates: **P7** — 10,000 random (board, hole) samples (seeded, recorded) compared
   against `ehs_reference`: exact equality. **P8** — throughput ≥ 8 × CPU_ENUM
   (same-session measurement). Rebuild-twice → blake3 identical (test
   `table_identity_rebuild`, gated `#[ignore]` by default, run once here).
Acceptance: P7+P8 green; manifest written; identity check recorded.

**G1.3 Flop EHS builder** — same pattern as G1.2 (`ehs_flop.msl`, `--kind flop`).
Kernel: threadgroup per flop board; runout loop = C(47,2)=1,081 (turn,river) pairs;
per runout 1 hero eval + 990 opp evals. Artifact 104 MB. Same P7 (10k samples vs
reference) + P8 (≥ 8 × CPU_ENUM) + rebuild-identity.
Acceptance: same as G1.2.

**G1.4 River EHS builder — CONDITIONAL** — First run G2.0's consumer discovery. Build
ONLY if a consumer anchor exists (turn machinery river-mode weighting, robust-path river
range weights, or a named V2 Phase 5 hook) — else SKIP with the evidence. If built:
`ehs_river.msl`, artifact u16 6.9 GB, same P7/P8/identity gates, disk check ≥ 10 GB free
before start, and a one-line README note about the artifact's size.
Acceptance: built with gates green, OR committed SKIP with the consumer-discovery output.

### Phase G2 — Conditional consumers (~1–2 sessions; both may SKIP)

**G2.0 Consumer discovery + profiling** — MEASUREMENT ONLY, no code changes.
1. Locate every consumer candidate:
   `rg -n -i "fmbr|reach_gadget|reach gadget|range_weight|equity_sample|aivat" crates/`
2. For each hit: does it estimate per-hand/per-range equity by Monte-Carlo sampling on
   turn/flop? Record file, function, sample count, and % of its stage's wall-clock
   (profile with `cargo run --release` + a manual timer around the stage, or `sample`
   on macOS).
3. AIVAT: locate the enumeration stage (`rg -n -i aivat crates/cham-eval crates/cham-cli`),
   measure its share of `chameleon eval` wall-clock on a standard fixture run.
4. Write a consumer table into worklog: anchor / what it estimates / sampled? / % wall.
Acceptance: table exists (even if empty — empty table ⇒ both consumers SKIP).

**G2.1 Turn-machinery exact utilities — CONDITIONAL** — Files: the consumer module found
in G2.0, new `crates/cham-gpu/src/consumer.rs`.
1. Add `--tables <dir>` (clap arg, default `artifacts/gpu-tables`) to the owning CLI
   command. When the turn table loads (hash-verified): replace the sampled equity with
   exact table lookup (divide numerator by denom in f64 ONLY at the consumer boundary —
   the stored/compared quantity stays the integer numerator).
2. Table absent / hash mismatch / non-macOS → silently use today's path (log one line).
3. Gates: **P9** — bit-identical outputs vs the sampled path is NOT expected (sampling
   ≠ exact); instead: (a) determinism: same inputs + table → same outputs, twice;
   (b) stage speedup ≥ 2× vs the sampled path on the G2.0 fixture (record numbers);
   (c) sanity: exact-EHS within ±2σ of the sampled estimate on 100 fixtures (guards a
   wrong-indexing bug — off-by-one indexing shows up as huge outliers).
Acceptance: gates green, or committed SKIP with the profiling numbers.

**G2.2 AIVAT enumeration stage — CONDITIONAL** — Only if G2.0 shows the stage ≥ 30% of
eval wall-clock. Port the enumeration inner loop to a GPU kernel (same discipline:
integer counts, no atomics, bit-exact vs `ehs_reference`-style oracle on 1k fixtures);
speedup gate ≥ 5× on the stage; otherwise SKIP with numbers. Ledger entry either way.
Acceptance: P7+P9 green on the stage, or committed SKIP.

### Phase G3 — Gates become permanent (~1 session)

**G3.0 `verify --gpu`** — Files: `crates/cham-cli/src/cmd/verify.rs`, `justfile`.
1. New flag `--gpu`: prints and enforces:
   - **P7 GPU-CONSISTENCY**: re-run the quick consistency suite (1k eval hands; 1k
     (board,hole) per BUILT table; 100 subgame fixtures if G2.1 shipped) — all bit-exact.
   - **P8 BUILD-THROUGHPUT**: read each manifest; FAIL if recorded throughput < 8 ×
     current-session CPU_ENUM (re-measure CPU_ENUM live, 60 s budget).
   - **P9 INTEGRATION**: for each shipped consumer, re-run its fixture set; assert the
     G2 speedup number still holds within 20%.
   - Missing artifact → FAIL "run gpu-build --kind <k> first" (never silently pass).
     Non-macOS / feature-off → print `SKIP: metal unavailable` and exit 0.
2. Push failures into verify's existing `failures` vec (nonzero exit, pattern of T6).
3. `justfile`: add `gpu-gates:` recipe = build + `verify --gpu`.
4. Update `cli_parse_surface` / `verify_exit_codes` tests for the new flag paths.
Acceptance: on macOS all gates PASS with artifacts; on Linux prints SKIP, exit 0.

### Phase G4 — Closure

**G4.0 Final verification + governance sync** — all of these green:
```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
cargo run -q -p cham-cli -- verify --perf --count-infosets --proofs    # CPU gates unharmed
cargo bench --workspace > bench-after-gpu.txt    # diff vs bench-before-gpu.txt: no regression
cargo run -q -p cham-cli -- verify --gpu                                # macOS: PASS; Linux: SKIP
```
Then:
1. README Status section: GPU track paragraph (device, GO/NO-GO verdict, tables built +
   hashes, consumer speedups or SKIPs, new `verify --gpu`).
2. SPECS/00: append the whitelist amendment line (`metal` + transitive closure list) and
   extend the scoped-unsafe note with the `metal`/`objc2` FFI-boundary sentence
   (pattern of V2-DEV-PLAN task 1.4).
3. worklog.md: final section with every gate number (P7/P8/P9, CPU_ENUM, GPU numbers,
   artifact hashes).
4. Leave a one-paragraph handoff note for the V2 executor: tables available at
   `artifacts/gpu-tables/` can back V2 Phase 5.3 ("exact enumeration where the river
   allows" → now also flop/turn) and reduce EXP-012's cost estimate; EXP-021/022/023
   templates exist in `experiments/`.

---

## PART V — GATE DEFINITIONS (P7/P8/P9)

| Gate | Name | Definition | Enforced by |
|------|------|------------|-------------|
| P7 | GPU-CONSISTENCY | Every GPU kernel's output is bit-identical to the CPU reference on the pinned sample sets (1M eval hands; 10k/board-street per table; 1k quick resample in verify). Integer arithmetic only in persisted kernels. | G0.2 test + per-table tests + `verify --gpu` |
| P8 | BUILD-THROUGHPUT | Each table builder ≥ 8 × same-session 4-thread CPU reference throughput. | manifests + `verify --gpu` |
| P9 | INTEGRATION | Each shipped consumer: deterministic-with-table, stage speedup ≥ pre-registered bar (G2.1 ≥ 2×, G2.2 ≥ 5×), exact-vs-sampled sanity bound holds. | per-consumer fixtures + `verify --gpu` |

---

## PART VI — EXP REGISTRATIONS

EXP-020 is this track's own pre-registered experiment. EXP-021..023 are REGISTERED-ONLY
templates for V2-era work — do not implement them here. Use the V2-DEV-PLAN Part III
TOML template verbatim.

| EXP | name | class | status | trigger / note |
|-----|------|-------|--------|----------------|
| 020 | gpu_eval_probe | measurement | resolved in G0.3 | GO iff bit-exact AND max(GPU_EVAL, GPU_ENUM) ≥ 10 × CPU_ENUM |
| 021 | gpu_every_river_rnr | research | registered | fires only if a future live-budget analysis shows every-river re-solve fits the move clock with tables; gate = promotion ratchet |
| 022 | gpu_exact_lbr_methodology | research | registered | fires if V2 Phase 3 ratchet wants exact turn-level LBR as evidence; requires owner sign-off on methodology change |
| 023 | gpu_exact_histo_phase5 | research | registered | fires when V2 Phase 5.3 runs and tables are present; A/B: sampled vs exact histograms, standard ratchet |

---

## PART VII — WHAT "DONE" MEANS

1. Every task row in Part III is committed (including committed SKIPs with evidence).
2. All Part IV acceptance criteria met; `experiments/EXP-020` resolved; `EXP-021..023`
   files exist, valid TOML, `status="registered"`.
3. `ls artifacts/gpu-tables/` matches exactly the tables whose consumers exist (turn +
   flop minimum on GO; river only if G1.4 fired).
4. All Part I gates green on macOS; Linux CI unaffected (141+ tests, feature-off build).
5. `bench-after-gpu.txt` vs `bench-before-gpu.txt`: no existing benchmark regressed > 5%.
6. README, SPECS/00, worklog all updated. Then — and only then — start V2-DEV-PLAN Phase 1.
