//! The shader stage: one compute pass per output frame (`record.wgsl`) — crop / fit, tone,
//! click rings and the cursor overlay. What a frame needs from the source is a [`FrameGeometry`];
//! what the moment adds on top is an [`Overlay`] (ZK-90 fills it, here it is drawn).

use std::sync::Arc;

use crate::interop::Gpu;

/// How the source maps onto the video (LH `Src()`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameGeometry {
    /// 0 gamma-encoded as is, 1 scRGB linear, 2 PQ BT.2020 (see `record.wgsl`).
    pub mode: u32,
    /// SDR white of the display in nits.
    pub white: f32,
    /// The part of the source texture to show: x, y, width, height in source texels.
    pub crop: (i32, i32, u32, u32),
    /// Video size.
    pub out: (u32, u32),
}

impl FrameGeometry {
    /// Source texels per output pixel, the area of the video the source covers and its origin.
    /// Same size (±1 px of parity) → 1:1; smaller → 1:1 at the top-left, black around; larger →
    /// shrunk to fit, keeping the aspect (a smaller window is never stretched: text would blur).
    pub fn fit(&self) -> (f32, (u32, u32)) {
        let (cw, ch) = (self.crop.2, self.crop.3);
        let (w, h) = self.out;
        if cw.abs_diff(w) <= 1 && ch.abs_diff(h) <= 1 {
            return (1.0, (cw.min(w), ch.min(h)));
        }
        if cw <= w && ch <= h {
            return (1.0, (cw, ch));
        }
        let scale = (cw as f32 / w as f32).max(ch as f32 / h as f32);
        (
            scale,
            (
                ((cw as f32 / scale).round() as u32).clamp(1, w),
                ((ch as f32 / scale).round() as u32).clamp(1, h),
            ),
        )
    }
}

/// A click ring in output pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ring {
    pub x: f32,
    pub y: f32,
    pub radius: f32,
    pub alpha: f32,
    /// 0 click, 1 right click, 2 hold.
    pub kind: u32,
}

/// The cursor as LH draws it: per pixel a multiplier in `a` and an addition in `rgb`
/// (RGBA16F, `w × h × 8` bytes), so an inverting I-beam is `a = −1, rgb = 1`.
#[derive(Clone, Debug)]
pub struct CursorImage {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub data: Arc<Vec<u8>>,
    /// Changes when the picture changes (a new `HCURSOR`): the texture is uploaded again only then.
    pub generation: u64,
}

#[derive(Clone, Debug, Default)]
pub struct Overlay {
    pub rings: Vec<Ring>,
    pub cursor: Option<CursorImage>,
    /// Device scale of the recorded content (line widths: 2.5·s, the dot 5·s — LH).
    pub scale: f32,
    /// Ring colour, 0..1.
    pub ring_color: [f32; 3],
}

impl Overlay {
    pub const MAX_RINGS: usize = 8;
}

/// What the stage writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutFormat {
    Bgra8,
    Rgba8,
}

impl OutFormat {
    pub fn wgpu(self) -> wgpu::TextureFormat {
        match self {
            Self::Bgra8 => wgpu::TextureFormat::Bgra8Unorm,
            Self::Rgba8 => wgpu::TextureFormat::Rgba8Unorm,
        }
    }
}

const UNIFORM_BYTES: u64 = 336;

pub struct Stage {
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniform: wgpu::Buffer,
    overlay: wgpu::Texture,
    overlay_view: wgpu::TextureView,
    overlay_gen: Option<u64>,
    pub format: OutFormat,
}

