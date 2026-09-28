//! wgpu side of playback: NV12 planes (either views of an imported NV12 texture or two uploaded
//! textures) → `nv12.wgsl` compute pass → RGBA8 frame. In the editor the RGBA frame is what the
//! canvas composes; here it stays offscreen and only the barcode band is ever read back.

pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    pub adapter: wgpu::AdapterInfo,
    /// The adapter can wrap a native NV12 texture (DX12 does, the zero-copy path needs it).
    pub nv12: bool,
}

/// Two uploaded planes, for the paths where the frame comes through CPU memory.
pub struct Planes {
    pub y: wgpu::Texture,
    pub uv: wgpu::Texture,
}

impl Gpu {
    pub fn new() -> Result<Self, String> {
        // DX12 on Windows, Metal on macOS (as in P1/P2): the Vulkan driver of Intel UHD 630 crashes
        // inside request_device on PLUM-MEDIA, and the shared-handle import is DX12-only anyway.
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = wgpu::Backends::from_env().unwrap_or(if cfg!(target_os = "macos") {
            wgpu::Backends::METAL
        } else if cfg!(windows) {
            wgpu::Backends::DX12
        } else {
            wgpu::Backends::all()
        });
        let instance = wgpu::Instance::new(desc);
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            ..Default::default()
        }))
        .or_else(|_| {
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::None,
                force_fallback_adapter: true,
                ..Default::default()
            }))
        })
        .map_err(|e| format!("немає адаптера wgpu: {e}"))?;
        let nv12 = adapter
            .features()
            .contains(wgpu::Features::TEXTURE_FORMAT_NV12);
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("znimok-p4"),
            required_features: if nv12 {
                wgpu::Features::TEXTURE_FORMAT_NV12
            } else {
                wgpu::Features::empty()
            },
            ..Default::default()
        }))
        .map_err(|e| format!("request_device: {e}"))?;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("nv12"),
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
            label: Some("nv12"),
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
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("nv12"),
            bind_group_layouts: &[Some(&layout)],
            ..Default::default()
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("nv12"),
            layout: Some(&pl),
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        Ok(Self {
            device,
            queue,
            pipeline,
            layout,
            adapter: adapter.get_info(),
            nv12,
        })
    }

    /// The RGBA8 frame the conversion writes into.
    pub fn target(&self, width: u32, height: u32) -> wgpu::Texture {
        self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("frame"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        })
    }

    pub fn bind(
        &self,
        y: &wgpu::TextureView,
        uv: &wgpu::TextureView,
        out: &wgpu::Texture,
    ) -> wgpu::BindGroup {
        let out = out.create_view(&Default::default());
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("nv12"),
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
                    resource: wgpu::BindingResource::TextureView(&out),
                },
            ],
        })
    }

    /// Record the conversion of one frame into `out` (size taken from the target).
    pub fn convert(&self, bind: &wgpu::BindGroup, width: u32, height: u32) -> wgpu::CommandBuffer {
        let mut enc = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, bind, &[]);
            pass.dispatch_workgroups(width.div_ceil(8), height.div_ceil(8), 1);
        }
        enc.finish()
    }

    pub fn planes(&self, width: u32, height: u32) -> Planes {
        let mk = |label, w, h, format| {
            self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
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
            y: mk("y", width, height, wgpu::TextureFormat::R8Unorm),
            uv: mk("uv", width / 2, height / 2, wgpu::TextureFormat::Rg8Unorm),
        }
    }

    /// Upload a frame that sits in CPU memory: luma rows at `y`, interleaved chroma rows at `uv`.
    pub fn upload(&self, p: &Planes, y: &[u8], y_pitch: u32, uv: &[u8], uv_pitch: u32) {
        let put = |t: &wgpu::Texture, data: &[u8], pitch: u32| {
            self.queue.write_texture(
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

    /// Read rows `0..rows` of an RGBA8 frame back (tightly packed).
    pub fn read_band(&self, frame: &wgpu::Texture, rows: u32) -> Vec<u8> {
        let w = frame.width();
        let pitch = (w * 4).div_ceil(256) * 256;
        let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("band"),
            size: u64::from(pitch * rows),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = self.device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            frame.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(pitch),
                    rows_per_image: None,
                },
            },
            wgpu::Extent3d {
                width: w,
                height: rows,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([enc.finish()]);
        let slice = buf.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        self.wait();
        let data = slice.get_mapped_range().expect("mapped");
        let mut out = Vec::with_capacity((w * 4 * rows) as usize);
        for r in 0..rows {
            let o = (r * pitch) as usize;
            out.extend_from_slice(&data[o..o + (w * 4) as usize]);
        }
        out
    }

    pub fn wait(&self) {
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pattern::{self, Nv12};

    /// The shader against the CPU formula on the synthetic frame, through the upload path —
    /// runs on CI (WARP on Windows, Metal on macOS).
    #[test]
    fn shader_matches_cpu() {
        let g = match Gpu::new() {
            Ok(g) => g,
            Err(e) => {
                eprintln!("пропущено: {e}");
                return;
            }
        };
        let (w, h) = (640, 360);
        let mut f = Nv12::new(w, h);
        f.draw(1234);
        let planes = g.planes(w, h);
        let (y, uv) = f.data.split_at((w * h) as usize);
        g.upload(&planes, y, w, uv, w);
        let out = g.target(w, h);
        let bind = g.bind(
            &planes.y.create_view(&Default::default()),
            &planes.uv.create_view(&Default::default()),
            &out,
        );
        g.queue.submit([g.convert(&bind, w, h)]);
        let gpu = g.read_band(&out, h);
        let cpu = pattern::nv12_to_rgba(&f);
        let worst = gpu
            .iter()
            .zip(&cpu)
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap_or(0);
        assert!(worst <= 1, "GPU і CPU розходяться на {worst}");
        assert_eq!(pattern::read(&gpu, w).index, Some(1234));
    }
}
