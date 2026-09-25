//! Device discovery: `GpuDevice::Available { name }` on macOS + feature,
//! else `Unavailable(reason)`. Never fails; callers fall back to CPU on
//! `Unavailable`.

/// Result of probing the host for a usable Metal device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GpuDevice {
    /// A Metal device is present (macOS + `--features metal`).
    Available { name: String },
    /// Reason the GPU path cannot be used (non-macOS host, feature off, or
    /// no Metal device). Free-form for `gpu-doctor` display.
    Unavailable(String),
}

impl GpuDevice {
    pub fn is_available(&self) -> bool {
        matches!(self, GpuDevice::Available { .. })
    }
}

#[cfg(all(target_os = "macos", feature = "metal"))]
pub fn probe() -> GpuDevice {
    match metal::Device::system_default() {
        Some(d) => GpuDevice::Available {
            name: d.name().to_string(),
        },
        None => GpuDevice::Unavailable("no system-default Metal device".into()),
    }
}

#[cfg(not(all(target_os = "macos", feature = "metal")))]
pub fn probe() -> GpuDevice {
    #[cfg(not(target_os = "macos"))]
    let reason = format!("non-macOS host: {}", std::env::consts::OS);
    #[cfg(target_os = "macos")]
    let reason = "metal feature not enabled (build with --features metal)".to_string();
    GpuDevice::Unavailable(reason)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_never_panics() {
        let d = probe();
        // Feature off / non-macOS → Unavailable with a non-empty reason.
        // Feature on + macOS + device present → Available with non-empty name.
        match d {
            GpuDevice::Available { name } => assert!(!name.is_empty()),
            GpuDevice::Unavailable(r) => assert!(!r.is_empty()),
        }
    }
}
