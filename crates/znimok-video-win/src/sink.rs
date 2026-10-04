//! The Media Foundation sink: the shader stage, the NV12 conversion and `IMFSinkWriter` writing
//! `<name>.part`.
//!
//! **Hardware path** (LH `VidEncOpen` with `gpu = true`, one step further): the Sink Writer gets
//! the bridge's `IMFDXGIDeviceManager` and NV12 input, so the hardware H.264 MFT reads the
//! frames straight from an `IMFVideoSampleAllocatorEx` pool of NV12 textures. Per frame: the
//! shader writes BGRA8 into a shared texture of ours; once the wgpu fence says it is done, the
//! D3D11 Video Processor converts it into the allocated sample (BT.709, 16–235 — [`crate::nv12`])
//! and the sample goes to the writer. The encoder holds the samples asynchronously; when the
//! whole pool is held (`MF_E_SAMPLEALLOCATOR_EMPTY`) the sink answers [`SinkError::Busy`] and the
//! recorder retries — the only back-pressure of the loop (§2.2).
//!
//! **Software path** (no D3D manager, the Microsoft H.264 encoder): the same shader and
//! conversion, then the NV12 frame is copied to a staging texture and handed over as a memory
//! buffer with an explicit stride. On an adapter without BGRA8 storage the shader writes RGBA8,
//! which is read back and given to the writer as RGB32 (its own converter then).
//!
//! Output type as LH: H.264 High, progressive, BT.709, 16–235, bitrate from the settings, no
//! B-frames (§7 item 2), a key frame every `keyframe_interval` frames; `MF_LOW_LATENCY` so
//! frames leave the encoder in order and the first one sits at 0; audio tracks AAC-LC 48 kHz
//! stereo from PCM s16.

use std::path::Path;
use std::rc::Rc;

use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_NV12, DXGI_SAMPLE_DESC,
};
use windows::Win32::Media::MediaFoundation::*;
use windows::core::{GUID, HSTRING, Interface};
use znimok_video::audio::AudioSampleTime;
use znimok_video::cfr::VideoSampleTime;
use znimok_video::traits::{EncoderConfig, SinkCaps, SinkError, VideoSink};

use crate::interop::{Bridge, Gpu, SharedTexture};
use crate::mf::{B_COUNT, GOP_SIZE, err, set_size, transform_name};
use crate::nv12::Nv12Converter;
use crate::shader::{OutFormat, Stage, local_output, read_back};
use crate::source::{GpuFrame, SharedPool};

/// Samples the encoder may hold at once (LH: 4 at start, 16 at most).
const POOL_INITIAL: u32 = 4;
const POOL_MAX: u32 = 16;
/// Output textures the shader rotates through (each is converted out before reuse).
const OUT_SLOTS: usize = 2;

/// How frames are handed to the writer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Input {
    /// NV12 textures from the allocator (hardware).
    Nv12Textures,
    /// NV12 memory buffers, rows `width` bytes apart.
    Nv12Memory,
    /// RGB32 memory buffers, rows `width × 4` bytes apart (no BGRA8 storage on the adapter).
    Rgb32Memory,
}

/// The shader output and the conversion after it.
struct Converted {
    out: Vec<(SharedTexture, u64)>,
    next: usize,
    scratch: wgpu::Texture,
    conv: Nv12Converter,
}

enum Path_ {
    Hw {
        allocator: IMFVideoSampleAllocatorEx,
        c: Converted,
        /// The allocator's textures were checked to be NV12 (the conversion needs that).
        format_checked: bool,
    },
    SwNv12 {
        c: Converted,
        nv12: ID3D11Texture2D,
        staging: ID3D11Texture2D,
    },
    SwRgb32 {
        tex: wgpu::Texture,
        view: wgpu::TextureView,
        swizzle: bool,
    },
}

