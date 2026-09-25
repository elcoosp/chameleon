//! cham-gpu: feature-gated Apple Metal accelerator (docs/GPU-PLAN.md).
//!
//! The `metal` feature is macOS-only; all GPU code paths are cfg-gated behind
//! it. Without the feature (or on other platforms) the crate compiles to a
//! no-op surface that reports `GpuDevice::Unavailable` — every caller keeps
//! today's CPU behavior unchanged.
//!
//! Whitelist amendment (SPECS/00 §2): this crate adds ONE new direct
//! dependency, `metal`, behind the `metal` feature. Its `objc2` transitive
//! family carries internal FFI unsafe at the boundary, scoped exactly like
//! the memmap2 carve-out (D-001); cham-gpu's own code stays
//! `#![forbid(unsafe_code)]`.

#![forbid(unsafe_code)]

pub mod device;
pub mod kernels;
pub mod noop;

pub use device::{GpuDevice, probe};
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
