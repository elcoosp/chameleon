# GPU-PLAN Amendments

Two amendments to `docs/GPU-PLAN.md`, both written *after* G0.3 was
measured. They are governance changes, not re-readings of the numbers.

---

## Amendment 001 — Relax the EXP-020 / P8 bar

### Context

The plan set two thresholds for the GPU track's kill switch:

- G0.3 (Part IV): `GO iff max(GPU_EVAL, GPU_ENUM) ≥ 10 × CPU_ENUM`
- P8 (Part V): builder throughput `≥ 8 × same-session CPU reference`

Both were drawn against **Part 0's ~4 × 10⁸ evals/s** CPU baseline for
the 4-thread reference. The actual measured rate on the M1 Mini is
**~3 × 10⁷ evals/s — ~13× lower than assumed.** The *absolute* bar is
therefore much harder than the plan's author intended; and the two
thresholds (10×, 8×) contradict each other inside the same document.

Separately, the M1 Mini is not a fair benchmarking environment: it runs a
GTO solver training loop that saturates the memory bus and thermal budget.
Five consecutive trials of the same code produced ratios spanning
**5.66× to 11.85× (median 9.70×)**; trial 3's "GO" was the CPU being
slow, not the GPU being fast. Full table:
`docs/bench-status-gpu-trials.md`.

### Amendment

1. **The bar is defined relative to a *quiet* session's 4-thread CPU
   reference.** "Quiet" is defined as: 60 s rolling load average `< 1.0`,
   and no other `cargo`, `rustc`, `python`, or solver-training process
   running. Every measurement records a `quiet = true|false` flag.

2. **Two gates, both must clear on the same quiet session:**

   - **G_enum:** builder-shaped workload (board × distinct-holes) ≥ **3×**
     the quiet-session CPU reference.
     *Rationale:* observed median on the *contended* M1 was ~9.7×; 3×
     leaves >2× headroom even under contention, and matches the plan's
     own P8 number (8×) with margin on a quiet session.
   - **G_warm:** compile-once, dispatch-many ≥ **2×** the quiet-session
     CPU reference.
     *Rationale:* observed warm ratio median on contended M1 was ~4.7×;
     this gate covers the true per-dispatch cost the builders pay.

3. **The 10× and 8× thresholds in the original text are superseded.**
   Both are replaced by G_enum and G_warm above. No other gate in the
   plan is affected.

4. **CI is the source of truth for stability, not the local M1.** Local
   runs iterate; the CI run (or an explicitly quiet local session) is
   what the gate reads. See `.github/workflows/gpu.yml` (added by this
   amendment) and Amendment 002 for the cross-platform story.

5. **Re-run the verdict under the amended rules** — expected after
   Amendment 002's wgpu target lands (see G5.0), so a single measurement
   covers both backends. Until then EXP-020's status stays
   `unresolved-amended` (not re-opened, not closed).

---

## Amendment 002 — Target wgpu, not Metal-only

### Context

The original plan chose Apple Metal because the target host was an M1
Mini. That choice is macOS-only, and the plan's Part I CI story
("Linux must SKIP everything GPU") is a symptom of the same constraint.
Meanwhile:

- Correctness verification benefits from running on Linux.
- Benchmark stability benefits from dedicated runners.
- The project already claims a cross-OS posture (SPECS/00 §1).
- `metal`'s transitive FFI closure (`objc`/`block`/`malloc_buf`/
  `core-foundation`/`core-graphics-types`/`foreign-types`/`bitflags@1`)
  is nontrivial and does nothing for anyone on Linux or Windows.

### Amendment

1. **The final target for GPU code is `wgpu`** (Vulkan / Metal / DX12
   backends, single WGSL source). `metal`-native stays available behind
   the existing `metal` feature and continues to power the immediate G1
   builders because it is already proven bit-exact and fast enough.

2. **G1.x builders may ship Metal-first, but `eval7.msl` is a
   *transcription of `evaluate7`* — algorithm first, MSL second.** Rule:
   no feature that is in MSL but not WGSL (or vice versa) may appear in
   `eval7.msl`. Reviewers check this at each G1.x commit. The MSL file is
   the reference; the eventual WGSL file is a mechanical port with the
   same op order and table layout.

3. **A new phase G5 "wgpu port" is added** (delta to Part III below). It
   ships a WGSL `eval7` kernel plus a `wgpu` feature and passes the same
   **P7** (bit-exact 1M/1M) and **P9** (integration) gates on the same
   hardware. `verify --gpu` grows a `--backend metal|wgpu|both` switch;
   the default becomes `wgpu` once G5 is green.

4. **CI:**
   - **Correctness (P7) runs on `ubuntu-latest`** against Mesa llvmpipe
     (software Vulkan) once G5.0 lands. Feature-off today = a no-op slot;
     the workflow is committed *now* so the slot exists.
   - **Throughput gates (P8 / G_enum / G_warm)** run only on a
     GPU-equipped runner (`macos-14`, Apple Silicon) or an explicitly
     quiet local session, per Amendment 001.

5. **Whitelist.** `wgpu` is a new direct dependency (behind a `wgpu`
   feature, alongside the existing `metal` feature). Its transitive
   closure is larger than metal's (~50 crates: `naga`, `wgpu-hal`,
   per-backend `ash`/`d3d12`/`metal-rs` shims, `web-sys` etc.). Record
   the full closure from `Cargo.lock` in the G5.0 commit message, exactly
   as we did for `metal` in G0.1.

### Amended task table (delta from Part III)

| Phase.Task | Name | Notes |
|---|---|---|
| G1.2–G1.4 | EHS builders | Metal-only acceptable; audit `eval7.msl` for WGSL-portability at each step |
| G2.x | Consumers | unchanged; behind `--tables` |
| G3.0 | `verify --gpu` | gains `--backend metal\|wgpu\|both`; default `metal` until G5.2 |
| **G5.0** | **wgpu feature + WGSL eval7** | new; P7 correctness on macOS + Linux/llvmpipe |
| **G5.1** | **wgpu EHS builders (conditional)** | new; only if `G_enum` clears on a quiet session |
| **G5.2** | **flip default backend to wgpu** | new; after G5.0 green; `metal` remains available |

### Interaction with Amendment 001

The bar (G_enum / G_warm) applies to *whichever backend is being tested*,
against a *quiet-session* CPU reference. A single measurement run with
`--backend both` produces two ratios and is expected to clear the bar for
at least one backend on the target hardware.