pub struct MfSink {
    writer: IMFSinkWriter,
    video: u32,
    audio: Vec<u32>,
    stage: Stage,
    pool: SharedPool,
    path: Path_,
    gpu: Rc<Gpu>,
    bridge: Rc<Bridge>,
    size: (u32, u32),
    /// Encoder name and whether it is a hardware transform.
    pub encoder: String,
    pub hardware: bool,
    /// NV12 made by our own converter (BT.709 exact); false = RGB32 through Media Foundation's.
    pub own_nv12: bool,
    began: bool,
}

fn video_types(cfg: &EncoderConfig, input: Input) -> Result<(IMFMediaType, IMFMediaType), String> {
    // SAFETY: attribute writes on freshly created media types.
    unsafe {
        let out = MFCreateMediaType().map_err(err("MFCreateMediaType"))?;
        out.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
            .map_err(err("out"))?;
        out.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)
            .map_err(err("out"))?;
        out.SetUINT32(&MF_MT_AVG_BITRATE, cfg.bitrate)
            .map_err(err("out"))?;
        out.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
            .map_err(err("out"))?;
        // High, as LH.
        out.SetUINT32(&MF_MT_MPEG2_PROFILE, 100)
            .map_err(err("out"))?;
        out.SetUINT32(&MF_MT_YUV_MATRIX, MFVideoTransferMatrix_BT709.0 as u32)
            .map_err(err("out"))?;
        out.SetUINT32(&MF_MT_VIDEO_PRIMARIES, MFVideoPrimaries_BT709.0 as u32)
            .map_err(err("out"))?;
        out.SetUINT32(&MF_MT_TRANSFER_FUNCTION, MFVideoTransFunc_709.0 as u32)
            .map_err(err("out"))?;
        out.SetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE, MFNominalRange_16_235.0 as u32)
            .map_err(err("out"))?;
        set_size(&out, &MF_MT_FRAME_SIZE, cfg.width, cfg.height).map_err(err("out"))?;
        set_size(&out, &MF_MT_FRAME_RATE, cfg.fps, 1).map_err(err("out"))?;
        set_size(&out, &MF_MT_PIXEL_ASPECT_RATIO, 1, 1).map_err(err("out"))?;

        let inp = MFCreateMediaType().map_err(err("MFCreateMediaType"))?;
        inp.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
            .map_err(err("in"))?;
        let (subtype, stride) = match input {
            Input::Nv12Textures => (MFVideoFormat_NV12, None),
            Input::Nv12Memory => (MFVideoFormat_NV12, Some(cfg.width)),
            Input::Rgb32Memory => (MFVideoFormat_RGB32, Some(cfg.width * 4)),
        };
        inp.SetGUID(&MF_MT_SUBTYPE, &subtype).map_err(err("in"))?;
        inp.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
            .map_err(err("in"))?;
        inp.SetUINT32(&MF_MT_ALL_SAMPLES_INDEPENDENT, 1)
            .map_err(err("in"))?;
        inp.SetUINT32(&MF_MT_YUV_MATRIX, MFVideoTransferMatrix_BT709.0 as u32)
            .map_err(err("in"))?;
        let range = if input == Input::Rgb32Memory {
            MFNominalRange_0_255
        } else {
            MFNominalRange_16_235
        };
        inp.SetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE, range.0 as u32)
            .map_err(err("in"))?;
        if let Some(s) = stride {
            inp.SetUINT32(&MF_MT_DEFAULT_STRIDE, s).map_err(err("in"))?;
        }
        set_size(&inp, &MF_MT_FRAME_SIZE, cfg.width, cfg.height).map_err(err("in"))?;
        set_size(&inp, &MF_MT_FRAME_RATE, cfg.fps, 1).map_err(err("in"))?;
        set_size(&inp, &MF_MT_PIXEL_ASPECT_RATIO, 1, 1).map_err(err("in"))?;
        Ok((out, inp))
    }
}

