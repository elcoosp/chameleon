//! `chameleon gpu-doctor` (GPU-PLAN G0.1): diagnostic probe.
//!
//! Prints OS, cham-gpu feature state, and either the Metal device name or the
//! reason the GPU path is unavailable. Always exits 0 — this is diagnostic,
//! never a gate.

pub fn run() -> i32 {
    println!("gpu-doctor: cham-gpu v{}", cham_gpu::CRATE_VERSION);
    println!("gpu-doctor: os = {}", std::env::consts::OS);
    // cham-cli itself never enables the metal feature (cham-gpu does, per
    // its own Cargo.toml). gpu-doctor reports what cham-gpu sees at runtime.
    println!(
        "gpu-doctor: cham-gpu compiled metal feature = {}",
        metal_on()
    );
    match cham_gpu::probe() {
        cham_gpu::GpuDevice::Available { name } => {
            println!("gpu-doctor: device = {name}");
            println!("gpu-doctor: verdict = AVAILABLE (GPU path usable)");
        }
        cham_gpu::GpuDevice::Unavailable(reason) => {
            println!("gpu-doctor: device = <none>");
            println!("gpu-doctor: reason = {reason}");
            println!("gpu-doctor: verdict = UNAVAILABLE (CPU fallback is the active path)");
        }
    }
    println!(
        "gpu-doctor: whitelist amendment — `metal` added to the workspace whitelist \
         per docs/GPU-PLAN.md Part II (scoped FFI, feature-gated, macOS-only)."
    );
    crate::cmd::EXIT_OK
}

/// Whether the cham-gpu crate was compiled with its `metal` feature. Since
/// cham-cli does not transitively turn that feature on by default, this is a
/// probe of what the (already-compiled) cham-gpu exposes.
fn metal_on() -> &'static str {
    // cham-gpu exposes availability through probe(); the feature state is
    // reflected there, not via cfg! in this crate.
    if cham_gpu::probe().is_available() {
        "on (device visible)"
    } else {
        "off or device unavailable"
    }
}
