//! wgpu backend for the eval7 kernel (GPU-PLAN G5.0b).
//!
//! Feature-gated behind `wgpu`. Compiles on macOS (Metal), Linux (Vulkan),
//! and Windows (DX12/Vulkan) — one WGSL source, identical output.
//!
//! No `unsafe`: all bytes are written/read via slice iteration and
//! `u32::from_le_bytes` / `to_le_bytes`, so the crate-wide deny(unsafe_code)
//! stays intact.

#![cfg(feature = "wgpu")]

use crate::kernels::KernelError;

pub struct WgpuContext {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::ComputePipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    tables_buf: wgpu::Buffer,
    multiset_off: u32,
    flush_off: u32,
    flush_count: u32,
}

impl WgpuContext {
    /// Compile the eval7 WGSL pipeline and park the packed tables on the GPU
    /// once. Reuse for every subsequent dispatch.
    pub fn new(tables: &cham_core::eval::EvalTables<'_>) -> Result<Self, KernelError> {
        // wgpu 30: Instance::new takes the descriptor by value; use the
        // provided constructor rather than a struct literal (a field may
        // appear in a future minor that we would miss).
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = wgpu::Backends::all();
        let instance = wgpu::Instance::new(desc);
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
            // wgpu 30: required field, prevents hardware fingerprinting.
            // `false` = expose the device's true limits.
            apply_limit_buckets: false,
        }))
        .map_err(|e| KernelError::NoDevice(format!("wgpu adapter: {e}")))?;

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("cham-gpu"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
        }))
        .map_err(|e| KernelError::Metal(format!("wgpu device: {e}")))?;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("eval7"),
            source: wgpu::ShaderSource::Wgsl(include_str!("wgsl/eval7.wgsl").into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("eval7-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("eval7-pl"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("eval7-pipeline"),
            layout: Some(&layout),
            module: &shader,
            entry_point: Some("eval7_kernel"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });

        // Pack tables once.
        let (data, multiset_off, flush_off, flush_count) = crate::kernels::pack_tables_wgsl(tables);
        let tables_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("tables"),
            size: data.len() as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&tables_buf, 0, &data);

        Ok(Self {
            device,
            queue,
            pipeline,
            bind_group_layout,
            tables_buf,
            multiset_off,
            flush_off,
            flush_count,
        })
    }

    /// Human-readable backend name (for diagnostics only).
    pub fn name(&self) -> String {
        "wgpu".to_string()
    }

    /// Dispatch the eval7 kernel once over `hands_packed` (u64/hand).
    pub fn dispatch_eval7(&self, hands_packed: &[u64], out: &mut [u16]) -> Result<(), KernelError> {
        if hands_packed.len() != out.len() {
            return Err(KernelError::Metal("hands/out length mismatch".into()));
        }
        let n = hands_packed.len();
        if n == 0 {
            return Ok(());
        }

        let hands_bytes = crate::kernels::pack_hands_wgsl(hands_packed);

        let hands_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("hands"),
            size: hands_bytes.len() as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.queue.write_buffer(&hands_buf, 0, &hands_bytes);

        let out_bytes_len = (out.len() as u64) * 4;
        let out_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("out"),
            size: out_bytes_len,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_SRC
                | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("out-staging"),
            size: out_bytes_len,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let params: [u32; 8] = [
            n as u32,
            self.multiset_off,
            self.flush_off,
            self.flush_count,
            0,
            0,
            0,
            0,
        ];
        let mut params_bytes = [0u8; 32];
        for (i, w) in params.iter().enumerate() {
            params_bytes[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
        }
        let params_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("params"),
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.queue.write_buffer(&params_buf, 0, &params_bytes);

        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("eval7-bg"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.tables_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: hands_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: out_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: params_buf.as_entire_binding(),
                },
            ],
        });

        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("eval7-pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            let wgs = (n as u32).div_ceil(64);
            pass.dispatch_workgroups(wgs, 1, 1);
        }
        enc.copy_buffer_to_buffer(&out_buf, 0, &staging, 0, out_bytes_len);
        self.queue.submit(Some(enc.finish()));

        // Block until done, then read out.
        let slice = staging.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |res| {
            let _ = tx.send(res);
        });
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .map_err(|e| KernelError::Metal(format!("wgpu poll: {e}")))?;
        rx.recv()
            .map_err(|e| KernelError::Metal(format!("wgpu map recv: {e}")))?
            .map_err(|e| KernelError::Metal(format!("wgpu map: {e:?}")))?;

        let view = slice
            .get_mapped_range()
            .map_err(|e| KernelError::Metal(format!("wgpu get_mapped_range: {e:?}")))?;
        for i in 0..out.len() {
            let w = u32::from_le_bytes([
                view[i * 4],
                view[i * 4 + 1],
                view[i * 4 + 2],
                view[i * 4 + 3],
            ]);
            out[i] = w as u16;
        }
        drop(view);
        staging.unmap();
        Ok(())
    }
}