fn audio_types(cfg: &EncoderConfig) -> Result<(IMFMediaType, IMFMediaType), String> {
    const RATE: u32 = 48_000;
    const CHANNELS: u32 = 2;
    // SAFETY: attribute writes on freshly created media types.
    unsafe {
        let out = MFCreateMediaType().map_err(err("MFCreateMediaType"))?;
        out.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)
            .map_err(err("aac"))?;
        out.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_AAC)
            .map_err(err("aac"))?;
        out.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)
            .map_err(err("aac"))?;
        out.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, RATE)
            .map_err(err("aac"))?;
        out.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, CHANNELS)
            .map_err(err("aac"))?;
        out.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, cfg.audio_bitrate / 8)
            .map_err(err("aac"))?;
        let inp = MFCreateMediaType().map_err(err("MFCreateMediaType"))?;
        inp.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)
            .map_err(err("pcm"))?;
        inp.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM)
            .map_err(err("pcm"))?;
        inp.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)
            .map_err(err("pcm"))?;
        inp.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, RATE)
            .map_err(err("pcm"))?;
        inp.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, CHANNELS)
            .map_err(err("pcm"))?;
        inp.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, CHANNELS * 2)
            .map_err(err("pcm"))?;
        inp.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, RATE * CHANNELS * 2)
            .map_err(err("pcm"))?;
        inp.SetUINT32(&MF_MT_ALL_SAMPLES_INDEPENDENT, 1)
            .map_err(err("pcm"))?;
        Ok((out, inp))
    }
}

/// The D3D11 texture (and its array slice) behind an allocated sample.
fn sample_texture(sample: &IMFSample) -> Result<(ID3D11Texture2D, u32), String> {
    // SAFETY: the buffer of an allocator sample is a DXGI buffer on the bridge's device.
    unsafe {
        let buf = sample
            .GetBufferByIndex(0)
            .map_err(err("GetBufferByIndex"))?;
        let dx: IMFDXGIBuffer = buf.cast().map_err(err("IMFDXGIBuffer"))?;
        let mut tex: Option<ID3D11Texture2D> = None;
        dx.GetResource(&ID3D11Texture2D::IID, (&raw mut tex).cast())
            .map_err(err("GetResource"))?;
        let sub = dx
            .GetSubresourceIndex()
            .map_err(err("GetSubresourceIndex"))?;
        Ok((tex.ok_or("семпл без текстури")?, sub))
    }
}

/// A memory buffer of `bytes` in a sample at `time` / `duration`.
fn memory_sample(bytes: &[u8], time: i64, duration: i64) -> Result<IMFSample, String> {
    let len = bytes.len() as u32;
    // SAFETY: the buffer is locked for exactly `len` bytes, written, unlocked.
    unsafe {
        let buf = MFCreateMemoryBuffer(len).map_err(err("MFCreateMemoryBuffer"))?;
        let mut p = std::ptr::null_mut();
        buf.Lock(&mut p, None, None).map_err(err("Lock"))?;
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
        buf.Unlock().map_err(err("Unlock"))?;
        buf.SetCurrentLength(len).map_err(err("SetCurrentLength"))?;
        let sample = MFCreateSample().map_err(err("MFCreateSample"))?;
        sample.AddBuffer(&buf).map_err(err("AddBuffer"))?;
        sample.SetSampleTime(time).map_err(err("SetSampleTime"))?;
        sample
            .SetSampleDuration(duration)
            .map_err(err("SetSampleDuration"))?;
        Ok(sample)
    }
}

fn nv12_texture(bridge: &Bridge, w: u32, h: u32, staging: bool) -> Result<ID3D11Texture2D, String> {
    let td = D3D11_TEXTURE2D_DESC {
        Width: w,
        Height: h,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_NV12,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: if staging {
            D3D11_USAGE_STAGING
        } else {
            D3D11_USAGE_DEFAULT
        },
        BindFlags: if staging {
            0
        } else {
            (D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE).0 as u32
        },
        CPUAccessFlags: if staging {
            D3D11_CPU_ACCESS_READ.0 as u32
        } else {
            0
        },
        MiscFlags: 0,
    };
    let mut t = None;
    // SAFETY: plain texture creation.
    unsafe { bridge.device.CreateTexture2D(&td, None, Some(&mut t)) }
        .map_err(err("CreateTexture2D (NV12)"))?;
    t.ok_or_else(|| "NV12 texture".into())
}

