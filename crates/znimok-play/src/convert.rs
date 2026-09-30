//! The wgpu side: NV12 planes → `nv12.wgsl` → an RGBA8 texture the UI can show as it is
//! (Slint takes `Rgba8Unorm` with TEXTURE_BINDING | RENDER_ATTACHMENT), or a small thumbnail
//! read back to the CPU.

/// A device and its queue — the application's own, so the frames never leave the GPU.
#[derive(Clone)]
pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

/// Two plane textures, for frames that come through CPU memory (the fallback path).
pub struct Planes {
    pub y: wgpu::Texture,
    pub uv: wgpu::Texture,
}

pub struct Converter {
    pub gpu: Gpu,
    main: wgpu::ComputePipeline,
    thumb: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    params: wgpu::Buffer,
    lut: wgpu::Buffer,
    tone: bool,
}

impl Converter {
    /// The conversion pipelines on `gpu`; an error when the device cannot run them (a device
    /// made without storage buffers or storage textures).
    pub fn new(gpu: &Gpu) -> Result<Self, String> {
        let device = &gpu.device;
        let l = device.limits();
        if l.max_storage_textures_per_shader_stage < 1 || l.max_storage_buffers_per_shader_stage < 1
        {
            return Err("the GPU device allows no storage textures / buffers in a compute pass".into());
        }
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("znimok nv12"),
            source: wgpu::ShaderSource::Wgsl(include_str!("nv12.wgsl").into()),
        });
        let tex = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("znimok nv12"),
            entries: &[
                tex(0),
                tex(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        view_dimension: wgpu::TextureViewDimension::D2,
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
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("znimok nv12"),
            bind_group_layouts: &[Some(&layout)],
            ..Default::default()
        });
        let pipe = |entry| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("znimok nv12"),
                layout: Some(&pl),
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("znimok nv12 params"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let lut = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("znimok nv12 tone"),
            size: 256 * 4,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Ok(Self {
            gpu: gpu.clone(),
            main: pipe("main"),
            thumb: pipe("thumb"),
            layout,
            params,
            lut,
            tone: false,
        })
    }

    /// The picture's tone table (`znimok_render::develop::tone_lut`), or None for none.
    pub fn set_tone(&mut self, lut: Option<[u8; 256]>) {
        self.tone = lut.is_some();
        if let Some(l) = lut {
            let words: Vec<u8> = l.iter().flat_map(|v| u32::from(*v).to_le_bytes()).collect();
            self.gpu.queue.write_buffer(&self.lut, 0, &words);
        }
    }

    /// An RGBA8 frame the UI can show and the conversion can write.
    pub fn target(&self, width: u32, height: u32) -> wgpu::Texture {
        self.gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("znimok video frame"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        })
    }

    /// Queues the conversion of a picture `width` × `height` from its planes into `out` (a
    /// smaller `out` with `thumb` gets the averaged thumbnail).
    pub fn run(
        &self,
        y: &wgpu::TextureView,
        uv: &wgpu::TextureView,
        width: u32,
        height: u32,
        out: &wgpu::Texture,
        thumb: bool,
    ) -> wgpu::SubmissionIndex {
        let mut p = [0u8; 16];
        p[0..4].copy_from_slice(&width.to_le_bytes());
        p[4..8].copy_from_slice(&height.to_le_bytes());
        p[8..12].copy_from_slice(&u32::from(self.tone).to_le_bytes());
        self.gpu.queue.write_buffer(&self.params, 0, &p);
        let view = out.create_view(&Default::default());
        let bind = self
            .gpu
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("znimok nv12"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(y),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(uv),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: self.params.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: self.lut.as_entire_binding(),
                    },
                ],
            });
        let (w, h) = (out.width(), out.height());
        let mut enc = self
            .gpu
            .device
            .create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_compute_pass(&Default::default());
            pass.set_pipeline(if thumb { &self.thumb } else { &self.main });
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(w.div_ceil(8), h.div_ceil(8), 1);
        }
        self.gpu.queue.submit([enc.finish()])
    }

    pub fn planes(&self, width: u32, height: u32) -> Planes {
        let mk = |label, w: u32, h: u32, format| {
            self.gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: w.max(1),
                    height: h.max(1),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        };
        Planes {
            y: mk("znimok luma", width, height, wgpu::TextureFormat::R8Unorm),
            uv: mk(
                "znimok chroma",
                width.div_ceil(2),
                height.div_ceil(2),
                wgpu::TextureFormat::Rg8Unorm,
            ),
        }
    }

    /// A frame in CPU memory: luma rows at `y` (`y_pitch` bytes apart), interleaved chroma rows
    /// at `uv`.
    pub fn upload(&self, p: &Planes, y: &[u8], y_pitch: u32, uv: &[u8], uv_pitch: u32) {
        let put = |t: &wgpu::Texture, data: &[u8], pitch: u32| {
            self.gpu.queue.write_texture(
                t.as_image_copy(),
                data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(pitch),
                    rows_per_image: None,
                },
                t.size(),
            );
        };
        put(&p.y, y, y_pitch);
        put(&p.uv, uv, uv_pitch);
    }

    /// The whole of an RGBA8 texture back in CPU memory, rows packed (waits for the GPU).
    pub fn read(&self, tex: &wgpu::Texture) -> Result<Vec<u8>, String> {
        let (w, h) = (tex.width(), tex.height());
        let pitch = (w * 4).div_ceil(256) * 256;
        let buf = self.gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("znimok readback"),
            size: u64::from(pitch) * u64::from(h),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = self
            .gpu
            .device
            .create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            tex.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(pitch),
                    rows_per_image: None,
                },
            },
            tex.size(),
        );
        self.gpu.queue.submit([enc.finish()]);
        let slice = buf.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        let _ = self
            .gpu
            .device
            .poll(wgpu::PollType::wait_indefinitely());
        rx.recv()
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        let data = slice.get_mapped_range().map_err(|e| format!("{e:?}"))?;
        let row = (w * 4) as usize;
        let mut out = Vec::with_capacity(row * h as usize);
        for r in 0..h as usize {
            let o = r * pitch as usize;
            out.extend_from_slice(&data[o..o + row]);
        }
        drop(data);
        buf.unmap();
        Ok(out)
    }

    /// Waits until the GPU finished `done`.
    pub fn wait(&self, done: &wgpu::SubmissionIndex) {
        let _ = self.gpu.device.poll(wgpu::PollType::Wait {
            submission_index: Some(done.clone()),
            timeout: None,
        });
    }
}
