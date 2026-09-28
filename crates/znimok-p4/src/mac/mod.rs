//! macOS side of P4 (ZK-17): AVAssetReader decodes (VideoToolbox, hardware) into NV12
//! `CVPixelBuffer`s backed by IOSurface; `CVMetalTextureCache` wraps each plane as an `MTLTexture`
//! on wgpu's own `MTLDevice` (no copy), and `hal::metal::Device::texture_from_raw` hands them to
//! wgpu — the same `nv12.wgsl` pass as on Windows reads them. Fallback: lock the pixel buffer and
//! upload the planes. Seeking: AVAssetReader cannot seek, so a new reader starts at the wanted time
//! (VideoToolbox decodes from the key frame before it) and frames are read until the wanted one.

use std::collections::VecDeque;
use std::ptr::NonNull;
use std::time::{Duration, Instant};

use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::AnyObject;
use objc2_av_foundation::{
    AVAssetReader, AVAssetReaderTrackOutput, AVAssetTrack, AVMediaTypeVideo, AVURLAsset,
    NSValueAVFoundationExtensions,
};
use objc2_core_foundation::{CFRetained, CFString};
use objc2_core_media::{CMSampleBuffer, CMTime, CMTimeFlags, CMTimeRange, kCMTimePositiveInfinity};
use objc2_core_video::{
    CVImageBuffer, CVMetalTexture, CVMetalTextureCache, CVMetalTextureGetTexture,
    CVPixelBufferGetBaseAddressOfPlane, CVPixelBufferGetBytesPerRowOfPlane,
    CVPixelBufferGetHeightOfPlane, CVPixelBufferGetWidthOfPlane, CVPixelBufferLockBaseAddress,
    CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress, kCVPixelBufferMetalCompatibilityKey,
    kCVPixelBufferPixelFormatTypeKey, kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
};
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString, NSURL, NSValue};
use objc2_metal::{MTLDevice, MTLPixelFormat, MTLTextureType};
use serde_json::json;
use wgpu::hal::api::Metal;

use crate::gpu::{Gpu, Planes};
use crate::pattern;

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|s| s == name)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

fn num<T: std::str::FromStr>(args: &[String], name: &str, default: T) -> Result<T, String> {
    match flag(args, name) {
        Some(v) => v.parse().map_err(|_| format!("{name}: не число «{v}»")),
        None => Ok(default),
    }
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

pub fn run(cmd: &str, args: &[String]) -> Result<(), String> {
    match cmd {
        "info" => info(),
        "bench" => bench(args),
        "seek" => seek(args),
        _ => Err(format!(
            "невідома команда «{cmd}» (на macOS: info, bench, seek; кліпи — з Windows `gen`)"
        )),
    }
}

fn info() -> Result<(), String> {
    let gpu = Gpu::new()?;
    let dev = metal_device(&gpu)?;
    println!(
        "{}",
        json!({"wgpu_adapter": gpu.adapter.name, "backend": format!("{:?}", gpu.adapter.backend),
               "metal_device": dev.name().to_string(), "unified_memory": dev.hasUnifiedMemory()})
    );
    Ok(())
}

fn metal_device(
    gpu: &Gpu,
) -> Result<Retained<objc2::runtime::ProtocolObject<dyn objc2_metal::MTLDevice>>, String> {
    // SAFETY: only reads the device handle wgpu created.
    let hal = unsafe { gpu.device.as_hal::<Metal>() }.ok_or("wgpu не на Metal")?;
    Ok(hal.raw_device().clone())
}

/// CPU time of this process (user + system).
fn cpu_time() -> Duration {
    let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
    // SAFETY: valid out-pointer.
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut ru) };
    let tv = |t: libc::timeval| Duration::new(t.tv_sec as u64, t.tv_usec as u32 * 1000);
    tv(ru.ru_utime) + tv(ru.ru_stime)
}

