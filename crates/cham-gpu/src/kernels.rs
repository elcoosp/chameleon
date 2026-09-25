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