fn converted(gpu: &Gpu, bridge: &Bridge, cfg: &EncoderConfig) -> Result<Converted, String> {
    let mut out = Vec::with_capacity(OUT_SLOTS);
    for _ in 0..OUT_SLOTS {
        let t = bridge.shared_texture(
            gpu,
            cfg.width,
            cfg.height,
            DXGI_FORMAT_B8G8R8A8_UNORM,
            (D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_UNORDERED_ACCESS).0 as u32,
            wgpu::TextureFormat::Bgra8Unorm,
            wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
            "record output",
        )?;
        out.push((t, 0));
    }
    let scratch = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("record scratch"),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Bgra8Unorm,
        usage: wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let conv = Nv12Converter::new(&bridge.device, &bridge.ctx, cfg.width, cfg.height, cfg.fps)?;
    Ok(Converted {
        out,
        next: 0,
        scratch,
        conv,
    })
}

/// The shader for one frame into `dst`; `publish`: the shared texture D3D11 reads next — after
/// the compute pass one texel of it is copied out, so wgpu moves it from the UAV state to a read
/// state (its writes flushed) and D3D12 lets it decay to COMMON at the end of that submit, which
/// a resource shared with D3D11 must be in. Returns the fence value the D3D11 side waits for.
#[allow(clippy::too_many_arguments)]
fn render_frame(
    gpu: &Gpu,
    bridge: &Bridge,
    stage: &mut Stage,
    pool: &SharedPool,
    frame: &GpuFrame,
    dst: &wgpu::TextureView,
    consumed: u64,
    publish: Option<(&wgpu::Texture, &wgpu::Texture)>,
) -> Result<u64, String> {
    let p = pool.borrow();
    let slot = p.slots.get(frame.slot).ok_or("слот пулу зник")?;
    bridge.wait_in_wgpu(gpu, frame.ready)?;
    bridge.wait_in_wgpu(gpu, consumed)?;
    let done = match publish {
        None => {
            let done = bridge.signal_in_wgpu(gpu)?;
            stage.render(gpu, &slot.tex.view, dst, &frame.geometry, &frame.overlay);
            done
        }
        Some((shared, scratch)) => {
            stage.render(gpu, &slot.tex.view, dst, &frame.geometry, &frame.overlay);
            let done = bridge.signal_in_wgpu(gpu)?;
            let mut enc = gpu.device.create_command_encoder(&Default::default());
            enc.copy_texture_to_texture(
                shared.as_image_copy(),
                scratch.as_image_copy(),
                wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
            );
            gpu.queue.submit([enc.finish()]);
            done
        }
    };
    drop(p);
    pool.borrow_mut().mark_read(frame.slot, done);
    Ok(done)
}

/// Shader → shared BGRA8 → NV12 in `dst[sub]` on the D3D11 side.
#[allow(clippy::too_many_arguments)]
fn convert_into(
    gpu: &Gpu,
    bridge: &Bridge,
    stage: &mut Stage,
    pool: &SharedPool,
    c: &mut Converted,
    frame: &GpuFrame,
    dst: &ID3D11Texture2D,
    sub: u32,
) -> Result<(), String> {
    let i = c.next;
    c.next = (i + 1) % OUT_SLOTS;
    let consumed = c.out[i].1;
    let done = render_frame(
        gpu,
        bridge,
        stage,
        pool,
        frame,
        &c.out[i].0.view,
        consumed,
        Some((&c.out[i].0.texture, &c.scratch)),
    )?;
    bridge.wait_d3d11(done);
    c.conv.blt(&c.out[i].0.tex11, dst, sub)?;
    c.out[i].1 = bridge.signal_d3d11();
    Ok(())
}