fn cm_time(seconds: f64) -> CMTime {
    // 600 is the usual video timescale; exact for 24/25/30/60 fps frame starts.
    CMTime {
        value: (seconds * 600.0).round() as i64,
        timescale: 600,
        flags: CMTimeFlags::Valid,
        epoch: 0,
    }
}

fn cm_seconds(t: CMTime) -> f64 {
    if t.timescale == 0 {
        0.0
    } else {
        t.value as f64 / f64::from(t.timescale)
    }
}

struct Reader {
    track: Retained<AVAssetTrack>,
    asset: Retained<AVURLAsset>,
    reader: Retained<AVAssetReader>,
    output: Retained<AVAssetReaderTrackOutput>,
    fps: f64,
    duration: f64,
    /// One random-access reader (default; `--new-reader` turns it off) for all seeks (`resetForReadingTimeRanges`) instead of a
    /// new AVAssetReader per seek.
    reuse: bool,
    ra: Option<(Retained<AVAssetReader>, Retained<AVAssetReaderTrackOutput>)>,
}

impl Reader {
    fn open(path: &str) -> Result<Self, String> {
        let abs = std::path::absolute(path).map_err(|e| e.to_string())?;
        let url = NSURL::fileURLWithPath(&NSString::from_str(&abs.to_string_lossy()));
        // SAFETY: plain AVFoundation calls on objects we own.
        unsafe {
            let asset = AVURLAsset::URLAssetWithURL_options(&url, None);
            #[allow(deprecated)]
            let tracks = asset.tracksWithMediaType(AVMediaTypeVideo.ok_or("AVMediaTypeVideo")?);
            let track = tracks.firstObject().ok_or("у файлі немає відеодоріжки")?;
            let fps = f64::from(track.nominalFrameRate());
            let duration = cm_seconds(asset.duration());
            let (reader, output) = Self::start(&asset, &track, None)?;
            Ok(Self {
                track,
                asset,
                reader,
                output,
                fps,
                duration,
                reuse: false,
                ra: None,
            })
        }
    }

    /// A fresh reader, optionally starting at `from` seconds.
    unsafe fn start(
        asset: &AVURLAsset,
        track: &AVAssetTrack,
        from: Option<f64>,
    ) -> Result<(Retained<AVAssetReader>, Retained<AVAssetReaderTrackOutput>), String> {
        unsafe { Self::start_with(asset, track, from.map(|t| (t, None)), false) }
    }

    /// A fresh reader for `range` (start, optional duration); `random` enables later
    /// `resetForReadingTimeRanges`.
    unsafe fn start_with(
        asset: &AVURLAsset,
        track: &AVAssetTrack,
        range: Option<(f64, Option<f64>)>,
        random: bool,
    ) -> Result<(Retained<AVAssetReader>, Retained<AVAssetReaderTrackOutput>), String> {
        unsafe {
            let reader = AVAssetReader::assetReaderWithAsset_error(asset)
                .map_err(|e| format!("AVAssetReader: {}", e.localizedDescription()))?;
            // NV12 video range, IOSurface-backed and Metal-compatible, so planes wrap as textures.
            let k1: &NSString =
                &*(kCVPixelBufferPixelFormatTypeKey as *const CFString).cast::<NSString>();
            let k2: &NSString =
                &*(kCVPixelBufferMetalCompatibilityKey as *const CFString).cast::<NSString>();
            let v1 = NSNumber::new_u32(kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange);
            let v2 = NSNumber::new_bool(true);
            let dict = NSDictionary::from_slices(&[k1, k2], &[&*v1, &*v2]);
            let dict: &NSDictionary<NSString, AnyObject> = &*(Retained::as_ptr(&dict).cast());
            let output = AVAssetReaderTrackOutput::assetReaderTrackOutputWithTrack_outputSettings(
                track,
                Some(dict),
            );
            output.setAlwaysCopiesSampleData(false);
            output.setSupportsRandomAccess(random);
            if let Some((t, d)) = range {
                let duration = d.map_or(kCMTimePositiveInfinity, cm_time);
                reader.setTimeRange(CMTimeRange {
                    start: cm_time(t),
                    duration,
                });
            }
            reader.addOutput(&output);
            if !reader.startReading() {
                return Err("startReading: false".into());
            }
            Ok((reader, output))
        }
    }

