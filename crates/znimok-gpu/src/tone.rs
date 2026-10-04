//! HDR → SDR tone mapping on the GPU (ZK-38): the captured frame is uploaded in its own format
//! (R16G16B16A16_FLOAT or R10G10B10A2 — no conversion on the CPU), a WGSL compute pass
//! (`tone.wgsl`) writes RGBA8 and the result is read back. Ported from prototype P2, where the
//! maths was measured against LH; here the reference is `znimok_platform::frame::tone`.
//!
//! One device for the whole process, made on first use ([`ToneMapper::shared`]); a machine where
//! it cannot be made keeps the CPU path ([`to_srgb8`]).

use std::sync::OnceLock;

use znimok_platform::{Frame, PixelFormat, Transfer};

use crate::GpuError;

/// Shader mode, shared with `tone.wgsl`.
fn mode(t: Transfer) -> u32 {
    match t {
        Transfer::ScRgb => 1,
        Transfer::Pq => 2,
        Transfer::Srgb => 3,
        Transfer::ExtendedLinear => 4,
    }
}

pub struct ToneMapper {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    max_side: u32,
    /// «NVIDIA GeForce … (Dx12)», for logs and test messages.
    pub adapter: String,
}

impl ToneMapper {
    /// A device of its own. DX12 on Windows and Metal on macOS, as the app's canvas (the Vulkan
    /// driver of Intel UHD 630 crashed inside `request_device`, ZK-14); `WGPU_BACKEND` overrides.
    /// Any adapter will do, the software one too (CI runners).
    pub fn new() -> Result<Self, GpuError> {
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
        .map_err(|e| GpuError::NoDevice(e.to_string()))?;
        let info = adapter.get_info();
        // A composed frame of several displays can be wide: ask for what the adapter has.
        let limits = wgpu::Limits {
            max_texture_dimension_2d: adapter.limits().max_texture_dimension_2d,
            ..wgpu::Limits::downlevel_defaults()
        };
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("znimok-tone"),
            required_limits: limits.clone(),
            ..Default::default()
        }))
        .map_err(|e| GpuError::NoDevice(e.to_string()))?;
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
            max_side: limits.max_texture_dimension_2d,
            adapter: format!("{} ({:?})", info.name, info.backend),
        })
    }

    /// The process-wide mapper, made on first use (≈0.1–0.3 s); `None` when this machine has
    /// no usable device — asked once, not on every capture.
    pub fn shared() -> Option<&'static ToneMapper> {
        static SHARED: OnceLock<Option<ToneMapper>> = OnceLock::new();
        SHARED.get_or_init(|| ToneMapper::new().ok()).as_ref()
    }

    /// Frame → tightly packed RGBA8, alpha 255. SDR 8-bit frames need no GPU and are only
    /// reordered on the CPU.
    pub fn map(&self, frame: &Frame) -> Result<Vec<u8>, GpuError> {
        let format = match frame.format {
            PixelFormat::Bgra8 | PixelFormat::Rgba8 => {
                return frame
                    .to_rgba8()
                    .ok_or_else(|| GpuError::Failed("8-bit frame".into()));
            }
            PixelFormat::Rgba16Float => wgpu::TextureFormat::Rgba16Float,
            PixelFormat::Rgb10A2 => wgpu::TextureFormat::Rgb10a2Unorm,
        };
        let (w, h) = (frame.width, frame.height);
        if w == 0 || h == 0 {
            return Ok(Vec::new());
        }
        if w > self.max_side || h > self.max_side {
            return Err(GpuError::TooLarge {
                width: w,
                height: h,
                max: self.max_side,
            });
        }
        let white = if frame.color.sdr_white_nits > 0.0 {
            frame.color.sdr_white_nits
        } else {
            80.0
        };
        let size = wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
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
        // The frame's own row pitch: write_texture takes any, no repacking on the CPU.
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &src,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &frame.data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(frame.stride),
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
        let params = [mode(frame.color.transfer), white.to_bits(), 0, 0];
        let ubuf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("params"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bytes: Vec<u8> = params.iter().flat_map(|v| v.to_le_bytes()).collect();
        self.queue.write_buffer(&ubuf, 0, &bytes);
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
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| GpuError::Failed(e.to_string()))?;
        rx.recv()
            .map_err(|e| GpuError::Failed(e.to_string()))?
            .map_err(|e| GpuError::Failed(e.to_string()))?;
        let data = slice
            .get_mapped_range()
            .map_err(|e| GpuError::Failed(e.to_string()))?;
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h as usize {
            let s = y * row as usize;
            out.extend_from_slice(&data[s..s + (w * 4) as usize]);
        }
        Ok(out)
    }
}