impl MfSink {
    /// Open the writer on `path` (the `.part` file). `hardware`: the D3D manager and the NV12
    /// texture pool; otherwise the software encoder with memory buffers.
    pub fn open(
        gpu: Rc<Gpu>,
        bridge: Rc<Bridge>,
        pool: SharedPool,
        path: &Path,
        cfg: &EncoderConfig,
        hardware: bool,
    ) -> Result<Self, String> {
        if cfg.width < 2
            || cfg.height < 2
            || !cfg.width.is_multiple_of(2)
            || !cfg.height.is_multiple_of(2)
        {
            return Err(format!(
                "розмір {}×{} має бути парним",
                cfg.width, cfg.height
            ));
        }
        if cfg.fps == 0 {
            return Err("fps = 0".into());
        }
        let abs = std::path::absolute(path).map_err(|e| e.to_string())?;
        let hardware = hardware && gpu.bgra_storage;
        let input = if hardware {
            Input::Nv12Textures
        } else if gpu.bgra_storage {
            Input::Nv12Memory
        } else {
            Input::Rgb32Memory
        };
        let format = if gpu.bgra_storage {
            OutFormat::Bgra8
        } else {
            OutFormat::Rgba8
        };
        let stage = Stage::new(&gpu, format)?;
        // SAFETY: Media Foundation calls with owned interfaces; the writer is finalised or
        // dropped by the sink.
        unsafe {
            let mut attrs = None;
            MFCreateAttributes(&mut attrs, 4).map_err(err("MFCreateAttributes"))?;
            let attrs = attrs.ok_or("attrs")?;
            // The extension `.part` says nothing about the container (§7 item 9).
            attrs
                .SetGUID(&MF_TRANSCODE_CONTAINERTYPE, &MFTranscodeContainerType_MPEG4)
                .map_err(err("attr"))?;
            // Frames leave the encoder in order and at once: no reordering delay, no composition
            // offsets — the first frame of the file is at 0.
            attrs.SetUINT32(&MF_LOW_LATENCY, 1).map_err(err("attr"))?;
            if hardware {
                attrs
                    .SetUnknown(&MF_SINK_WRITER_D3D_MANAGER, &bridge.manager)
                    .map_err(err("attr"))?;
                attrs
                    .SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)
                    .map_err(err("attr"))?;
            } else {
                attrs
                    .SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 0)
                    .map_err(err("attr"))?;
            }
            let writer = MFCreateSinkWriterFromURL(&HSTRING::from(abs.as_os_str()), None, &attrs)
                .map_err(err("MFCreateSinkWriterFromURL"))?;
            let (out, inp) = video_types(cfg, input)?;
            let video = writer.AddStream(&out).map_err(err("AddStream (video)"))?;
            let mut ep = None;
            MFCreateAttributes(&mut ep, 2).map_err(err("MFCreateAttributes"))?;
            let ep = ep.ok_or("encoder params")?;
            ep.SetUINT32(&GOP_SIZE, cfg.keyframe_interval.max(1))
                .map_err(err("gop"))?;
            ep.SetUINT32(&B_COUNT, cfg.b_frames)
                .map_err(err("bcount"))?;
            writer
                .SetInputMediaType(video, &inp, &ep)
                .map_err(err("SetInputMediaType (video)"))?;
            let mut audio = Vec::new();
            for _ in 0..cfg.audio_tracks {
                let (ao, ai) = audio_types(cfg)?;
                let s = writer.AddStream(&ao).map_err(err("AddStream (audio)"))?;
                writer
                    .SetInputMediaType(s, &ai, None)
                    .map_err(err("SetInputMediaType (audio)"))?;
                audio.push(s);
            }
            // The stream's transforms: a converter or two, then the encoder — name the encoder.
            let (encoder, hw) = writer
                .cast::<IMFSinkWriterEx>()
                .ok()
                .and_then(|w| {
                    let mut found = None;
                    for i in 0..4 {
                        let mut cat = GUID::zeroed();
                        let mut t = None;
                        if w.GetTransformForStream(video, i, Some(&mut cat), &mut t)
                            .is_err()
                        {
                            break;
                        }
                        if let Some(t) = t {
                            let named = transform_name(&t);
                            if cat == MFT_CATEGORY_VIDEO_ENCODER || found.is_none() {
                                found = Some(named);
                            }
                            if cat == MFT_CATEGORY_VIDEO_ENCODER {
                                break;
                            }
                        }
                    }
                    found
                })
                .unwrap_or(("?".into(), false));
            // A transform rarely carries a friendly name outside enumeration: say what it is.
            let encoder = if encoder == "?" {
                if hw {
                    "апаратний H.264 MFT".to_string()
                } else {
                    "Microsoft H.264 (програмний)".to_string()
                }
            } else {
                encoder
            };
            let path_ = match input {
                Input::Nv12Textures => {
                    let mut raw = std::ptr::null_mut();
                    MFCreateVideoSampleAllocatorEx(&IMFVideoSampleAllocatorEx::IID, &mut raw)
                        .map_err(err("MFCreateVideoSampleAllocatorEx"))?;
                    let allocator = IMFVideoSampleAllocatorEx::from_raw(raw);
                    allocator
                        .SetDirectXManager(&bridge.manager)
                        .map_err(err("SetDirectXManager"))?;
                    let mut sa = None;
                    MFCreateAttributes(&mut sa, 3).map_err(err("MFCreateAttributes"))?;
                    let sa = sa.ok_or("allocator attrs")?;
                    sa.SetUINT32(
                        &MF_SA_D3D11_BINDFLAGS,
                        (D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE).0 as u32,
                    )
                    .map_err(err("sa"))?;
                    sa.SetUINT32(&MF_SA_D3D11_USAGE, D3D11_USAGE_DEFAULT.0 as u32)
                        .map_err(err("sa"))?;
                    sa.SetUINT32(&MF_SA_BUFFERS_PER_SAMPLE, 1)
                        .map_err(err("sa"))?;
                    allocator
                        .InitializeSampleAllocatorEx(POOL_INITIAL, POOL_MAX, &sa, &inp)
                        .map_err(err("InitializeSampleAllocatorEx"))?;
                    Path_::Hw {
                        allocator,
                        c: converted(&gpu, &bridge, cfg)?,
                        format_checked: false,
                    }
                }
                Input::Nv12Memory => match converted(&gpu, &bridge, cfg) {
                    Ok(c) => Path_::SwNv12 {
                        c,
                        nv12: nv12_texture(&bridge, cfg.width, cfg.height, false)?,
                        staging: nv12_texture(&bridge, cfg.width, cfg.height, true)?,
                    },
                    // No video processor on this device: RGB32 to the writer, its converter.
                    Err(e) => {
                        tracing::warn!("no D3D11 video processor ({e}): RGB32 into the encoder");
                        let (_, inp) = video_types(cfg, Input::Rgb32Memory)?;
                        writer
                            .SetInputMediaType(video, &inp, &ep)
                            .map_err(err("SetInputMediaType (RGB32)"))?;
                        let tex = local_output(&gpu, format, cfg.width, cfg.height);
                        let view = tex.create_view(&Default::default());
                        Path_::SwRgb32 {
                            tex,
                            view,
                            swizzle: format == OutFormat::Rgba8,
                        }
                    }
                },
                Input::Rgb32Memory => {
                    let tex = local_output(&gpu, format, cfg.width, cfg.height);
                    let view = tex.create_view(&Default::default());
                    Path_::SwRgb32 {
                        tex,
                        view,
                        swizzle: format == OutFormat::Rgba8,
                    }
                }
            };
            writer.BeginWriting().map_err(err("BeginWriting"))?;
            let own_nv12 = !matches!(path_, Path_::SwRgb32 { .. });
            Ok(Self {
                writer,
                video,
                audio,
                stage,
                pool,
                path: path_,
                gpu,
                bridge,
                size: (cfg.width, cfg.height),
                encoder: format!(
                    "{encoder}{}",
                    if hw { " (hardware)" } else { " (software)" }
                ),
                hardware,
                own_nv12,
                began: true,
            })
        }
    }

    /// Hardware first, the software encoder when that fails (LH §2.5); with `hardware = false`
    /// straight to software.
    pub fn open_best(
        gpu: Rc<Gpu>,
        bridge: Rc<Bridge>,
        pool: SharedPool,
        path: &Path,
        cfg: &EncoderConfig,
        hardware: bool,
    ) -> Result<Self, String> {
        if hardware {
            match Self::open(gpu.clone(), bridge.clone(), pool.clone(), path, cfg, true) {
                Ok(s) => return Ok(s),
                Err(e) => {
                    // A half-opened writer may have created the file: start over.
                    let _ = std::fs::remove_file(path);
                    tracing::warn!("no hardware encoder ({e}); the software one");
                }
            }
        }
        Self::open(gpu, bridge, pool, path, cfg, false)
    }

    fn write_hw(&mut self, frame: &GpuFrame, t: VideoSampleTime) -> Result<(), SinkError> {
        let Path_::Hw {
            allocator,
            c,
            format_checked,
        } = &mut self.path
        else {
            unreachable!()
        };
        // SAFETY: MF and D3D11 calls on live objects of one device.
        unsafe {
            let sample = match allocator.AllocateSample() {
                Ok(s) => s,
                Err(e) if e.code() == MF_E_SAMPLEALLOCATOR_EMPTY => return Err(SinkError::Busy),
                Err(e) => return Err(SinkError::Failed(err("AllocateSample")(e))),
            };
            let (tex, sub) = sample_texture(&sample).map_err(SinkError::Failed)?;
            if !*format_checked {
                let mut desc = Default::default();
                tex.GetDesc(&mut desc);
                if desc.Format != DXGI_FORMAT_NV12 {
                    return Err(SinkError::Failed(format!(
                        "текстура пулу кодувальника у форматі {:?}, а не NV12",
                        desc.Format
                    )));
                }
                *format_checked = true;
            }
            convert_into(
                &self.gpu,
                &self.bridge,
                &mut self.stage,
                &self.pool,
                c,
                frame,
                &tex,
                sub,
            )
            .map_err(SinkError::Failed)?;
            // A DXGI buffer of the allocator starts with a current length of 0, and the writer
            // refuses such a sample (E_INVALIDARG): say the whole surface is there.
            if let Ok(buf) = sample.GetBufferByIndex(0)
                && let Ok(max) = buf.GetMaxLength()
            {
                let _ = buf.SetCurrentLength(max);
            }
            sample
                .SetSampleTime(t.time)
                .map_err(|e| SinkError::Failed(err("SetSampleTime")(e)))?;
            sample
                .SetSampleDuration(t.duration)
                .map_err(|e| SinkError::Failed(err("SetSampleDuration")(e)))?;
            self.writer
                .WriteSample(self.video, &sample)
                .map_err(|e| SinkError::Failed(err("WriteSample")(e)))?;
        }
        Ok(())
    }

    fn write_sw_nv12(&mut self, frame: &GpuFrame, t: VideoSampleTime) -> Result<(), SinkError> {
        let Path_::SwNv12 { c, nv12, staging } = &mut self.path else {
            unreachable!()
        };
        convert_into(
            &self.gpu,
            &self.bridge,
            &mut self.stage,
            &self.pool,
            c,
            frame,
            nv12,
            0,
        )
        .map_err(SinkError::Failed)?;
        let (w, h) = (self.size.0 as usize, self.size.1 as usize);
        let mut bytes = Vec::with_capacity(w * h * 3 / 2);
        // SAFETY: the staging copy is mapped for reading; rows are read within RowPitch.
        unsafe {
            self.bridge.ctx.CopyResource(&*staging, &*nv12);
            let mut m = D3D11_MAPPED_SUBRESOURCE::default();
            self.bridge
                .ctx
                .Map(&*staging, 0, D3D11_MAP_READ, 0, Some(&mut m))
                .map_err(|e| SinkError::Failed(err("Map (NV12)")(e)))?;
            let pitch = m.RowPitch as usize;
            let p = m.pData.cast::<u8>();
            for r in 0..h {
                bytes.extend_from_slice(std::slice::from_raw_parts(p.add(r * pitch), w));
            }
            let c0 = p.add(pitch * h);
            for r in 0..h / 2 {
                bytes.extend_from_slice(std::slice::from_raw_parts(c0.add(r * pitch), w));
            }
            self.bridge.ctx.Unmap(&*staging, 0);
        }
        let sample = memory_sample(&bytes, t.time, t.duration).map_err(SinkError::Failed)?;
        // SAFETY: a write on the live writer.
        unsafe { self.writer.WriteSample(self.video, &sample) }
            .map_err(|e| SinkError::Failed(err("WriteSample")(e)))
    }

    fn write_sw_rgb32(&mut self, frame: &GpuFrame, t: VideoSampleTime) -> Result<(), SinkError> {
        let Path_::SwRgb32 { tex, view, swizzle } = &self.path else {
            unreachable!()
        };
        let (view, tex, swizzle) = (view.clone(), tex.clone(), *swizzle);
        render_frame(
            &self.gpu,
            &self.bridge,
            &mut self.stage,
            &self.pool,
            frame,
            &view,
            0,
            None,
        )
        .map_err(SinkError::Failed)?;
        let mut pixels = read_back(&self.gpu, &tex);
        if swizzle {
            for p in pixels.as_chunks_mut::<4>().0 {
                p.swap(0, 2);
            }
        }
        let sample = memory_sample(&pixels, t.time, t.duration).map_err(SinkError::Failed)?;
        // SAFETY: a write on the live writer.
        unsafe { self.writer.WriteSample(self.video, &sample) }
            .map_err(|e| SinkError::Failed(err("WriteSample")(e)))
    }
}

