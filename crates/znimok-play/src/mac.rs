//! macOS: AVAssetReader decodes (VideoToolbox, hardware) into NV12 `CVPixelBuffer`s backed by
//! IOSurface; `CVMetalTextureCache` wraps each plane as an `MTLTexture` on the UI's own
//! `MTLDevice` (no copy) and `hal::metal::Device::texture_from_raw` hands them to wgpu — the same
//! `nv12.wgsl` as on Windows reads them (P4, ZK-17). Fallback: the planes locked and uploaded.
//!
//! AVAssetReader cannot seek: a new reader starts a tenth of a frame inside the wanted one
//! (starting a quarter frame early gave frame t−1 stamped as t), VideoToolbox decodes from the
//! key frame before it, and the first frame out is the wanted one.

use std::ptr::NonNull;

use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::AnyObject;
use objc2_av_foundation::{
    AVAssetReader, AVAssetReaderTrackOutput, AVAssetTrack, AVMediaTypeVideo, AVURLAsset,
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
use objc2_foundation::{NSDictionary, NSNumber, NSString, NSURL};
use objc2_metal::{MTLPixelFormat, MTLTextureType};
use wgpu::hal::api::Metal;

use crate::Decoder;
use crate::convert::{Converter, Gpu, Planes};

fn cm_time(seconds: f64) -> CMTime {
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

/// What must stay alive until the GPU has read a frame.
struct InFlight {
    _textures: (wgpu::Texture, wgpu::Texture),
    _cv: (CFRetained<CVMetalTexture>, CFRetained<CVMetalTexture>),
    _pb: CFRetained<CVImageBuffer>,
    done: wgpu::SubmissionIndex,
}

pub struct AvPlayer {
    gpu: Gpu,
    asset: Retained<AVURLAsset>,
    track: Retained<AVAssetTrack>,
    reader: Option<(Retained<AVAssetReader>, Retained<AVAssetReaderTrackOutput>)>,
    cache: Option<CFRetained<CVMetalTextureCache>>,
    fps: f64,
    width: u32,
    height: u32,
    current: Option<(Retained<CMSampleBuffer>, CFRetained<CVImageBuffer>)>,
    in_flight: Vec<InFlight>,
    planes: Option<Planes>,
    path: &'static str,
}

impl AvPlayer {
    pub fn open(gpu: &Gpu, path: &std::path::Path) -> Result<Self, String> {
        let abs = std::path::absolute(path).map_err(|e| e.to_string())?;
        let url = NSURL::fileURLWithPath(&NSString::from_str(&abs.to_string_lossy()));
        // SAFETY: plain AVFoundation calls on objects we own.
        let (asset, track, fps, size) = unsafe {
            let asset = AVURLAsset::URLAssetWithURL_options(&url, None);
            #[allow(deprecated)]
            let tracks = asset.tracksWithMediaType(AVMediaTypeVideo.ok_or("AVMediaTypeVideo")?);
            let track = tracks.firstObject().ok_or("no video track")?;
            let fps = f64::from(track.nominalFrameRate());
            let s = track.naturalSize();
            (asset, track, fps, (s.width.round() as u32, s.height.round() as u32))
        };
        // The planes as Metal textures on the UI's own device; without it — the upload path.
        // SAFETY: only reads the device handle wgpu created.
        let cache = unsafe { gpu.device.as_hal::<Metal>() }.and_then(|hal| {
            let dev = hal.raw_device().clone();
            let mut cache: *mut CVMetalTextureCache = std::ptr::null_mut();
            // SAFETY: out-pointer to a local; the cache is created on wgpu's own MTLDevice.
            let rc = unsafe {
                CVMetalTextureCache::create(None, None, &dev, None, NonNull::from(&mut cache))
            };
            // SAFETY: Create rule — we own the reference.
            NonNull::new(cache)
                .filter(|_| rc == 0)
                .map(|c| unsafe { CFRetained::from_raw(c) })
        });
        let path = if cache.is_some() { "gpu" } else { "upload" };
        let mut me = Self {
            gpu: gpu.clone(),
            asset,
            track,
            reader: None,
            cache,
            fps: if fps > 0.0 { fps } else { 30.0 },
            width: size.0,
            height: size.1,
            current: None,
            in_flight: Vec::new(),
            planes: None,
            path,
        };
        me.start(0.0)?;
        Ok(me)
    }

    /// A fresh reader from `from` seconds.
    fn start(&mut self, from: f64) -> Result<(), String> {
        // SAFETY: AVFoundation calls on objects we own.
        unsafe {
            if let Some((r, _)) = self.reader.take() {
                r.cancelReading();
            }
            let reader = AVAssetReader::assetReaderWithAsset_error(&self.asset)
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
                &self.track,
                Some(dict),
            );
            output.setAlwaysCopiesSampleData(false);
            if from > 0.0 {
                reader.setTimeRange(CMTimeRange {
                    start: cm_time(from),
                    duration: kCMTimePositiveInfinity,
                });
            }
            reader.addOutput(&output);
            if !reader.startReading() {
                return Err("AVAssetReader: startReading failed".into());
            }
            self.reader = Some((reader, output));
        }
        Ok(())
    }

    fn plane_texture(
        &self,
        pb: &CVImageBuffer,
        plane: usize,
        format: MTLPixelFormat,
        wf: wgpu::TextureFormat,
    ) -> Result<(wgpu::Texture, CFRetained<CVMetalTexture>), String> {
        let cache = self.cache.as_ref().ok_or("no texture cache")?;
        let (w, h) = (
            CVPixelBufferGetWidthOfPlane(pb, plane),
            CVPixelBufferGetHeightOfPlane(pb, plane),
        );
        let mut t: *mut CVMetalTexture = std::ptr::null_mut();
        // SAFETY: the pixel buffer is IOSurface-backed (asked for in the reader's settings).
        let rc = unsafe {
            CVMetalTextureCache::create_texture_from_image(
                None,
                cache,
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
                    label: Some("znimok cv plane"),
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

    fn convert_zero(
        &mut self,
        conv: &Converter,
        out: &wgpu::Texture,
        thumb: bool,
    ) -> Result<wgpu::SubmissionIndex, String> {
        let pb = self.current.as_ref().ok_or("no frame")?.1.clone();
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
        let done = conv.run(
            &ty.create_view(&Default::default()),
            &tuv.create_view(&Default::default()),
            self.width,
            self.height,
            out,
            thumb,
        );
        self.in_flight.push(InFlight {
            _textures: (ty, tuv),
            _cv: (cy, cuv),
            _pb: pb,
            done: done.clone(),
        });
        // Three frames in flight at most; then the oldest is waited for and released.
        while self.in_flight.len() > 3 {
            let f = self.in_flight.remove(0);
            conv.wait(&f.done);
        }
        if let Some(c) = &self.cache {
            c.flush(0);
        }
        Ok(done)
    }

    fn convert_upload(
        &mut self,
        conv: &Converter,
        out: &wgpu::Texture,
        thumb: bool,
    ) -> Result<wgpu::SubmissionIndex, String> {
        let pb = self.current.as_ref().ok_or("no frame")?.1.clone();
        if self.planes.is_none() {
            self.planes = Some(conv.planes(self.width, self.height));
        }
        let p = self.planes.as_ref().unwrap();
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
            conv.upload(p, y, yp, uv, uvp);
            CVPixelBufferUnlockBaseAddress(&pb, CVPixelBufferLockFlags::ReadOnly);
        }
        Ok(conv.run(
            &p.y.create_view(&Default::default()),
            &p.uv.create_view(&Default::default()),
            self.width,
            self.height,
            out,
            thumb,
        ))
    }
}

impl Decoder for AvPlayer {
    fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    fn fps(&self) -> f64 {
        self.fps
    }

    fn path(&self) -> &'static str {
        self.path
    }

    fn seek(&mut self, frame: i64) -> Result<(), String> {
        self.current = None;
        self.start((frame.max(0) as f64 + 0.1) / self.fps)
    }

    fn next(&mut self) -> Result<Option<(i64, i64)>, String> {
        let Some((_, output)) = &self.reader else {
            return Ok(None);
        };
        autoreleasepool(|_| {
            loop {
                // SAFETY: the output belongs to a started reader.
                let Some(s) = (unsafe { output.copyNextSampleBuffer() }) else {
                    self.current = None;
                    return Ok(None);
                };
                // SAFETY: reads of a live sample buffer.
                let Some(pb) = (unsafe { s.image_buffer() }) else {
                    continue;
                };
                let pts = cm_seconds(unsafe { s.presentation_time_stamp() });
                let dur = cm_seconds(unsafe { s.duration() });
                let a = (pts * self.fps).round() as i64;
                let n = ((dur * self.fps).round() as i64).max(1);
                let (w, h) = (
                    CVPixelBufferGetWidthOfPlane(&pb, 0) as u32,
                    CVPixelBufferGetHeightOfPlane(&pb, 0) as u32,
                );
                if (w, h) != (self.width, self.height) {
                    self.width = w;
                    self.height = h;
                    self.planes = None;
                }
                self.current = Some((s, pb));
                return Ok(Some((a, n)));
            }
        })
    }

    fn convert(
        &mut self,
        conv: &Converter,
        out: &wgpu::Texture,
        thumb: bool,
    ) -> Result<wgpu::SubmissionIndex, String> {
        if self.cache.is_some() {
            match self.convert_zero(conv, out, thumb) {
                Ok(d) => return Ok(d),
                Err(e) => {
                    eprintln!("player: {e}; uploading the planes from now on");
                    self.cache = None;
                    self.path = "upload";
                }
            }
        }
        self.convert_upload(conv, out, thumb)
    }
}