    fn index_of(&self, pts: f64) -> u32 {
        (pts * self.fps).round() as u32
    }

    fn frame_count(&self) -> u32 {
        self.index_of(self.duration)
    }

    /// Next decoded frame: (presentation time, the sample, its pixel buffer).
    fn next(&self) -> Option<(f64, Retained<CMSampleBuffer>, CFRetained<CVImageBuffer>)> {
        loop {
            // SAFETY: the output belongs to a started reader.
            let s = unsafe { self.output.copyNextSampleBuffer() }?;
            let Some(pb) = (unsafe { s.image_buffer() }) else {
                continue;
            };
            let pts = cm_seconds(unsafe { s.presentation_time_stamp() });
            return Some((pts, s, pb));
        }
    }

    /// New reader from the wanted frame's time; read until that frame. Returns it and how many
    /// frames came out on the way.
    fn seek(
        &mut self,
        index: u32,
    ) -> Result<
        (
            f64,
            Retained<CMSampleBuffer>,
            CFRetained<CVImageBuffer>,
            u32,
        ),
        String,
    > {
        if self.reuse {
            return self.seek_reuse(index);
        }
        // SAFETY: cancelling our own reader, then a fresh one.
        unsafe { self.reader.cancelReading() };
        // AVAssetReader returns the frame that covers the range start and stamps it with the start
        // time: starting a quarter frame early gave frame t−1 labelled as t. Start just inside t.
        let from = (f64::from(index) + 0.1) / self.fps;
        let (r, o) = unsafe { Self::start(&self.asset, &self.track, Some(from.max(0.0)))? };
        self.reader = r;
        self.output = o;
        let mut n = 0;
        loop {
            let (pts, s, pb) = self.next().ok_or("кінець потоку до потрібного кадру")?;
            n += 1;
            if self.index_of(pts) >= index {
                return Ok((pts, s, pb, n));
            }
        }
    }
}