fn overlay_texture(gpu: &Gpu, w: u32, h: u32) -> wgpu::Texture {
    gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("cursor overlay"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

impl Stage {
    pub fn new(gpu: &Gpu, format: OutFormat) -> Result<Self, String> {
        if format == OutFormat::Bgra8 && !gpu.bgra_storage {
            return Err("адаптер не пише BGRA8 з шейдера".into());
        }
        let src = include_str!("record.wgsl").replace(
            "@FORMAT@",
            match format {
                OutFormat::Bgra8 => "bgra8unorm",
                OutFormat::Rgba8 => "rgba8unorm",
            },
        );
        let module = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("record"),
                source: wgpu::ShaderSource::Wgsl(src.into()),
            });
        let tex = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("record"),
                entries: &[
                    tex(0),
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    tex(2),
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::StorageTexture {
                            access: wgpu::StorageTextureAccess::WriteOnly,
                            format: format.wgpu(),
                            view_dimension: wgpu::TextureViewDimension::D2,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 4,
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
        let pl = gpu
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("record"),
                bind_group_layouts: &[Some(&layout)],
                ..Default::default()
            });
        let pipeline = gpu
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("record"),
                layout: Some(&pl),
                module: &module,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                cache: None,
            });
        let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("fit"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniform = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("record params"),
            size: UNIFORM_BYTES,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // No cursor: one pixel that changes nothing (a = 1, rgb = 0).
        let overlay = overlay_texture(gpu, 1, 1);
        gpu.queue.write_texture(
            overlay.as_image_copy(),
            &[0, 0, 0, 0, 0, 0, 0x00, 0x3C],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(8),
                rows_per_image: None,
            },
            overlay.size(),
        );
        let overlay_view = overlay.create_view(&Default::default());
        Ok(Self {
            pipeline,
            layout,
            sampler,
            uniform,
            overlay,
            overlay_view,
            overlay_gen: None,
            format,
        })
    }

    fn params(geo: &FrameGeometry, ov: &Overlay, cursor: (i32, i32, u32, u32)) -> Vec<u8> {
        let mut b = Vec::with_capacity(UNIFORM_BYTES as usize);
        let (scale, fit) = geo.fit();
        let n = ov.rings.len().min(Overlay::MAX_RINGS);
        for v in [geo.mode, geo.white.to_bits(), scale.to_bits(), n as u32] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        for v in [
            (geo.crop.0 as f32).to_bits(),
            (geo.crop.1 as f32).to_bits(),
            fit.0,
            fit.1,
        ] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        for v in [cursor.0, cursor.1, cursor.2 as i32, cursor.3 as i32] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        for v in [ov.ring_color[0], ov.ring_color[1], ov.ring_color[2], 1.0] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        let s = if ov.scale > 0.0 { ov.scale } else { 1.0 };
        for v in [2.5 * s, 5.0 * s, 0.0, 0.0] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        for i in 0..Overlay::MAX_RINGS {
            let r = ov.rings.get(i).copied().unwrap_or(Ring {
                x: 0.0,
                y: 0.0,
                radius: 0.0,
                alpha: 0.0,
                kind: 0,
            });
            for v in [r.x, r.y, r.radius, r.alpha] {
                b.extend_from_slice(&v.to_le_bytes());
            }
            for v in [r.kind, 0, 0, 0] {
                b.extend_from_slice(&v.to_le_bytes());
            }
        }
        debug_assert_eq!(b.len() as u64, UNIFORM_BYTES);
        b
    }

    /// Queue one frame: `src` through `geo` and `ov` into `dst` (a storage view of the stage's
    /// format, sized `geo.out`). Fence waits and signals for this submit are staged by the caller
    /// before the call ([`crate::interop::Bridge`]).
    pub fn render(
        &mut self,
        gpu: &Gpu,
        src: &wgpu::TextureView,
        dst: &wgpu::TextureView,
        geo: &FrameGeometry,
        ov: &Overlay,
    ) {
        let mut cursor = (0, 0, 0, 0);
        if let Some(c) = &ov.cursor
            && c.width > 0
            && c.height > 0
            && c.data.len() >= (c.width * c.height * 8) as usize
        {
            if self.overlay_gen != Some(c.generation)
                || self.overlay.width() != c.width
                || self.overlay.height() != c.height
            {
                if self.overlay.width() != c.width || self.overlay.height() != c.height {
                    self.overlay = overlay_texture(gpu, c.width, c.height);
                    self.overlay_view = self.overlay.create_view(&Default::default());
                }
                gpu.queue.write_texture(
                    self.overlay.as_image_copy(),
                    &c.data,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(c.width * 8),
                        rows_per_image: None,
                    },
                    self.overlay.size(),
                );
                self.overlay_gen = Some(c.generation);
            }
            cursor = (c.x, c.y, c.width, c.height);
        }
        gpu.queue
            .write_buffer(&self.uniform, 0, &Self::params(geo, ov, cursor));
        let bind = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("record"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(src),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&self.overlay_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(dst),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: self.uniform.as_entire_binding(),
                },
            ],
        });
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(geo.out.0.div_ceil(8), geo.out.1.div_ceil(8), 1);
        }
        gpu.queue.submit([enc.finish()]);
    }
}

/// A plain output texture for the software path and the tests.
pub fn local_output(gpu: &Gpu, format: OutFormat, w: u32, h: u32) -> wgpu::Texture {
    gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("record output"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: format.wgpu(),
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

/// Read a whole 4-byte-per-pixel texture back, tightly packed (waits for the GPU).
pub fn read_back(gpu: &Gpu, tex: &wgpu::Texture) -> Vec<u8> {
    let (w, h) = (tex.width(), tex.height());
    let pitch =
        (w * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: u64::from(pitch) * u64::from(h),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = gpu.device.create_command_encoder(&Default::default());
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
    gpu.queue.submit([enc.finish()]);
    let slice = buf.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    gpu.wait();
    let data = slice.get_mapped_range().expect("readback map");
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h as usize {
        let o = y * pitch as usize;
        out.extend_from_slice(&data[o..o + (w * 4) as usize]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_rules() {
        let g = |cw, ch| FrameGeometry {
            mode: 0,
            white: 80.0,
            crop: (0, 0, cw, ch),
            out: (1280, 720),
        };
        assert_eq!(g(1280, 720).fit(), (1.0, (1280, 720)));
        // A window of odd size recorded at the even size: still 1:1.
        assert_eq!(g(1281, 721).fit(), (1.0, (1280, 720)));
        assert_eq!(g(800, 600).fit(), (1.0, (800, 600)));
        let (s, f) = g(2560, 1440).fit();
        assert_eq!(s, 2.0);
        assert_eq!(f, (1280, 720));
        let (s, f) = g(1920, 1200).fit();
        assert!((s - 1.6667).abs() < 1e-3);
        assert_eq!(f, (1152, 720));
    }

    #[test]
    fn params_are_the_uniform_size() {
        let geo = FrameGeometry {
            mode: 1,
            white: 240.0,
            crop: (10, 20, 300, 200),
            out: (300, 200),
        };
        let ov = Overlay {
            rings: vec![Ring {
                x: 1.0,
                y: 2.0,
                radius: 3.0,
                alpha: 0.5,
                kind: 1,
            }],
            ..Default::default()
        };
        assert_eq!(
            Stage::params(&geo, &ov, (0, 0, 0, 0)).len() as u64,
            UNIFORM_BYTES
        );
    }
}