impl VideoSink for MfSink {
    type Frame = GpuFrame;

    fn caps(&self) -> SinkCaps {
        // The MF muxer honours sample durations (§7 item 1).
        SinkCaps {
            carries_duration: true,
        }
    }

    fn write_video(&mut self, frame: &GpuFrame, t: VideoSampleTime) -> Result<(), SinkError> {
        match self.path {
            Path_::Hw { .. } => self.write_hw(frame, t),
            Path_::SwNv12 { .. } => self.write_sw_nv12(frame, t),
            Path_::SwRgb32 { .. } => self.write_sw_rgb32(frame, t),
        }
    }

    fn write_audio(
        &mut self,
        track: usize,
        pcm: &[i16],
        t: AudioSampleTime,
    ) -> Result<(), SinkError> {
        let Some(&stream) = self.audio.get(track) else {
            return Err(SinkError::Failed(format!("немає доріжки {track}")));
        };
        let bytes: Vec<u8> = pcm.iter().flat_map(|v| v.to_le_bytes()).collect();
        if bytes.is_empty() {
            return Ok(());
        }
        let sample = memory_sample(&bytes, t.time, t.duration).map_err(SinkError::Failed)?;
        // SAFETY: a write on the live writer.
        unsafe { self.writer.WriteSample(stream, &sample) }
            .map_err(|e| SinkError::Failed(err("WriteSample (audio)")(e)))
    }

    fn finalize(&mut self) -> Result<(), SinkError> {
        if !self.began {
            return Ok(());
        }
        self.began = false;
        // The encoder may still hold our output textures: let the GPU finish first.
        self.gpu.wait();
        // SAFETY: Finalize on a writer that began writing.
        unsafe { self.writer.Finalize() }.map_err(|e| SinkError::Failed(err("Finalize")(e)))
    }
}
