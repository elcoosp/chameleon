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
    /// Eval7 pipeline (compiled in `new()` — the only one used per dispatch).
    pipeline: metal::ComputePipelineState,
    /// L-25 fix (2026-09-27): EHS kernels used to recompile MSL on EVERY
    /// dispatch (~100 ms each on M1; a full `gpu-build --kind flop` run
    /// spent ~9 min in pure recompilation). The pipelines are content-
    /// invariant, so cache them lazily: the first `dispatch_ehs_*` call
    /// compiles; every later call reuses the cell. `OnceLock` is
    /// thread-safe and preserves determinism (one compile, one result).
    ehs_turn_pipeline: std::sync::OnceLock<metal::ComputePipelineState>,
    ehs_flop_pipeline: std::sync::OnceLock<metal::ComputePipelineState>,
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
        // Concatenate the shared inline helpers with the kernel entry
        // point. MSL has no #include; this is the standard workaround.
        let source = format!(
            "{}\n{}",
            include_str!("msl/eval7_shared.msl"),
            include_str!("msl/eval7.msl"),
        );
        let library = device
            .new_library_with_source(&source, &CompileOptions::new())
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
            ehs_turn_pipeline: std::sync::OnceLock::new(),
            ehs_flop_pipeline: std::sync::OnceLock::new(),
        })
    }

    pub fn name(&self) -> String {
        self.device.name().to_string()
    }
}

/// Turn EHS: dispatch one thread per (board, hole_index) pair.
/// `boards_packed[i]` is board i's 4 card bytes packed LE.
/// `out[(i*1326) + h]` receives the numerator.
#[cfg(all(target_os = "macos", feature = "metal"))]
pub(crate) fn dispatch_ehs_turn(
    ctx: &GpuContext,
    tables_bytes: &[u8],
    boards_packed: &[u32],
    out: &mut [u32],
    seven_off: u64,
    seven_mask: u64,
    flush_off: u64,
    flush_mask: u64,
) -> Result<(), KernelError> {
    use metal::{CompileOptions, MTLResourceOptions, MTLSize};

    let n_boards = boards_packed.len() as u64;
    if out.len() as u64 != n_boards * 1326 {
        return Err(KernelError::Metal(
            "out length mismatch (expect n_boards*1326)".into(),
        ));
    }

    // L-25: compile once per (device, kernel) and reuse. `OnceLock` caches
    // the pipeline for the lifetime of the GpuContext; a compile error
    // leaves the cell empty so a retry is possible but never silently uses
    // a stale pipeline.
    let pipeline = match ctx.ehs_turn_pipeline.get() {
        Some(p) => p,
        None => {
            let source = format!(
                "{}\n{}",
                include_str!("msl/eval7_shared.msl"),
                include_str!("msl/ehs_turn.msl"),
            );
            let library = ctx
                .device
                .new_library_with_source(&source, &CompileOptions::new())
                .map_err(|e| KernelError::Metal(format!("EHS MSL compile: {e:?}")))?;
            let function = library
                .get_function("ehs_turn_kernel", None)
                .map_err(|e| KernelError::Metal(format!("get_function ehs_turn: {e:?}")))?;
            let compiled = ctx
                .device
                .new_compute_pipeline_state_with_function(&function)
                .map_err(|e| KernelError::Metal(format!("ehs_turn pipeline: {e:?}")))?;
            let _ = ctx.ehs_turn_pipeline.set(compiled);
            ctx.ehs_turn_pipeline
                .get()
                .expect("just set the turn pipeline")
        }
    };

    let tables_buf = ctx.device.new_buffer_with_data(
        tables_bytes.as_ptr() as *const std::ffi::c_void,
        tables_bytes.len() as u64,
        MTLResourceOptions::StorageModeShared,
    );
    let boards_buf = ctx.device.new_buffer_with_data(
        boards_packed.as_ptr() as *const std::ffi::c_void,
        (boards_packed.len() as u64) * 4,
        MTLResourceOptions::StorageModeShared,
    );
    let out_buf = ctx.device.new_buffer(
        (out.len() as u64) * 4,
        MTLResourceOptions::StorageModeShared,
    );

    let board_count: u32 = n_boards as u32;
    let total: u64 = n_boards * 1326;

    let cmd = ctx.queue.new_command_buffer();
    let enc = cmd.new_compute_command_encoder();
    enc.set_compute_pipeline_state(&pipeline);
    enc.set_buffer(0, Some(&tables_buf), 0);
    enc.set_buffer(1, Some(&boards_buf), 0);
    enc.set_buffer(2, Some(&out_buf), 0);
    enc.set_bytes(3, 4, &board_count as *const u32 as *const std::ffi::c_void);
    enc.set_bytes(4, 8, &seven_off as *const u64 as *const std::ffi::c_void);
    enc.set_bytes(5, 8, &seven_mask as *const u64 as *const std::ffi::c_void);
    enc.set_bytes(6, 8, &flush_off as *const u64 as *const std::ffi::c_void);
    enc.set_bytes(7, 8, &flush_mask as *const u64 as *const std::ffi::c_void);
    enc.dispatch_threads(
        MTLSize {
            width: total,
            height: 1,
            depth: 1,
        },
        MTLSize {
            width: 64,
            height: 1,
            depth: 1,
        },
    );
    enc.end_encoding();
    cmd.commit();
    cmd.wait_until_completed();

    // SAFETY: wait_until_completed returned; shared buffer holds outputs.
    let src = out_buf.contents() as *const u32;
    let gpu_out = unsafe { std::slice::from_raw_parts(src, out.len()) };
    out.copy_from_slice(gpu_out);
    Ok(())
}

