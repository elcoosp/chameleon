//! Metal FFI shim — the only module in cham-gpu that touches `unsafe`.
//!
//! SAFETY CONTRACT (D-001 / memmap2 pattern): all raw-pointer work is
//! confined here. Wrapper functions take safe `&[T]` / `&mut [T]`; the
//! caller never sees a pointer. Crate-level lints deny `unsafe_code`
//! everywhere else; `#[allow(unsafe_code)]` is scoped to `mod mtl;` at its
//! declaration site in lib.rs.
//!
//! ## Persistent context (post-G0.3 revision)
//!
//! The G0.3 probe timed the MSL compile inside every dispatch, which
//! dominated the timed window for small batches. `GpuContext` now owns the
//! compiled `Device`/`CommandQueue`/`ComputePipelineState`, so a caller
//! compiles once and dispatches many times. `dispatch_eval7` reads from the
//! context; its only per-call work is buffer creation, dispatch, and copy.

use crate::kernels::KernelError;

#[cfg(all(target_os = "macos", feature = "metal"))]
pub struct GpuContext {
    device: metal::Device,
    queue: metal::CommandQueue,
    pipeline: metal::ComputePipelineState,
}

#[cfg(all(target_os = "macos", feature = "metal"))]
impl GpuContext {
    /// Compile the `eval7_kernel` pipeline once. Reuse the returned context
    /// for every subsequent dispatch — the MSL compile is ~100 ms on M1 and
    /// is the dominant cost of a single small dispatch.
    pub fn new() -> Result<Self, KernelError> {
        use metal::{CompileOptions, Device};
        let device = Device::system_default()
            .ok_or_else(|| KernelError::NoDevice("no system-default Metal device".into()))?;
        let queue = device.new_command_queue();
        let source = include_str!("msl/eval7.msl");
        let library = device
            .new_library_with_source(source, &CompileOptions::new())
            .map_err(|e| KernelError::Metal(format!("MSL compile: {e:?}")))?;
        let function = library
            .get_function("eval7_kernel", None)
            .map_err(|e| KernelError::Metal(format!("get_function: {e:?}")))?;
        let pipeline = device
            .new_compute_pipeline_state_with_function(&function)
            .map_err(|e| KernelError::Metal(format!("pipeline: {e:?}")))?;
        Ok(Self {
            device,
            queue,
            pipeline,
        })
    }

    pub fn name(&self) -> String {
        self.device.name().to_string()
    }
}

#[cfg(not(all(target_os = "macos", feature = "metal")))]
#[derive(Debug)]
pub struct GpuContext;

#[cfg(not(all(target_os = "macos", feature = "metal")))]
impl GpuContext {
    pub fn new() -> Result<Self, KernelError> {
        Err(KernelError::NoDevice("metal unavailable".into()))
    }
}

#[cfg(all(target_os = "macos", feature = "metal"))]
pub(crate) fn dispatch_eval7(
    ctx: &GpuContext,
    tables_bytes: &[u8],
    hands: &[u64],
    out: &mut [u16],
    seven_off: u64,
    seven_mask: u64,
    flush_off: u64,
    flush_mask: u64,
) -> Result<(), KernelError> {
    use metal::MTLResourceOptions;

    if hands.len() != out.len() {
        return Err(KernelError::Metal("hands/out length mismatch".into()));
    }
    let n = hands.len() as u64;
    if n == 0 {
        return Ok(());
    }

    let tables_buf = ctx.device.new_buffer_with_data(
        tables_bytes.as_ptr() as *const std::ffi::c_void,
        tables_bytes.len() as u64,
        MTLResourceOptions::StorageModeShared,
    );
    let hands_buf = ctx.device.new_buffer_with_data(
        hands.as_ptr() as *const std::ffi::c_void,
        (hands.len() as u64) * 8,
        MTLResourceOptions::StorageModeShared,
    );
    let out_buf = ctx.device.new_buffer(
        (out.len() as u64) * 2,
        MTLResourceOptions::StorageModeShared,
    );

    let hand_count: u32 = n as u32;

    let cmd = ctx.queue.new_command_buffer();
    let enc = cmd.new_compute_command_encoder();
    enc.set_compute_pipeline_state(&ctx.pipeline);
    enc.set_buffer(0, Some(&tables_buf), 0);
    enc.set_buffer(1, Some(&hands_buf), 0);
    enc.set_buffer(2, Some(&out_buf), 0);
    enc.set_bytes(3, 4, &hand_count as *const u32 as *const std::ffi::c_void);
    enc.set_bytes(4, 8, &seven_off as *const u64 as *const std::ffi::c_void);
    enc.set_bytes(5, 8, &seven_mask as *const u64 as *const std::ffi::c_void);
    enc.set_bytes(6, 8, &flush_off as *const u64 as *const std::ffi::c_void);
    enc.set_bytes(7, 8, &flush_mask as *const u64 as *const std::ffi::c_void);
    enc.dispatch_threads(
        metal::MTLSize {
            width: n,
            height: 1,
            depth: 1,
        },
        metal::MTLSize {
            width: 64,
            height: 1,
            depth: 1,
        },
    );
    enc.end_encoding();
    cmd.commit();
    cmd.wait_until_completed();

    // SAFETY: wait_until_completed returned; the shared buffer holds the
    // kernel's output for `out.len()` u16 values; buffer outlives this block.
    let src = out_buf.contents() as *const u16;
    let gpu_out = unsafe { std::slice::from_raw_parts(src, out.len()) };
    out.copy_from_slice(gpu_out);
    Ok(())
}
