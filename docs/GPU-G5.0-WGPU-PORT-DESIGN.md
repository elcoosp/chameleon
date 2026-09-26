# G5.0 — wgpu port design (the multi-platform move)

> **Status:** [DONE] — wgpu backend landed (G5.0a/b); Metal-native still available

Companion to `docs/GPU-PLAN-AMENDMENTS.md` Amendment 002. This document
is a concrete design for the wgpu port, not an implementation. It exists
so the WGSL kernel is written once, correctly, rather than iterated on
by trial and error against a moving target.

## What stays the same

- **Algorithm.** `crates/cham-gpu/src/msl/eval7.msl` is the reference.
  The WGSL kernel is a mechanical port: same op order, same index math,
  same table layout, same packed hand encoding (6 bits/card x 7 = 42
  bits in a u64).
- **Bit-exactness gate (P7).** `consistency_eval7_one_million_hands`
  runs against whichever backend is active. Same 1M-hand corpus, same
  seed.
- **Correctness runs on Linux CI** once WGSL exists (via Mesa llvmpipe,
  software Vulkan). The `gpu.yml` workflow already has the slot
  commented in.

## What changes (WGSL constraints vs MSL)

WGSL is *stricter* than MSL. Three concrete consequences:

1. **No scalar u8/u16 in storage buffers.** Storage buffers are
   `array<T>` where T is one of u32, i32, f32, or a vector of those.
   Bytes must be packed manually.

   Resolution: represent both `tables` and `hands` as `array<u32>`.

   Table layout (u32-aligned):

       tables[0 .. 2048]        straight[8192]  as 2048 u32 (4 bytes each)
       tables[2048 ..]          entries: 2 u32 per entry (key_u32, val_u32)

   Where the entry key has already been reduced to u32 (see point 2)
   and val_u32 holds the u16 value in its low 16 bits.

   Hands layout: hands[i] = lower 32 bits of the packed hand,
   hands[n + i] = upper 32 bits. Two separate buffer views over one
   allocation, or two buffers; whichever keeps the dispatch code
   cleanest.

2. **No ulong / u64 scalar.** WGSL has 32-bit integers and floats.

   Resolution: do NOT reimplement the LinearMap hash in two-limb
   u32 arithmetic. Instead, notice that the seven_map's prime-product
   keys only ever take ~60k distinct values (one per 7-card rank
   multiset). Reindex them to u32 at *pack time* (Rust side, where u64
   works fine) and ship a u32-keyed map. Same for flush_map. The kernel
   never sees u64 at all.

   This shrinks the buffer, keeps the WGSL kernel simple, and removes
   the one place a mechanical port could introduce a subtle bug.

3. **Workgroup model differs.** MSL uses dispatch_threads(grid, tg)
   with an arbitrary grid; WGSL uses dispatch_workgroups(gx, gy, gz)
   and the kernel indexes with global_invocation_id.

   Resolution: trivial. `@workgroup_size(64)` and
   `@builtin(global_invocation_id) id: vec3<u32>; let tid = id.x;`.

## What the port touches (files)

| File | Change |
|---|---|
| `crates/cham-gpu/Cargo.toml` | add optional `wgpu` + feature `wgpu = ["dep:wgpu"]` |
| workspace `Cargo.toml` | add `wgpu` to workspace deps (whitelist amendment; record transitive closure in commit) |
| `crates/cham-gpu/src/wgsl/eval7.wgsl` | new — the WGSL kernel |
| `crates/cham-gpu/src/wgpu_backend.rs` | new — `WgpuContext` (device+queue+pipeline), `dispatch_eval7` |
| `crates/cham-gpu/src/lib.rs` | cfg-gate: `#[cfg(feature = "wgpu")] pub mod wgpu_backend;` |
| `crates/cham-gpu/src/kernels.rs` | `launch_eval7` grows backend dispatch (compile-time via cfg, or runtime via a `Backend` enum) |
| `crates/cham-gpu/tests/consistency_eval7.rs` | add `#[cfg(feature = "wgpu")]` arm — same 1M corpus, same seed, same bit-exact assertion |
| `.github/workflows/gpu.yml` | uncomment the "wgpu correctness (llvmpipe)" step; runs on Linux CI |

## The one design decision left open

**`launch_eval7` backend selection: compile-time or runtime?**

- Compile-time (`cfg!(feature = "metal")` preferred, else wgpu):
  simplest, no trait objects, no dynamic dispatch. But you cannot pick
  the backend at runtime on a machine that has both.
- Runtime (an enum passed in, or a global once-set): more flexible;
  matches `verify --gpu --backend metal|wgpu|both`. Costs one match at
  dispatch time.

Recommendation: start compile-time with cfg precedence
`metal > wgpu > none`; add runtime selection when G3.0 grows its
`--backend` flag. Keeps the first port minimal.

## Concrete task list (for the G5.0 session)

1. **Whitelist the wgpu dep.** Add to workspace `Cargo.toml` behind a
   feature flag; do not enable it in any crate by default. Record the
   full transitive closure (from `Cargo.lock`) in the commit message,
   as we did for `metal` in G0.1.
2. **Write the WGSL kernel.** Mechanical port of `eval7.msl` per the
   layout above. Include the layout in a comment block at the top of
   the file.
3. **Write `pack_tables_wgsl`.** The reduced layout (option B above):
   `straight` as `array<u32>` and `entries` as `array<u32>` of 2
   (key_u32, val_u32). The CPU-side key reduction lives in Rust where
   u64 works.
4. **Write `WgpuContext`.** Instance, request_adapter, request_device,
   create_shader_module (from WGSL), pipeline. Use `pollster::block_on`
   (one extra whitelist amendment) so the API is synchronous, matching
   the Metal side. Add `pollster` in the same commit.
5. **Wire into `launch_eval7`** via cfg precedence.
6. **Extend the consistency test.** Same 1M corpus, run against the wgpu
   backend, assert bit-exact.
7. **Run on macOS** locally — expect PASS.
8. **Enable the Linux CI step** (already in `gpu.yml`). Verify llvmpipe
   on ubuntu-latest produces bit-equal output.
9. **Only then** flip the default in `verify --gpu` to `wgpu`
   (this is G5.2).

## What this session should NOT do

- Not port the EHS builders to WGSL yet (G1.2/G1.3 stay Metal-native;
  they can be ported once WGSL eval7 is proven).
- Not touch `metal`-only code paths beyond keeping them behind their
  feature.
- Not add wgpu as a default dependency — it stays optional forever.
