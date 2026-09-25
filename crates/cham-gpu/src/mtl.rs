//! Metal FFI shim — the only module in cham-gpu that touches `unsafe`.
//!
//! SAFETY CONTRACT (D-001 / memmap2 pattern): all raw-pointer work is
//! confined here. The wrapper takes safe `&[T]` / `&mut [T]` and the caller
//! never sees a pointer. Crate-level `#![deny(unsafe_code)]` holds everywhere
//! else; the `#[allow(unsafe_code)]` is scoped to `mod mtl;` at its
//! declaration site in lib.rs.
//!
//! In metal 0.31 the buffer-construction and `set_bytes` APIs are already
//! safe (the crate internalises the raw-pointer call); the sole `unsafe`
//! block below is the `contents()` → `&[u16]` slice cast.

use crate::kernels::KernelError;

#[cfg(all(target_os = "macos", feature = "metal"))]
pub(crate) fn dispatch_eval7(
    tables_bytes: &[u8],
    hands: &[u64],
    out: &mut [u16],
    seven_off: u64,
    seven_mask: u64,
    flush_off: u64,
    flush_mask: u64,
) -> Result<(), KernelError> {
    use metal::{CompileOptions, Device, MTLResourceOptions, MTLSize};

    if hands.len() != out.len() {
        return Err(KernelError::Metal("hands/out length mismatch".into()));
    }
    let n = hands.len() as u64;
    if n == 0 {
        return Ok(());
    }

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

    // All of these metal-0.31 calls are safe wrappers over the raw FFI.
    let tables_buf = device.new_buffer_with_data(
        tables_bytes.as_ptr() as *const std::ffi::c_void,
        tables_bytes.len() as u64,
        MTLResourceOptions::StorageModeShared,
    );
    let hands_buf = device.new_buffer_with_data(
        hands.as_ptr() as *const std::ffi::c_void,
        (hands.len() as u64) * 8,
        MTLResourceOptions::StorageModeShared,
    );
    let out_buf = device.new_buffer(
        (out.len() as u64) * 2,
        MTLResourceOptions::StorageModeShared,
    );

    let hand_count: u32 = n as u32;

    let cmd = queue.new_command_buffer();
    let enc = cmd.new_compute_command_encoder();
    enc.set_compute_pipeline_state(&pipeline);
    enc.set_buffer(0, Some(&tables_buf), 0);
    enc.set_buffer(1, Some(&hands_buf), 0);
    enc.set_buffer(2, Some(&out_buf), 0);
    enc.set_bytes(3, 4, &hand_count as *const u32 as *const std::ffi::c_void);
    enc.set_bytes(4, 8, &seven_off as *const u64 as *const std::ffi::c_void);
    enc.set_bytes(5, 8, &seven_mask as *const u64 as *const std::ffi::c_void);
    enc.set_bytes(6, 8, &flush_off as *const u64 as *const std::ffi::c_void);
    enc.set_bytes(7, 8, &flush_mask as *const u64 as *const std::ffi::c_void);

    let grid = MTLSize {
        width: n,
        height: 1,
        depth: 1,
    };
    let tg = MTLSize {
        width: 64,
        height: 1,
        depth: 1,
    };
    enc.dispatch_threads(grid, tg);
    enc.end_encoding();
    cmd.commit();
    cmd.wait_until_completed();

    // SAFETY: wait_until_completed has returned, so the shared buffer holds
    // the kernel's output for this command buffer. The buffer's contents are
    // valid for `out.len()` u16 values (we allocated exactly that many), and
    // the buffer outlives this block. This is the sole unsafe in the crate.
    let src = out_buf.contents() as *const u16;
    let gpu_out = unsafe { std::slice::from_raw_parts(src, out.len()) };
    out.copy_from_slice(gpu_out);
    Ok(())
}
