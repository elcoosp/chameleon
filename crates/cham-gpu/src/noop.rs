//! Always-available CPU-side stub. Exists so the crate's API surface is the
//! same with the `metal` feature on or off — callers can hold a `GpuNoop`
//! unconditionally and branch on `device_name()`.

/// Zero-cost stub; its method returns `None` (no device).
#[derive(Clone, Copy, Debug, Default)]
pub struct GpuNoop;

impl GpuNoop {
    pub fn new() -> Self {
        Self
    }

    /// Always `None` on the CPU fallback path.
    pub fn device_name(&self) -> Option<&'static str> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_is_inert() {
        let n = GpuNoop::new();
        assert!(n.device_name().is_none());
    }
}