impl Reader {
    /// Seek through one random-access reader: read the previous range to its end (the API asks
    /// for that), then `resetForReadingTimeRanges` to half a frame inside the wanted one.
    fn seek_reuse(
        &mut self,
        index: u32,
    ) -> Result<
        (
            f64,
            Retained<CMSampleBuffer>,
            CFRetained<CVImageBuffer>,
            u32,
        ),
        String,
    > {
        let start = (f64::from(index) + 0.1) / self.fps;
        let len = 0.5 / self.fps;
        // SAFETY: AVFoundation calls on objects we own; ranges are valid CMTimeRanges.
        unsafe {
            match &self.ra {
                None => {
                    self.ra = Some(Self::start_with(
                        &self.asset,
                        &self.track,
                        Some((start, Some(len))),
                        true,
                    )?)
                }
                Some((_, o)) => {
                    while o.copyNextSampleBuffer().is_some() {}
                    let r = CMTimeRange {
                        start: cm_time(start),
                        duration: cm_time(len),
                    };
                    let v = NSValue::valueWithCMTimeRange(r);
                    o.resetForReadingTimeRanges(&NSArray::from_retained_slice(&[v]));
                }
            }
            let (_, o) = self.ra.as_ref().unwrap();
            let mut n = 0;
            loop {
                let s = o
                    .copyNextSampleBuffer()
                    .ok_or("порожній діапазон перемотки")?;
                let Some(pb) = s.image_buffer() else { continue };
                n += 1;
                let pts = cm_seconds(s.presentation_time_stamp());
                if self.index_of(pts) >= index {
                    return Ok((pts, s, pb, n));
                }
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Zero,
    Cpu,
}

/// What must stay alive until the GPU has read a frame.
struct InFlight {
    _textures: (wgpu::Texture, wgpu::Texture),
    _cv: (CFRetained<CVMetalTexture>, CFRetained<CVMetalTexture>),
    _pb: CFRetained<CVImageBuffer>,
    _sample: Retained<CMSampleBuffer>,
    done: wgpu::SubmissionIndex,
}

struct Player {
    gpu: Gpu,
    reader: Reader,
    mode: Mode,
    cache: CFRetained<CVMetalTextureCache>,
    out: Option<(wgpu::Texture, u32, u32)>,
    planes: Option<Planes>,
    bind: Option<wgpu::BindGroup>,
    in_flight: VecDeque<InFlight>,
    frames_on_gpu: Option<bool>,
}

impl Player {
    fn open(path: &str, mode: Mode) -> Result<Self, String> {
        let gpu = Gpu::new()?;
        let dev = metal_device(&gpu)?;
        let mut cache: *mut CVMetalTextureCache = std::ptr::null_mut();
        // SAFETY: out-pointer to a local; the cache is created on wgpu's own MTLDevice.
        let rc = unsafe {
            CVMetalTextureCache::create(None, None, &dev, None, NonNull::from(&mut cache))
        };
        let cache = NonNull::new(cache)
            .filter(|_| rc == 0)
            .ok_or(format!("CVMetalTextureCacheCreate: {rc}"))?;
        // SAFETY: Create rule — we own the reference.
        let cache = unsafe { CFRetained::from_raw(cache) };
        Ok(Self {
            gpu,
            reader: Reader::open(path)?,
            mode,
            cache,
            out: None,
            planes: None,
            bind: None,
            in_flight: VecDeque::new(),
            frames_on_gpu: None,
        })
    }

    fn target(&mut self, w: u32, h: u32) -> &wgpu::Texture {
        if self
            .out
            .as_ref()
            .is_none_or(|(_, ow, oh)| (*ow, *oh) != (w, h))
        {
            self.out = Some((self.gpu.target(w, h), w, h));
            self.planes = None;
            self.bind = None;
        }
        &self.out.as_ref().unwrap().0
    }

    fn plane_texture(
        &self,
        pb: &CVImageBuffer,
        plane: usize,
        format: MTLPixelFormat,
        wf: wgpu::TextureFormat,
    ) -> Result<(wgpu::Texture, CFRetained<CVMetalTexture>), String> {
        let (w, h) = (
            CVPixelBufferGetWidthOfPlane(pb, plane),
            CVPixelBufferGetHeightOfPlane(pb, plane),
        );
        let mut t: *mut CVMetalTexture = std::ptr::null_mut();
        // SAFETY: the pixel buffer is IOSurface-backed (Metal compatible, asked for in the reader settings).
        let rc = unsafe {
            CVMetalTextureCache::create_texture_from_image(
                None,
                &self.cache,
                pb,
                None,
                format,
                w,
                h,
                plane,
                NonNull::from(&mut t),
            )
        };
        let t = NonNull::new(t)
            .filter(|_| rc == 0)
            .ok_or(format!("CVMetalTextureCacheCreateTextureFromImage: {rc}"))?;
        // SAFETY: Create rule.
        let cv = unsafe { CFRetained::from_raw(t) };
        let mtl = CVMetalTextureGetTexture(&cv).ok_or("CVMetalTextureGetTexture: null")?;
        let size = wgpu::Extent3d {
            width: w as u32,
            height: h as u32,
            depth_or_array_layers: 1,
        };
        // SAFETY: the texture lives as long as `cv`, which InFlight keeps until the GPU is done.
        let tex = unsafe {
            let hal = wgpu::hal::metal::Device::texture_from_raw(
                mtl,
                wf,
                MTLTextureType::Type2D,
                1,
                1,
                wgpu::hal::CopyExtent {
                    width: size.width,
                    height: size.height,
                    depth: 1,
                },
                None,
            );
            self.gpu.device.create_texture_from_hal::<Metal>(
                hal,
                &wgpu::TextureDescriptor {
                    label: Some("cv plane"),
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wf,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
                wgpu::TextureUses::empty(),
            )
        };
        Ok((tex, cv))
    }

    /// Queue the conversion of one frame into the output texture.
    fn show(
        &mut self,
        sample: Retained<CMSampleBuffer>,
        pb: CFRetained<CVImageBuffer>,
    ) -> Result<(), String> {
        let (w, h) = (
            CVPixelBufferGetWidthOfPlane(&pb, 0) as u32,
            CVPixelBufferGetHeightOfPlane(&pb, 0) as u32,
        );
        self.target(w, h);
        self.frames_on_gpu.get_or_insert(true);
        match self.mode {
            Mode::Zero => {
                let (ty, cy) = self.plane_texture(
                    &pb,
                    0,
                    MTLPixelFormat::R8Unorm,
                    wgpu::TextureFormat::R8Unorm,
                )?;
                let (tuv, cuv) = self.plane_texture(
                    &pb,
                    1,
                    MTLPixelFormat::RG8Unorm,
                    wgpu::TextureFormat::Rg8Unorm,
                )?;
                let out = &self.out.as_ref().unwrap().0;
                let bind = self.gpu.bind(
                    &ty.create_view(&Default::default()),
                    &tuv.create_view(&Default::default()),
                    out,
                );
                let done = self.gpu.queue.submit([self.gpu.convert(&bind, w, h)]);
                self.in_flight.push_back(InFlight {
                    _textures: (ty, tuv),
                    _cv: (cy, cuv),
                    _pb: pb,
                    _sample: sample,
                    done,
                });
                // Three frames in flight at most; then wait for the oldest before releasing it.
                while self.in_flight.len() > 3 {
                    let f = self.in_flight.pop_front().unwrap();
                    let _ = self.gpu.device.poll(wgpu::PollType::Wait {
                        submission_index: Some(f.done.clone()),
                        timeout: None,
                    });
                }
                self.cache.flush(0);
            }
            Mode::Cpu => {
                if self.planes.is_none() {
                    let p = self.gpu.planes(w, h);
                    let out = &self.out.as_ref().unwrap().0;
                    self.bind = Some(self.gpu.bind(
                        &p.y.create_view(&Default::default()),
                        &p.uv.create_view(&Default::default()),
                        out,
                    ));
                    self.planes = Some(p);
                }
                // SAFETY: lock read-only, read within bytes-per-row × rows of each plane, unlock.
                unsafe {
                    if CVPixelBufferLockBaseAddress(&pb, CVPixelBufferLockFlags::ReadOnly) != 0 {
                        return Err("CVPixelBufferLockBaseAddress".into());
                    }
                    let plane = |i: usize| {
                        let base = CVPixelBufferGetBaseAddressOfPlane(&pb, i).cast::<u8>();
                        let bpr = CVPixelBufferGetBytesPerRowOfPlane(&pb, i);
                        let rows = CVPixelBufferGetHeightOfPlane(&pb, i);
                        (std::slice::from_raw_parts(base, bpr * rows), bpr as u32)
                    };
                    let ((y, yp), (uv, uvp)) = (plane(0), plane(1));
                    let p = self.planes.as_ref().unwrap();
                    self.gpu.queue.write_texture(
                        p.y.as_image_copy(),
                        y,
                        wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(yp),
                            rows_per_image: None,
                        },
                        p.y.size(),
                    );
                    self.gpu.queue.write_texture(
                        p.uv.as_image_copy(),
                        uv,
                        wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(uvp),
                            rows_per_image: None,
                        },
                        p.uv.size(),
                    );
                    CVPixelBufferUnlockBaseAddress(&pb, CVPixelBufferLockFlags::ReadOnly);
                }
                self.gpu
                    .queue
                    .submit([self.gpu.convert(self.bind.as_ref().unwrap(), w, h)]);
            }
        }
        Ok(())
    }

    fn check(&self) -> pattern::Reading {
        let (out, w, _) = self.out.as_ref().unwrap();
        let band = self.gpu.read_band(out, pattern::band_rows(*w));
        pattern::read(&band, *w)
    }

    fn describe(&self) -> serde_json::Value {
        let (w, h) = self.out.as_ref().map_or((0, 0), |(_, w, h)| (*w, *h));
        json!({
            "mode": if self.mode == Mode::Zero { "zero" } else { "cpu" },
            "size": [w, h], "fps": round1(self.reader.fps), "frames_in_file": self.reader.frame_count(),
            "decoder": "AVAssetReader (VideoToolbox)", "wgpu_adapter": self.gpu.adapter.name,
            "seek_reader": if self.reader.reuse { "one random-access reader" } else { "new reader per seek" },
        })
    }
}

fn open_player(args: &[String], usage: &str) -> Result<Player, String> {
    let path = args
        .first()
        .filter(|a| !a.starts_with('-'))
        .ok_or(usage.to_string())?;
    let mode = match flag(args, "--mode").unwrap_or("zero") {
        "zero" => Mode::Zero,
        "cpu" => Mode::Cpu,
        m => return Err(format!("--mode zero|cpu, а не «{m}»")),
    };
    Player::open(path, mode)
}

fn percentile(v: &mut [f64], p: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f64::total_cmp);
    v[((v.len() - 1) as f64 * p).round() as usize]
}

fn bench(args: &[String]) -> Result<(), String> {
    let mut p = open_player(
        args,
        "bench <файл> [--mode zero|cpu] [--paced] [--verify] [--frames N]",
    )?;
    let paced = args.iter().any(|a| a == "--paced");
    let verify = args.iter().any(|a| a == "--verify");
    let limit: u32 = num(args, "--frames", u32::MAX)?;
    let period = Duration::from_secs_f64(1.0 / p.reader.fps);
    let (mut frames, mut wrong, mut damaged, mut patch_worst, mut late) =
        (0u32, 0u32, 0u32, 0u8, 0u32);
    let mut per_frame = Vec::new();
    let (t0, c0) = (Instant::now(), cpu_time());
    while frames < limit {
        let step = autoreleasepool(|_| -> Result<bool, String> {
            let tf = Instant::now();
            let Some((pts, s, pb)) = p.reader.next() else {
                return Ok(false);
            };
            p.show(s, pb)?;
            per_frame.push(tf.elapsed().as_secs_f64() * 1000.0);
            if verify {
                let r = p.check();
                match r.index {
                    Some(i) if i == p.reader.index_of(pts) => {}
                    Some(_) => wrong += 1,
                    None => damaged += 1,
                }
                patch_worst = patch_worst.max(r.patch_max_diff);
            }
            Ok(true)
        })?;
        if !step {
            break;
        }
        frames += 1;
        if paced {
            let due = t0 + period * frames;
            let now = Instant::now();
            if now < due {
                std::thread::sleep(due - now);
            } else if now - due > period {
                late += 1;
            }
        }
    }
    p.gpu.wait();
    let wall = t0.elapsed().as_secs_f64();
    let cpu = (cpu_time() - c0).as_secs_f64();
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get()) as f64;
    let mut out = p.describe();
    let o = out.as_object_mut().unwrap();
    o.insert("paced".into(), json!(paced));
    o.insert("frames".into(), json!(frames));
    o.insert("seconds".into(), json!(round1(wall)));
    o.insert(
        "fps_achieved".into(),
        json!(round1(f64::from(frames) / wall)),
    );
    o.insert(
        "frame_ms_p50".into(),
        json!(round1(percentile(&mut per_frame.clone(), 0.5))),
    );
    o.insert(
        "frame_ms_p95".into(),
        json!(round1(percentile(&mut per_frame, 0.95))),
    );
    o.insert(
        "cpu_percent_of_machine".into(),
        json!(round1(cpu / wall / cores * 100.0)),
    );
    o.insert(
        "cpu_percent_of_one_core".into(),
        json!(round1(cpu / wall * 100.0)),
    );
    o.insert("logical_cores".into(), json!(cores));
    if paced {
        o.insert("frames_late_over_one_period".into(), json!(late));
    }
    if verify {
        o.insert("verify".into(), json!({"wrong_index": wrong, "damaged_barcode": damaged, "patch_max_diff": patch_worst}));
    }
    println!("{out}");
    Ok(())
}

