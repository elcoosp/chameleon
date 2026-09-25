//! GPU kernel launchers (GPU-PLAN G0.2).
//!
//! Everything here is behind `#[cfg(all(target_os = "macos", feature = "metal"))]`
//! so the crate compiles to a no-op surface everywhere else.
//!
//! API surface (planned, stabilizes once cham-core's `eval_tables()` ships):
//! - `pub fn launch_eval7(tables: &[u8], hands_packed: &[u64], out: &mut [u16])`
//!   → dispatches MSL `eval7_kernel` with the exact table bytes.
//! - The meta offsets/masks (seven_off, seven_mask, flush_off, flush_mask) are
//!   computed by `cham_core::eval::eval_tables()` and shipped alongside the bytes.

/// Error type scoped to kernel launches.
#[derive(Debug, thiserror::Error)]
pub enum KernelError {
    #[error("no GPU available: {0}")]
    NoDevice(String),
    #[error("metal setup: {0}")]
    Metal(String),
}

/// Whether GPU kernel dispatch is possible right now. Callers use this to
/// decide CPU-vs-GPU without inspecting feature cfgs.
pub fn can_dispatch() -> bool {
    crate::probe().is_available()
}

#[cfg(all(target_os = "macos", feature = "metal"))]
pub fn launch_eval7(
    _tables: &[u8],
    _hands_packed: &[u64],
    _out: &mut [u16],
) -> Result<(), KernelError> {
    // Real Metal dispatch lands once cham-core's eval_tables() API ships
    // (G0.2 step 1). Left as TODO so the crate compiles cleanly now.
    Err(KernelError::Metal(
        "launch_eval7 body pending eval_tables() API".into(),
    ))
}

#[cfg(not(all(target_os = "macos", feature = "metal")))]
pub fn launch_eval7(
    _tables: &[u8],
    _hands_packed: &[u64],
    _out: &mut [u16],
) -> Result<(), KernelError> {
    Err(KernelError::NoDevice(
        crate::probe().unwrap_or_unavailable_reason(),
    ))
}

// Convenience for the no-device branch above.
#[cfg(not(all(target_os = "macos", feature = "metal")))]
impl crate::GpuDevice {
    pub(crate) fn unwrap_or_unavailable_reason(&self) -> String {
        match self {
            crate::GpuDevice::Unavailable(r) => r.clone(),
            crate::GpuDevice::Available { .. } => "available but feature off".into(),
        }
    }
}
