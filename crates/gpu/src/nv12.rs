//! GPU RGBA8 → NV12 (BT.709 limited range) for hardware-encoded exports.
//!
//! A compute pass reads the resolved `Rgba8Unorm` output texture and writes the NV12 picture into
//! a storage buffer that is copied to a mappable staging buffer: only `w * h * 3 / 2` bytes cross
//! back to the CPU instead of `w * h * 4`, and the CPU conversion disappears. The arithmetic is
//! the CPU reference's (see `nv12.wgsl`).

pub(crate) struct Nv12Stage {
    y_pipeline: wgpu::ComputePipeline,
    uv_pipeline: wgpu::ComputePipeline,
    bgl: wgpu::BindGroupLayout,
    uniform: wgpu::Buffer,
    /// Storage buffer the shader writes, and the mappable copy of it (both `size` bytes).
    buffers: Option<(wgpu::Buffer, wgpu::Buffer, u64)>,
}

impl Nv12Stage {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("filmcraft-nv12"),
            source: wgpu::ShaderSource::Wgsl(include_str!("nv12.wgsl").into()),
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("nv12-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: false }, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("nv12-layout"), bind_group_layouts: &[Some(&bgl)], immediate_size: 0 });
        let pipe = |entry: &'static str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(&layout),
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("nv12-size"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self { y_pipeline: pipe("y_main"), uv_pipeline: pipe("uv_main"), bgl, uniform, buffers: None }
    }

    /// Whether a `w` x `h` picture can be converted here (whole 4-pixel words, even height).
    pub(crate) fn fits(w: u32, h: u32) -> bool {
        w >= 4 && h >= 2 && w.is_multiple_of(4) && h.is_multiple_of(2)
    }

    /// Record the conversion of `src` (the `Rgba8Unorm` output, `w` x `h`) and its copy to the
    /// staging buffer; returns the NV12 size in bytes.
    pub(crate) fn record(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        enc: &mut wgpu::CommandEncoder,
        src: &wgpu::TextureView,
        w: u32,
        h: u32,
    ) -> Option<u64> {
        if !Self::fits(w, h) {
            return None;
        }
        let size = u64::from(w) * u64::from(h) * 3 / 2;
        if self.buffers.as_ref().is_none_or(|b| b.2 < size) {
            let storage = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("nv12-storage"),
                size,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });
            let staging = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("nv12-staging"),
                size,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            self.buffers = Some((storage, staging, size));
        }
        let (storage, staging, _) = self.buffers.as_ref()?;
        let mut dims = [0u8; 16];
        dims[..4].copy_from_slice(&w.to_le_bytes());
        dims[4..8].copy_from_slice(&h.to_le_bytes());
        queue.write_buffer(&self.uniform, 0, &dims);
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("nv12-bg"),
            layout: &self.bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(src) },
                wgpu::BindGroupEntry { binding: 1, resource: storage.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: self.uniform.as_entire_binding() },
            ],
        });
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("nv12"), timestamp_writes: None });
            pass.set_bind_group(0, &bg, &[]);
            let gx = (w / 4).div_ceil(16);
            pass.set_pipeline(&self.y_pipeline);
            pass.dispatch_workgroups(gx, h.div_ceil(16), 1);
            pass.set_pipeline(&self.uv_pipeline);
            pass.dispatch_workgroups(gx, (h / 2).div_ceil(16), 1);
        }
        enc.copy_buffer_to_buffer(storage, 0, staging, 0, size);
        Some(size)
    }

    /// Wait for the staging buffer and copy the NV12 bytes out.
    pub(crate) fn map(&self, device: &wgpu::Device, size: u64, sub: Option<wgpu::SubmissionIndex>) -> Option<Vec<u8>> {
        let (_, staging, _) = self.buffers.as_ref()?;
        let slice = staging.slice(..size);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        super::wait_mapped(device, sub)?;
        rx.recv().ok()?.ok()?;
        let data = slice.get_mapped_range().ok()?;
        let out = data.to_vec();
        drop(data);
        staging.unmap();
        Some(out)
    }
}
