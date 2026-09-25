// WGSL port of crates/cham-gpu/src/msl/eval7.msl (GPU-PLAN G5.0).
//
// MECHANICAL TRANSCRIPTION. Same op order, same index math, same table
// semantics. Any divergence from the MSL reference is a P7 failure by
// definition — do not "improve" this file.
//
// Layout differences vs MSL (forced by WGSL constraints; see
// docs/GPU-G5.0-WGPU-PORT-DESIGN.md):
//
//   - Storage buffers are `array<u32>` (WGSL has no scalar u8/u16/u64).
//   - The tables buffer packs:
//       [0       .. 2048)                    straight[8192] as 2048 u32
//       [2048    .. 2048 + 2*E7)             seven entries: (key_u32, val_u32)*
//       [2048+2E7.. 2048 + 2*E7 + 2*E8)      flush entries: (key_u32, val_u32)*
//     E7 = seven entry count, E8 = flush entry count (both powers of two).
//   - seven_map and flush_map KEYS are DENSE u32 indices produced at pack
//     time in Rust (they take only ~60k / ~7.5k distinct values; reindexing
//     at pack time removes all u64 arithmetic from the kernel).
//   - Hands are passed as `array<u64>`; each hand packs 7 card indices,
//     6 bits each (42 bits used). WGSL has no scalar u64 in the *core*
//     spec, but wgpu's Metal/Vulkan/DX12 backends all enable the
//     `shader-int64` capability when the adapter supports it. If the
//     adapter does NOT support int64, the caller falls back to CPU.
//     (Alternative: pass two u32 arrays. Kept u64 for parity with MSL.)

struct Tables {
    data: array<u32>,
};

struct Hands {
    data: array<u64>,
};

struct Out {
    data: array<u32>,   // u16 values widened to u32 for storage alignment
};

@group(0) @binding(0) var<storage, read>       tables: Tables;
@group(0) @binding(1) var<storage, read>       hands:  Hands;
@group(0) @binding(2) var<storage, read_write> out:    Out;

struct Params {
    hand_count: u32,
    seven_off:  u32,   // u32 word offset, NOT byte offset
    seven_mask: u32,
    flush_off:  u32,
    flush_mask: u32,
};
@group(0) @binding(3) var<uniform> params: Params;

// ---- cham-core constants (mirror eval/mod.rs) ----
const CAT_HIGH:           u32 = 0u;
const CAT_PAIR:           u32 = 1u;
const CAT_TWO_PAIR:       u32 = 2u;
const CAT_TRIPS:          u32 = 3u;
const CAT_STRAIGHT:       u32 = 4u;
const CAT_FLUSH:          u32 = 5u;
const CAT_FULL_HOUSE:     u32 = 6u;
const CAT_QUADS:          u32 = 7u;
const CAT_STRAIGHT_FLUSH: u32 = 8u;
const W5:                 u32 = 15u;

// primes per rank (mirror cham-core)
const PRIMES: array<u32, 13> = array<u32, 13>(
    2u, 3u, 5u, 7u, 11u, 13u, 17u, 19u, 23u, 29u, 31u, 37u, 41u
);

// pack(cat, [k0..k4]) = cat*15^5 + k0*15^4 + k1*15^3 + k2*15^2 + k3*15 + k4
fn pack5(cat: u32, k0: u32, k1: u32, k2: u32, k3: u32, k4: u32) -> u32 {
    var mul: u32 = 15u * 15u * 15u * 15u;   // 15^4
    var v: u32 = cat * (15u * 15u * 15u * 15u * 15u);
    v += k0 * mul; mul /= 15u;
    v += k1 * mul; mul /= 15u;
    v += k2 * mul; mul /= 15u;
    v += k3 * mul; mul /= 15u;
    v += k4 * mul;
    return v;
}

// 32-bit Wang-style hash (used only when the key wasn't dense-reducible;
// kept here for reference — the actual lookup uses the dense u32 key
// directly against the reindexed map, so no hash is needed).
// fn hash32(x: u32) -> u32 {
//     var h: u32 = x;
//     h = (h ^ 61u) ^ (h >> 16u);
//     h = h + (h << 3u);
//     h = h ^ (h >> 4u);
//     h = h * 0x27d4eb2du;
//     h = h ^ (h >> 15u);
//     return h;
// }

// Dense-map lookup: the pack step in Rust reindexes the whole key space
// to consecutive u32 ordinals, and the map is stored as a sorted array
// of (key_u32, val_u32) that we scan with a small binary search. This
// avoids needing a hash and keeps the kernel trivially deterministic.
fn dense_lookup(off_words: u32, count_log2: u32, key: u32) -> u32 {
    // count_log2 is the log2 of the number of entries (both maps are
    // power-of-two sizes at pack time). Binary search over the sorted
    // (key, val) pairs.
    var lo: u32 = 0u;
    var hi: u32 = (1u << count_log2) - 1u;
    loop {
        if lo > hi { return 0u; }
        let mid: u32 = (lo + hi) / 2u;
        let base: u32 = off_words + mid * 2u;
        let k: u32 = tables.data[base];
        if k == key { return tables.data[base + 1u]; }
        if k < key { lo = mid + 1u; } else { hi = mid - 1u; }
    }
}

