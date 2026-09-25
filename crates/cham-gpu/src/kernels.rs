//! GPU kernel launchers (GPU-PLAN G0.2).
//!
//! Real Metal dispatch is behind `#[cfg(all(target_os = "macos", feature = "metal"))]`;
//! everywhere else, `launch_eval7` returns `KernelError::NoDevice` so callers
//! transparently fall back to CPU.

use cham_core::eval::EvalTables;

/// Error type scoped to kernel launches.
#[derive(Debug, thiserror::Error)]
pub enum KernelError {
    #[error("no GPU available: {0}")]
    NoDevice(String),
    #[error("metal setup: {0}")]
    Metal(String),
}

/// Whether GPU kernel dispatch is possible right now.
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

/// Dispatch `eval7_kernel` over `hands_packed`, writing `out[i]`.
///
/// Tables are packed once per call into the exact byte layout the MSL kernel
/// expects (see `msl/eval7.msl` header). The MSL kernel is bit-exact to
/// `cham_core::eval::evaluate7` — see the 1M-hand test in
/// `crates/cham-gpu/tests/consistency_eval7.rs`.
#[cfg(all(target_os = "macos", feature = "metal"))]
pub fn launch_eval7(
    tables: &EvalTables<'_>,
    hands_packed: &[u64],
    out: &mut [u16],
) -> Result<(), KernelError> {
    // Packed tables buffer: [straight 8192 bytes][seven entries 16B each][flush entries 16B each]
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

    crate::mtl::dispatch_eval7(
        &packed,
        hands_packed,
        out,
        seven_off,
        seven_mask,
        flush_off,
        flush_mask,
    )
}

/// CPU-only fallback path: the GPU is not present (feature off or non-macOS).
/// Callers should route to `evaluate7` themselves; this exists to keep the
/// signature stable across feature states.
#[cfg(not(all(target_os = "macos", feature = "metal")))]
pub fn launch_eval7(
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
