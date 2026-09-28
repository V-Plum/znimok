//! Tone mapping on the GPU: the captured frame goes into a texture of its own format, a WGSL
//! compute pass (`tone.wgsl`) writes RGBA8, and the result is read back. In the product the frame
//! will come in without the CPU copy (shared handle, P4); here the point is the maths and the
//! formats, measured against `tone.rs`.

use crate::tone::{Frame, Pixels, mode};

pub struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    pub adapter_name: String,
}

impl Gpu {
    /// Any adapter will do, including the software one (CI runners have no GPU).
    pub fn new() -> Result<Self, String> {
        // DX12 on Windows, Metal on macOS (as in P1): the Vulkan driver of Intel UHD 630 crashes inside
        // request_device on PLUM-MEDIA. WGPU_BACKEND still overrides for experiments.
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
        let info = adapter.get_info();
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("znimok-p2"),
            ..Default::default()
        }))
        .map_err(|e| format!("request_device: {e}"))?;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("tone"),
            source: wgpu::ShaderSource::Wgsl(include_str!("tone.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("tone"),
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
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
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
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("tone"),
            bind_group_layouts: &[Some(&layout)],
            ..Default::default()
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("tone"),
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
            adapter_name: format!("{} ({:?})", info.name, info.backend),
        })
    }

    /// Frame → RGBA8, tightly packed.
    pub fn map(&self, frame: &Frame, white_nits: f32) -> Vec<u8> {
        let (w, h) = (frame.width, frame.height);
        let size = wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        };
        let (format, bpp, bytes): (_, u32, &[u8]) = match &frame.pixels {
            Pixels::F16(v) => (wgpu::TextureFormat::Rgba16Float, 8, as_bytes_u16(v)),
            Pixels::Rgb10a2(v) => (wgpu::TextureFormat::Rgb10a2Unorm, 4, as_bytes_u32(v)),
            Pixels::Bgra8(v) => (wgpu::TextureFormat::Bgra8Unorm, 4, v.as_slice()),
        };
        let src = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("frame"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &src,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w * bpp),
                rows_per_image: Some(h),
            },
            size,
        );
        let dst = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("sdr"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let params: [u32; 4] = [mode(frame.transfer), white_nits.to_bits(), 0, 0];
        let ubuf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("params"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.queue.write_buffer(&ubuf, 0, as_bytes_u32(&params));
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("tone"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(
                        &src.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(
                        &dst.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: ubuf.as_entire_binding(),
                },
            ],
        });
        let row = (w * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let rb = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: u64::from(row) * u64::from(h),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut enc = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(w.div_ceil(8), h.div_ceil(8), 1);
        }
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &dst,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &rb,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(h),
                },
            },
            size,
        );
        self.queue.submit([enc.finish()]);
        let slice = rb.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        let data = slice.get_mapped_range().expect("readback map");
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h as usize {
            let s = y * row as usize;
            out.extend_from_slice(&data[s..s + (w * 4) as usize]);
        }
        out
    }
}

fn as_bytes_u16(v: &[u16]) -> &[u8] {
    // SAFETY: u16 has no padding and any byte pattern is valid u8; alignment of u8 is 1.
    unsafe { std::slice::from_raw_parts(v.as_ptr().cast::<u8>(), std::mem::size_of_val(v)) }
}

fn as_bytes_u32(v: &[u32]) -> &[u8] {
    // SAFETY: as above.
    unsafe { std::slice::from_raw_parts(v.as_ptr().cast::<u8>(), std::mem::size_of_val(v)) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tone::{map_cpu, synthetic_frames};

    #[test]
    fn gpu_matches_cpu_reference() {
        let gpu = match Gpu::new() {
            Ok(g) => g,
            Err(e) => {
                eprintln!("пропущено: {e}");
                return;
            }
        };
        for (frame, white) in synthetic_frames() {
            let a = gpu.map(&frame, white);
            let b = map_cpu(&frame, white);
            let worst = a
                .iter()
                .zip(&b)
                .map(|(x, y)| x.abs_diff(*y))
                .max()
                .unwrap_or(0);
            assert!(
                worst <= 1,
                "{:?} white {white}: розбіжність {worst} на {}",
                frame.transfer,
                gpu.adapter_name
            );
        }
    }
}