fn seek(args: &[String]) -> Result<(), String> {
    let mut p = open_player(
        args,
        "seek <файл> [--mode zero|cpu] [--count N] [--gop N] [--seed N] [--new-reader] [--rows]",
    )?;
    // One random-access reader is the default (measured faster); --new-reader for comparison.
    p.reader.reuse = !args.iter().any(|a| a == "--new-reader");
    let total = p.reader.frame_count().max(1);
    let gop: u32 = num(args, "--gop", p.reader.fps.round() as u32)?;
    let count: u32 = num(args, "--count", 40)?;
    let mut rng: u64 = num(args, "--seed", 7)?;
    let mut targets = Vec::new();
    for _ in 0..count {
        rng = rng
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        targets.push(((rng >> 33) % u64::from(total)) as u32);
    }
    targets.extend((1..=8).map(|k| k * gop - 1).filter(|&f| f < total));
    // Warm-up.
    let (_, s, pb, _) = p.reader.seek(0)?;
    p.show(s, pb)?;
    p.gpu.wait();
    let (mut ms_all, mut ms_worst, mut rows) = (Vec::new(), Vec::new(), Vec::new());
    let (mut wrong, mut patch_worst) = (0u32, 0u8);
    for (n, &t) in targets.iter().enumerate() {
        let (ms, decoded, r) =
            autoreleasepool(|_| -> Result<(f64, u32, pattern::Reading), String> {
                let t0 = Instant::now();
                let (_, s, pb, decoded) = p.reader.seek(t)?;
                p.show(s, pb)?;
                p.gpu.wait();
                let ms = t0.elapsed().as_secs_f64() * 1000.0;
                Ok((ms, decoded, p.check()))
            })?;
        if r.index != Some(t) {
            wrong += 1;
        }
        patch_worst = patch_worst.max(r.patch_max_diff);
        let gop_end = n >= count as usize;
        if gop_end {
            ms_worst.push(ms);
        }
        ms_all.push(ms);
        rows.push(json!({"frame": t, "got": r.index, "decoded": decoded, "ms": round1(ms), "gop_end": gop_end}));
    }
    let mut out = p.describe();
    let o = out.as_object_mut().unwrap();
    o.insert("gop".into(), json!(gop));
    o.insert("seeks".into(), json!(targets.len()));
    o.insert("wrong_frame".into(), json!(wrong));
    o.insert("patch_max_diff".into(), json!(patch_worst));
    o.insert(
        "ms_p50".into(),
        json!(round1(percentile(&mut ms_all.clone(), 0.5))),
    );
    o.insert(
        "ms_p95".into(),
        json!(round1(percentile(&mut ms_all.clone(), 0.95))),
    );
    o.insert("ms_max".into(), json!(round1(percentile(&mut ms_all, 1.0))));
    o.insert(
        "ms_max_gop_end".into(),
        json!(round1(percentile(&mut ms_worst, 1.0))),
    );
    if args.iter().any(|a| a == "--rows") {
        o.insert("rows".into(), json!(rows));
    }
    println!("{out}");
    Ok(())
}