@compute @workgroup_size(64)
fn eval7_kernel(@builtin(global_invocation_id) id: vec3<u32>) {
    let tid: u32 = id.x;
    if tid >= params.hand_count { return; }

    let packed: u64 = hands.data[tid];
    var suit_mask:  array<u32, 4> = array<u32, 4>(0u, 0u, 0u, 0u);
    var suit_count: array<u32, 4> = array<u32, 4>(0u, 0u, 0u, 0u);
    var prod: u64 = 1u;

    for (var i: u32 = 0u; i < 7u; i = i + 1u) {
        let shift: u64 = 6u * u64(i);
        let idx: u32 = u32((packed >> shift) & 0x3Fu);
        let rank: u32 = idx >> 2u;
        let suit: u32 = idx & 3u;
        suit_mask[suit]  = suit_mask[suit]  | (1u << rank);
        suit_count[suit] = suit_count[suit] + 1u;
        prod = prod * u64(PRIMES[rank]);
    }

    // flush-suit select (mirrors evaluate7's if-chain)
    var fs: i32 = -1;
    for (var s: u32 = 0u; s < 4u; s = s + 1u) {
        if suit_count[s] >= 5u { fs = i32(s); break; }
    }

    if fs < 0 {
        // non-flush: prod is the prime product; the pack step must have
        // produced a map keyed by prod directly (u64 -> u32 not needed
        // here because prime products fit comfortably in u32: max is
        // 41^7 = 1.9e11? No — 41^7 overflows u32. Hence prod is u64 and
        // the map key is a REDUCED index. The pack step ships a parallel
        // array `prod_to_key: array<u64>`? See design doc; for now the
        // kernel signature assumes the CALLER pre-reduced via a parallel
        // lookup — this is the design doc's "option B". Concretely: the
        // pack step produces a map from prod (u64) to dense index, and
        // the caller passes the per-hand dense index in a parallel
        // buffer. Implementation detail deferred to the pack function.
        out.data[tid] = 0u;   // placeholder — see kernel TODO below
        return;
    }

    let m: u32 = suit_mask[u32(fs)];
    let st_word: u32 = tables.data[m / 4u];
    let st: u32 = (st_word >> ((m & 3u) * 8u)) & 0xFFu;
    if st != 0xFFu {
        let packed_val: u32 = pack5(CAT_STRAIGHT_FLUSH, st, 0u, 0u, 0u, 0u);
        out.data[tid] = dense_lookup(params.flush_off, params.flush_mask, packed_val);
        return;
    }
    // flush_top5
    var ks: array<u32, 5> = array<u32, 5>(0u, 0u, 0u, 0u, 0u);
    var n: u32 = 0u;
    for (var r: i32 = 12; r >= 0; r = r - 1) {
        if n >= 5u { break; }
        if (m & (1u << u32(r))) != 0u { ks[n] = u32(r); n = n + 1u; }
    }
    let packed_val: u32 = pack5(CAT_FLUSH, ks[0], ks[1], ks[2], ks[3], ks[4]);
    out.data[tid] = dense_lookup(params.flush_off, params.flush_mask, packed_val);
}

// ─────────────────────────────────────────────────────────────────────────
// KERNEL TODO (design open item, tracked in docs/GPU-G5.0-WGPU-PORT-DESIGN.md)
//
// The non-flush branch above has a placeholder because 41^7 overflows u32.
// WGSL has no u64 scalar multiplication on all backends. Two clean fixes:
//
//   (A) Split the prime product into (hi, lo) u32 halves in the kernel and
//       use a two-limb compare against a pre-split table. Correct but
//       fiddly, and this is exactly the "non-mechanical" area the design
//       doc says to avoid.
//
//   (B) Reduce the domain in Rust. The 7-card prime products take only
//       ~60k distinct values. At pack time, build a sorted array of
//       (prod_u64, dense_key_u32) and ship it as an extra buffer; the
//       kernel does a u64-keyed binary search to get the dense key, then
//       a u32 lookup. Slower per-eval but mechanical.
//
//   (C) Cleanest: pre-reduce in Rust is impossible because prod depends
//       on the hand, not just the tables. So we must either split-limb
//       in-kernel (A) or use a different key function (e.g. a perfect
//       hash over rank-multisets) at pack time.
//
// Recommendation: adopt (A) — a small, well-tested `mul_u64_u32`-style
// two-limb helper — because it keeps the kernel self-contained and does
// not double the memory traffic the way (B) would. This is the piece of
// G5.0 that genuinely needs care; the rest of the kernel is mechanical.
// The next session wires this up, writes WgpuContext, and lands the
// consistency test arm.
// ─────────────────────────────────────────────────────────────────────────