/// 8-bit sRGB RGBA of any captured frame: HDR on the GPU when there is one, otherwise (or if the
/// GPU fails) the CPU reference — the same result within ±1.
pub fn to_srgb8(frame: &Frame) -> Vec<u8> {
    if matches!(
        frame.format,
        PixelFormat::Rgba16Float | PixelFormat::Rgb10A2
    ) && let Some(t) = ToneMapper::shared()
        && let Ok(v) = t.map(frame)
    {
        return v;
    }
    frame.to_srgb8()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A test that cannot run on this machine says so and passes — except on CI, where
    /// `ZNIMOK_REQUIRE_GPU` is set: a runner that lost its GPU or encoder must not stay green (ZK-285).
    fn skipped(why: impl std::fmt::Display) {
        if std::env::var_os("ZNIMOK_REQUIRE_GPU").is_some() {
            panic!("ZNIMOK_REQUIRE_GPU is set, but: {why}");
        }
        eprintln!("skipped: {why}");
    }
    use znimok_platform::{ColorInfo, Rect};

    fn half(v: f32) -> [u8; 2] {
        // f32 → binary16, round to nearest (enough for test values).
        let b = v.to_bits();
        let sign = ((b >> 16) & 0x8000) as u16;
        let exp = ((b >> 23) & 0xFF) as i32 - 127 + 15;
        let man = b & 0x7F_FFFF;
        let h = if v.is_nan() {
            0x7E00
        } else if exp <= 0 {
            sign
        } else if exp >= 31 {
            sign | 0x7C00
        } else {
            sign | ((exp as u16) << 10) | ((man + 0x1000) >> 13) as u16
        };
        h.to_le_bytes()
    }

    /// A frame covering the format's whole range: values rise along x, the channels are
    /// decorrelated along y; odd width and padded rows test the pitch.
    fn frame(format: PixelFormat, transfer: Transfer, white: f32, top: f32) -> Frame {
        let (w, h) = (257u32, 33u32);
        let bpp = format.bytes_per_pixel();
        let stride = w * bpp + 12;
        let mut data = vec![0xAB; (stride * h) as usize];
        for y in 0..h {
            for x in 0..w {
                let t = x as f32 / (w - 1) as f32;
                let u = y as f32 / (h - 1) as f32;
                let o = (y * stride + x * bpp) as usize;
                match format {
                    PixelFormat::Rgba16Float => {
                        let v = t * top;
                        for (i, c) in [v, v * u, v * 0.3, 1.0].into_iter().enumerate() {
                            data[o + 2 * i..o + 2 * i + 2].copy_from_slice(&half(c));
                        }
                    }
                    PixelFormat::Rgb10A2 => {
                        let e = |q: f32| ((q * 1023.0).round() as u32).min(1023);
                        let word = e(t) | (e(u) << 10) | (e(1.0 - t) << 20) | (3 << 30);
                        data[o..o + 4].copy_from_slice(&word.to_le_bytes());
                    }
                    _ => unreachable!(),
                }
            }
        }
        Frame {
            width: w,
            height: h,
            stride,
            format,
            color: ColorInfo {
                transfer,
                sdr_white_nits: white,
                hdr: true,
            },
            source: Rect::new(0, 0, w, h),
            scale: 1.0,
            data,
        }
    }

    fn cases() -> Vec<Frame> {
        vec![
            // Windows HDR: scRGB up to 1000 nits, SDR white 80 and 240 nits.
            frame(PixelFormat::Rgba16Float, Transfer::ScRgb, 80.0, 12.5),
            frame(PixelFormat::Rgba16Float, Transfer::ScRgb, 240.0, 12.5),
            // HDR10: PQ BT.2020, SDR white 203 nits.
            frame(PixelFormat::Rgb10A2, Transfer::Pq, 203.0, 1.0),
            // 10-bit SDR.
            frame(PixelFormat::Rgb10A2, Transfer::Srgb, 80.0, 1.0),
            // macOS EDR: 1.0 = SDR white, headroom ×4.
            frame(
                PixelFormat::Rgba16Float,
                Transfer::ExtendedLinear,
                100.0,
                4.0,
            ),
        ]
    }

    #[test]
    fn gpu_matches_the_cpu_reference() {
        let t = match ToneMapper::new() {
            Ok(t) => t,
            Err(e) => {
                skipped(e);
                return;
            }
        };
        for f in cases() {
            let gpu = t.map(&f).unwrap();
            let cpu = f.to_srgb8();
            assert_eq!(gpu.len(), cpu.len());
            let worst = gpu
                .iter()
                .zip(&cpu)
                .map(|(a, b)| a.abs_diff(*b))
                .max()
                .unwrap();
            assert!(
                worst <= 1,
                "{:?}/{:?} white {}: off by {worst} on {}",
                f.format,
                f.color.transfer,
                f.color.sdr_white_nits,
                t.adapter
            );
            // Not trivially equal: the ramp reaches black and clipped white.
            assert!(cpu.chunks(4).any(|p| p[0] == 0) && cpu.chunks(4).any(|p| p[0] == 255));
        }
    }

    #[test]
    fn sdr_white_is_white_and_above_clips() {
        let Some(t) = ToneMapper::shared() else {
            skipped("no GPU device");
            return;
        };
        let mut f = frame(PixelFormat::Rgba16Float, Transfer::ScRgb, 240.0, 12.5);
        // Pixel 0 of row 0: exactly SDR white (3.0 at 240 nits), pixel 1: twice as bright.
        for (x, v) in [(0usize, 3.0f32), (1, 6.0)] {
            for c in 0..3 {
                f.data[x * 8 + 2 * c..x * 8 + 2 * c + 2].copy_from_slice(&half(v));
            }
        }
        let out = t.map(&f).unwrap();
        assert_eq!(&out[0..8], &[255, 255, 255, 255, 255, 255, 255, 255]);
    }

    #[test]
    fn nan_is_black_as_on_the_cpu() {
        let Some(t) = ToneMapper::shared() else {
            return;
        };
        let mut f = frame(PixelFormat::Rgba16Float, Transfer::ScRgb, 80.0, 12.5);
        for c in 0..3 {
            f.data[2 * c..2 * c + 2].copy_from_slice(&half(f32::NAN));
        }
        assert_eq!(&t.map(&f).unwrap()[0..3], &f.to_srgb8()[0..3]);
    }

    #[test]
    fn sdr_frames_skip_the_gpu() {
        let f = Frame {
            width: 1,
            height: 1,
            stride: 4,
            format: PixelFormat::Bgra8,
            color: ColorInfo::SDR,
            source: Rect::new(0, 0, 1, 1),
            scale: 1.0,
            data: vec![10, 20, 30, 255],
        };
        assert_eq!(to_srgb8(&f), vec![30, 20, 10, 255]);
    }
}
