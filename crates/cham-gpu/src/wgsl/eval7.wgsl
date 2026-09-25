// WGSL port of crates/cham-gpu/src/msl/eval7.msl (GPU-PLAN G5.0).
//
// Behavioural transcription — SAME input/output as the MSL kernel, but the
// non-flush path uses multiset rank instead of a prime-product hash, because
// WGSL has no u64 multiply. Final u16 output is bit-identical to evaluate7;
// see `consistency_eval7_one_million_hands`.
//
// Buffers (all u32-native; WGSL has no scalar u8/u16 in storage buffers):
//
//   tables (storage, read):
//     word [0         .. 2048)          straight[8192] as 2048 u32 (LE)
//     word [multiset_off .. +50388)     seven_multiset_ranks[] (u32)
//     word [flush_off   .. +2*N)        flush_sorted: (key_u32, val_u32)*
//
//   hands (storage, read):
//     word [2*i]   = low 32 bits of packed hand i (6 bits/card, LE)
//     word [2*i+1] = high 32 bits (bits 32..41 of the 42-bit hand)
//
//   out (storage, read_write):
//     word [i] = u16 value in the low 16 bits

struct Tables { data: array<u32> };
struct Out    { data: array<u32> };

@group(0) @binding(0) var<storage, read>       tables: Tables;
@group(0) @binding(1) var<storage, read>       hands:  array<u32>;
@group(0) @binding(2) var<storage, read_write> out:    Out;

struct Params {
    hand_count:   u32,
    multiset_off: u32,
    flush_off:    u32,
    flush_count:  u32,
    _pad0:        u32,
    _pad1:        u32,
    _pad2:        u32,
    _pad3:        u32,
};
@group(0) @binding(3) var<uniform> params: Params;

const CAT_FLUSH:          u32 = 5u;
const CAT_STRAIGHT_FLUSH: u32 = 8u;

fn nck(n: u32, k: u32) -> u32 {
    if k > n { return 0u; }
    let kk: u32 = min(k, n - k);
    var r: u32 = 1u;
    for (var i: u32 = 0u; i < kk; i = i + 1u) {
        r = r * (n - i) / (i + 1u);
    }
    return r;
}

// Multiset-combinadic rank of a 7-card rank multiset.
fn multiset_rank(rank_counts: ptr<function, array<u32, 13>>) -> u32 {
    var rank: u32 = 0u;
    var n: u32 = 0u;
    for (var i: u32 = 0u; i < 13u; i = i + 1u) {
        let c: u32 = (*rank_counts)[i];
        for (var j: u32 = 0u; j < c; j = j + 1u) {
            rank = rank + nck(i + n, n + 1u);
            n = n + 1u;
        }
    }
    return rank;
}

fn pack5(cat: u32, k0: u32, k1: u32, k2: u32, k3: u32, k4: u32) -> u32 {
    var mul: u32 = 15u * 15u * 15u * 15u;
    var v: u32 = cat * (15u * 15u * 15u * 15u * 15u);
    v = v + k0 * mul; mul = mul / 15u;
    v = v + k1 * mul; mul = mul / 15u;
    v = v + k2 * mul; mul = mul / 15u;
    v = v + k3 * mul; mul = mul / 15u;
    v = v + k4 * mul;
    return v;
}

fn straight_get(off_words: u32, m: u32) -> u32 {
    let w: u32 = tables.data[off_words + (m >> 2u)];
    return (w >> ((m & 3u) * 8u)) & 0xFFu;
}

fn flush_lookup(off_words: u32, count: u32, key: u32) -> u32 {
    // naga rejects `loop { ... }` where the exit is unreachable (every
    // path returns or diverges); use `while` with an explicit result and
    // a `break` so control flow has a defined exit value.
    var lo: u32 = 0u;
    var hi: u32 = count;
    var result: u32 = 0u;
    while lo < hi {
        let mid: u32 = (lo + hi) / 2u;
        let k: u32 = tables.data[off_words + mid * 2u];
        if k == key {
            result = tables.data[off_words + mid * 2u + 1u];
            break;
        }
        if k < key { lo = mid + 1u; } else { hi = mid; }
    }
    return result;
}

fn extract_card(lo: u32, hi: u32, i: u32) -> u32 {
    let shift: u32 = 6u * i;
    if shift + 6u <= 32u {
        return (lo >> shift) & 0x3Fu;
    }
    if shift >= 32u {
        return (hi >> (shift - 32u)) & 0x3Fu;
    }
    // straddle (only possible when shift == 30, i.e. card index 5)
    let from_lo: u32 = 32u - shift;
    let low_part: u32  = (lo >> shift) & ((1u << from_lo) - 1u);
    let high_part: u32 = (hi & ((1u << (6u - from_lo)) - 1u)) << from_lo;
    return low_part | high_part;
}

@compute @workgroup_size(64)
fn eval7_kernel(@builtin(global_invocation_id) id: vec3<u32>) {
    let tid: u32 = id.x;
    if tid >= params.hand_count { return; }

    let lo: u32 = hands[tid * 2u];
    let hi: u32 = hands[tid * 2u + 1u];

    var suit_mask:   array<u32, 4>  = array<u32, 4>(0u, 0u, 0u, 0u);
    var suit_count:  array<u32, 4>  = array<u32, 4>(0u, 0u, 0u, 0u);
    var rank_counts: array<u32, 13> = array<u32, 13>(
        0u, 0u, 0u, 0u, 0u, 0u, 0u, 0u, 0u, 0u, 0u, 0u, 0u
    );

    for (var i: u32 = 0u; i < 7u; i = i + 1u) {
        let idx:  u32 = extract_card(lo, hi, i);
        let rank: u32 = idx >> 2u;
        let suit: u32 = idx & 3u;
        suit_mask[suit]   = suit_mask[suit]   | (1u << rank);
        suit_count[suit]  = suit_count[suit]  + 1u;
        rank_counts[rank] = rank_counts[rank] + 1u;
    }

    // flush-suit select (mirrors evaluate7's if-chain)
    var fs: i32 = -1;
    for (var s: u32 = 0u; s < 4u; s = s + 1u) {
        if suit_count[s] >= 5u { fs = i32(s); break; }
    }

    if fs < 0 {
        let mrank: u32 = multiset_rank(&rank_counts);
        out.data[tid] = tables.data[params.multiset_off + mrank];
        return;
    }

    let m: u32 = suit_mask[u32(fs)];
    let st: u32 = straight_get(0u, m);
    if st != 0xFFu {
        let pv: u32 = pack5(CAT_STRAIGHT_FLUSH, st, 0u, 0u, 0u, 0u);
        out.data[tid] = flush_lookup(params.flush_off, params.flush_count, pv);
        return;
    }

    var ks: array<u32, 5> = array<u32, 5>(0u, 0u, 0u, 0u, 0u);
    var n: u32 = 0u;
    for (var r: i32 = 12; r >= 0; r = r - 1) {
        if n >= 5u { break; }
        if (m & (1u << u32(r))) != 0u { ks[n] = u32(r); n = n + 1u; }
    }
    let pv: u32 = pack5(CAT_FLUSH, ks[0], ks[1], ks[2], ks[3], ks[4]);
    out.data[tid] = flush_lookup(params.flush_off, params.flush_count, pv);
}
