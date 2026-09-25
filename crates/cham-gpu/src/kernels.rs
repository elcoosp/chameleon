//! GPU kernel launchers (GPU-PLAN G0.2 + post-G0.3 revision).

use cham_core::eval::EvalTables;

#[cfg(all(target_os = "macos", feature = "metal"))]
pub use crate::mtl::GpuContext;

#[derive(Debug, thiserror::Error)]
pub enum KernelError {
    #[error("no GPU available: {0}")]
    NoDevice(String),
    #[error("metal setup: {0}")]
    Metal(String),
}

pub fn can_dispatch() -> bool {
    crate::probe().is_available()
}

/// Pack a 7-card hand into 6-bit-per-card little-endian (42 bits used).
pub fn pack_hand(hand: &[cham_core::card::Card; 7]) -> u64 {
    let mut v = 0u64;
    for (i, c) in hand.iter().enumerate() {
        v |= ((c.0 as u64) & 0x3F) << (6 * i as u64);
    }
    v
}

/// Pack an `EvalTables` view into the exact byte layout the MSL kernel
/// expects: `straight[8192] || seven_entries*16B || flush_entries*16B`, where
/// each entry is `(u64 key LE || u16 val LE || 6 pad)`.
pub fn pack_tables(tables: &EvalTables<'_>) -> (Vec<u8>, u64, u64, u64, u64) {
    let mut packed: Vec<u8> =
        Vec::with_capacity(8192 + (tables.seven_entries.len() + tables.flush_entries.len()) * 16);
    packed.extend_from_slice(tables.straight);
    for &(k, v) in tables.seven_entries {
        packed.extend_from_slice(&k.to_le_bytes());
        packed.extend_from_slice(&v.to_le_bytes());
        packed.extend_from_slice(&[0u8; 6]);
    }
    let seven_off: u64 = 8192;
    let seven_mask: u64 = tables.seven_mask;
    for &(k, v) in tables.flush_entries {
        packed.extend_from_slice(&k.to_le_bytes());
        packed.extend_from_slice(&v.to_le_bytes());
        packed.extend_from_slice(&[0u8; 6]);
    }
    let flush_off: u64 = 8192 + (tables.seven_entries.len() as u64) * 16;
    let flush_mask: u64 = tables.flush_mask;
    (packed, seven_off, seven_mask, flush_off, flush_mask)
}

/// Pack the u32-native table buffer for the WGSL backend.
///
/// Layout (u32-word index): `[0..2048) straight` || `multiset_ranks` ||
/// `flush_sorted` as `(key_u32, val_u32)` pairs. Returns
/// `(bytes, multiset_off, flush_off, flush_count)` — offsets are in u32
/// words, matching the WGSL `Params` struct.
#[cfg(feature = "wgpu")]
pub fn pack_tables_wgsl(tables: &EvalTables<'_>) -> (Vec<u8>, u32, u32, u32) {
    let mut data: Vec<u32> = Vec::with_capacity(
        2048 + tables.seven_multiset_ranks.len() + tables.flush_sorted.len() * 2,
    );
    // straight (8192 bytes → 2048 u32, LE)
    for chunk in tables.straight.chunks(4) {
        let mut w = [0u8; 4];
        w[..chunk.len()].copy_from_slice(chunk);
        data.push(u32::from_le_bytes(w));
    }
    let multiset_off: u32 = data.len() as u32;
    data.extend_from_slice(tables.seven_multiset_ranks);
    let flush_off: u32 = data.len() as u32;
    let flush_count: u32 = tables.flush_sorted.len() as u32;
    for (k, v) in tables.flush_sorted {
        data.push(*k);
        data.push(*v as u32);
    }
    let mut bytes = Vec::with_capacity(data.len() * 4);
    for w in &data {
        bytes.extend_from_slice(&w.to_le_bytes());
    }
    (bytes, multiset_off, flush_off, flush_count)
}

/// Pack hands into the WGSL layout: `[lo_u32, hi_u32]` per hand, LE bytes.
#[cfg(feature = "wgpu")]
pub fn pack_hands_wgsl(hands_packed: &[u64]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(hands_packed.len() * 8);
    for h in hands_packed {
        bytes.extend_from_slice(&(*h as u32).to_le_bytes());
        bytes.extend_from_slice(&((*h >> 32) as u32).to_le_bytes());
    }
    bytes
}

/// Dispatch `eval7_kernel` over `hands_packed`, writing `out[i]`.
#[cfg(all(target_os = "macos", feature = "metal"))]
pub fn launch_eval7(
    ctx: &GpuContext,
    tables: &EvalTables<'_>,
    hands_packed: &[u64],
    out: &mut [u16],
) -> Result<(), KernelError> {
    let (packed, so, sm, fo, fm) = pack_tables(tables);
    crate::mtl::dispatch_eval7(ctx, &packed, hands_packed, out, so, sm, fo, fm)
}

#[cfg(not(all(target_os = "macos", feature = "metal")))]
pub fn launch_eval7(
    _ctx: &GpuContext,
    _tables: &EvalTables<'_>,
    _hands_packed: &[u64],
    _out: &mut [u16],
) -> Result<(), KernelError> {
    let reason = match crate::probe() {
        crate::GpuDevice::Unavailable(r) => r,
        crate::GpuDevice::Available { .. } => "available but metal feature off".into(),
    };
    Err(KernelError::NoDevice(reason))
}

/// Convenience: compile-once + dispatch-many. On non-macOS, returns the
/// `NoDevice` error from `GpuContext::new`.
#[cfg(not(all(target_os = "macos", feature = "metal")))]
#[derive(Debug)]
pub struct GpuContext;

#[cfg(not(all(target_os = "macos", feature = "metal")))]
impl GpuContext {
    pub fn new() -> Result<Self, KernelError> {
        Err(KernelError::NoDevice("metal unavailable".into()))
    }
    pub fn name(&self) -> String {
        "cpu".into()
    }
}
