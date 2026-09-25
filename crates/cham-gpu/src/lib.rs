//! cham-gpu: feature-gated Apple Metal accelerator (docs/GPU-PLAN.md).
//!
//! The `metal` feature is macOS-only; all GPU code paths are cfg-gated behind
//! it. Without the feature (or on other platforms) the crate compiles to a
//! no-op surface that reports `GpuDevice::Unavailable` — every caller keeps
//! today's CPU behavior unchanged.
//!
//! Whitelist amendment (SPECS/00 §2): this crate adds ONE new direct
//! dependency, `metal`, behind the `metal` feature. Its `objc2` transitive
//! family carries internal FFI unsafe at the boundary.
//!
//! SAFETY POSTURE — deviation from the plan's literal wording:
//! docs/GPU-PLAN.md Part II says "cham-gpu's own code stays
//! `#![forbid(unsafe_code)]`". That is literally impossible: the `metal`
//! crate's low-level API (`new_buffer_with_data`, `set_bytes`,
//! `contents`) is `unsafe fn` because it takes raw pointers. We therefore
//! follow the plan's cited D-001 / memmap2 pattern *exactly*: crate-level
//! `#![deny(unsafe_code)]`, with `#[allow(unsafe_code)]` scoped to the
//! single FFI shim module `mtl`. Every other module in this crate is
//! unsafe-free. Recorded in worklog.

pub mod device;
pub mod kernels;
pub mod noop;

#[cfg(all(target_os = "macos", feature = "metal"))]
#[allow(unsafe_code)] // the D-001 / memmap2-scoped FFI shim
mod mtl;

pub use device::{GpuDevice, probe};
pub use kernels::GpuContext;
pub use noop::GpuNoop;

/// Crate version surfaced in `chameleon gpu-doctor`.
pub const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod crate_tests {
    use super::*;

    /// The default-feature build must compile and construct the noop surface;
    /// this is the Linux CI guarantee (metal feature off everywhere except a
    /// macOS developer build).
    #[test]
    fn gpu_crate_compiles_without_feature() {
        let n = GpuNoop::new();
        assert!(n.device_name().is_none());
        // probe() is never a panic — it returns Unavailable on Linux / feature-off.
        match probe() {
            GpuDevice::Available { name } => assert!(!name.is_empty()),
            GpuDevice::Unavailable(r) => assert!(!r.is_empty()),
        }
    }
}