/// Non-macOS / feature-off stub.
#[cfg(not(all(target_os = "macos", feature = "metal")))]
pub(crate) fn dispatch_ehs_turn(
    _ctx: &GpuContext,
    _tables_bytes: &[u8],
    _boards_packed: &[u32],
    _out: &mut [u32],
    _so: u64,
    _sm: u64,
    _fo: u64,
    _fm: u64,
) -> Result<(), KernelError> {
    Err(KernelError::NoDevice("metal unavailable".into()))
}

/// Flop EHS: same shape as the turn dispatch, kernel `ehs_flop_kernel`.
#[cfg(all(target_os = "macos", feature = "metal"))]
pub(crate) fn dispatch_ehs_flop(
    ctx: &GpuContext,
    tables_bytes: &[u8],
    boards_packed: &[u32],
    out: &mut [u32],
    seven_off: u64,
    seven_mask: u64,
    flush_off: u64,
    flush_mask: u64,
) -> Result<(), KernelError> {
    use metal::{CompileOptions, MTLResourceOptions, MTLSize};

    let n_boards = boards_packed.len() as u64;
    if out.len() as u64 != n_boards * 1326 {
        return Err(KernelError::Metal(
            "out length mismatch (expect n_boards*1326)".into(),
        ));
    }

    // L-25: same caching as turn (see dispatch_ehs_turn).
    let pipeline = match ctx.ehs_flop_pipeline.get() {
        Some(p) => p,
        None => {
            let source = format!(
                "{}\n{}",
                include_str!("msl/eval7_shared.msl"),
                include_str!("msl/ehs_flop.msl"),
            );
            let library = ctx
                .device
                .new_library_with_source(&source, &CompileOptions::new())
                .map_err(|e| KernelError::Metal(format!("EHS flop MSL compile: {e:?}")))?;
            let function = library
                .get_function("ehs_flop_kernel", None)
                .map_err(|e| KernelError::Metal(format!("get_function ehs_flop: {e:?}")))?;
            let compiled = ctx
                .device
                .new_compute_pipeline_state_with_function(&function)
                .map_err(|e| KernelError::Metal(format!("ehs_flop pipeline: {e:?}")))?;
            let _ = ctx.ehs_flop_pipeline.set(compiled);
            ctx.ehs_flop_pipeline
                .get()
                .expect("just set the flop pipeline")
        }
    };

    let tables_buf = ctx.device.new_buffer_with_data(
        tables_bytes.as_ptr() as *const std::ffi::c_void,
        tables_bytes.len() as u64,
        MTLResourceOptions::StorageModeShared,
    );
    let boards_buf = ctx.device.new_buffer_with_data(
        boards_packed.as_ptr() as *const std::ffi::c_void,
        (boards_packed.len() as u64) * 4,
        MTLResourceOptions::StorageModeShared,
    );
    let out_buf = ctx.device.new_buffer(
        (out.len() as u64) * 4,
        MTLResourceOptions::StorageModeShared,
    );

    let board_count: u32 = n_boards as u32;
    let total: u64 = n_boards * 1326;

    let cmd = ctx.queue.new_command_buffer();
    let enc = cmd.new_compute_command_encoder();
    enc.set_compute_pipeline_state(&pipeline);
    enc.set_buffer(0, Some(&tables_buf), 0);
    enc.set_buffer(1, Some(&boards_buf), 0);
    enc.set_buffer(2, Some(&out_buf), 0);
    enc.set_bytes(3, 4, &board_count as *const u32 as *const std::ffi::c_void);
    enc.set_bytes(4, 8, &seven_off as *const u64 as *const std::ffi::c_void);
    enc.set_bytes(5, 8, &seven_mask as *const u64 as *const std::ffi::c_void);
    enc.set_bytes(6, 8, &flush_off as *const u64 as *const std::ffi::c_void);
    enc.set_bytes(7, 8, &flush_mask as *const u64 as *const std::ffi::c_void);
    enc.dispatch_threads(
        MTLSize {
            width: total,
            height: 1,
            depth: 1,
        },
        MTLSize {
            width: 64,
            height: 1,
            depth: 1,
        },
    );
    enc.end_encoding();
    cmd.commit();
    cmd.wait_until_completed();

    let src = out_buf.contents() as *const u32;
    let gpu_out = unsafe { std::slice::from_raw_parts(src, out.len()) };
    out.copy_from_slice(gpu_out);
    Ok(())
}

/// Non-macOS / feature-off stub.
#[cfg(not(all(target_os = "macos", feature = "metal")))]
pub(crate) fn dispatch_ehs_flop(
    _ctx: &GpuContext,
    _tables_bytes: &[u8],
    _boards_packed: &[u32],
    _out: &mut [u32],
    _so: u64,
    _sm: u64,
    _fo: u64,
    _fm: u64,
) -> Result<(), KernelError> {
    Err(KernelError::NoDevice("metal unavailable".into()))
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
