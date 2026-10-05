//! AVAssetWriter: H.264 by VideoToolbox (hardware on every Apple Silicon and recent Intel Mac)
//! and AAC, muxed into an MP4. Used by the recording ([`crate::sink::AvSink`], frames straight
//! from ScreenCaptureKit) and by the export (ZK-205, frames made from memory with
//! [`AvWriter::pixel_buffer_from_rgba`]).
//!
//! Settings follow the Windows encoder (ZK-87): no B-frames (frame reordering off — LH's lesson:
//! the software encoder's composition offset shifts the video by a frame), a key frame every
//! ~0.25 s for seeking (ZK-17), BT.709 limited range stated explicitly. AVAssetWriter places
//! samples by their timestamps and ignores durations, so the recorder repeats a stretched frame
//! (`SinkCaps::carries_duration = false`, ZK-86).

use std::path::Path;
use std::ptr::NonNull;
use std::sync::mpsc;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_av_foundation::{
    AVAssetWriter, AVAssetWriterInput, AVAssetWriterInputPixelBufferAdaptor, AVAssetWriterStatus,
    AVFileTypeMPEG4, AVMediaTypeAudio, AVMediaTypeVideo, AVVideoAllowFrameReorderingKey,
    AVVideoAverageBitRateKey, AVVideoCodecKey, AVVideoCodecTypeH264,
    AVVideoColorPrimaries_ITU_R_709_2, AVVideoColorPrimariesKey, AVVideoColorPropertiesKey,
    AVVideoCompressionPropertiesKey, AVVideoExpectedSourceFrameRateKey, AVVideoHeightKey,
    AVVideoMaxKeyFrameIntervalKey, AVVideoProfileLevelH264HighAutoLevel, AVVideoProfileLevelKey,
    AVVideoTransferFunction_ITU_R_709_2, AVVideoTransferFunctionKey, AVVideoWidthKey,
    AVVideoYCbCrMatrix_ITU_R_709_2, AVVideoYCbCrMatrixKey,
};
use objc2_avf_audio::{AVEncoderBitRateKey, AVFormatIDKey, AVNumberOfChannelsKey, AVSampleRateKey};
use objc2_core_audio_types::{
    AudioStreamBasicDescription, kAudioFormatFlagIsPacked, kAudioFormatFlagIsSignedInteger,
    kAudioFormatLinearPCM, kAudioFormatMPEG4AAC,
};
use objc2_core_foundation::{CFDictionary, CFRetained, CFString};
use objc2_core_media::{
    CMAudioFormatDescriptionCreate, CMAudioSampleBufferCreateReadyWithPacketDescriptions,
    CMBlockBuffer, CMFormatDescription, CMSampleBuffer, CMTime,
};
use objc2_core_video::{
    CVPixelBuffer, CVPixelBufferCreate, CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow,
    CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags, CVPixelBufferPool,
    CVPixelBufferUnlockBaseAddress, kCVPixelBufferHeightKey, kCVPixelBufferIOSurfacePropertiesKey,
    kCVPixelBufferPixelFormatTypeKey, kCVPixelBufferWidthKey, kCVPixelFormatType_32BGRA,
};
use objc2_foundation::{NSDictionary, NSNumber, NSString, NSURL};

/// A frame for the writer: a retained `CVPixelBuffer` (BGRA; from ScreenCaptureKit an IOSurface,
/// from the export a buffer of the writer's own pool).
pub struct PixelBuf(pub CFRetained<CVPixelBuffer>);

// SAFETY: a CVPixelBuffer is a reference-counted CoreFoundation object; CoreVideo allows using it
// from any thread (it is handed between ScreenCaptureKit's queue and the recording thread).
unsafe impl Send for PixelBuf {}
// SAFETY: as above — only read through CoreVideo calls that are thread-safe.
unsafe impl Sync for PixelBuf {}

impl PixelBuf {
    /// A buffer ScreenCaptureKit (or anyone) hands over as a raw `CVPixelBufferRef`: retained here.
    ///
    /// # Safety
    /// `ptr` must be a valid `CVPixelBufferRef` for the duration of the call.
    pub unsafe fn retain_raw(ptr: *mut std::ffi::c_void) -> Option<Self> {
        let p = NonNull::new(ptr.cast::<CVPixelBuffer>())?;
        // SAFETY: the caller says it is a live CVPixelBuffer; retaining keeps it alive.
        Some(Self(unsafe { CFRetained::retain(p) }))
    }
}

/// Fills a BGRA buffer (at least `w`×`h`) with `rgba` (`w`×`h`, tightly packed).
fn fill_bgra(pb: &CVPixelBuffer, w: usize, h: usize, rgba: &[u8]) -> Result<(), String> {
    if rgba.len() < w * h * 4 {
        return Err("the frame is smaller than the video".into());
    }
    // SAFETY: the buffer is locked while its memory is written, each row within its stride.
    unsafe {
        CVPixelBufferLockBaseAddress(pb, CVPixelBufferLockFlags(0));
        let base = CVPixelBufferGetBaseAddress(pb).cast::<u8>();
        let stride = CVPixelBufferGetBytesPerRow(pb);
        if base.is_null() || stride < w * 4 {
            CVPixelBufferUnlockBaseAddress(pb, CVPixelBufferLockFlags(0));
            return Err("CVPixelBuffer: no memory".into());
        }
        for y in 0..h {
            let src = &rgba[y * w * 4..(y + 1) * w * 4];
            let dst = std::slice::from_raw_parts_mut(base.add(y * stride), w * 4);
            for (d, s) in dst
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(src.as_chunks::<4>().0)
            {
                d[0] = s[2];
                d[1] = s[1];
                d[2] = s[0];
                d[3] = 255;
            }
        }
        CVPixelBufferUnlockBaseAddress(pb, CVPixelBufferLockFlags(0));
    }
    Ok(())
}

/// A BGRA buffer of its own (IOSurface-backed, as ScreenCaptureKit's are) of any size with
/// `rgba` in it: what the screen hands over, for tests and tools.
pub fn bgra_buffer(width: u32, height: u32, rgba: &[u8]) -> Result<PixelBuf, String> {
    let empty: Retained<NSDictionary<NSString, AnyObject>> = NSDictionary::new();
    // SAFETY: CoreVideo's extern CFString key.
    let attrs = unsafe { dict(&[(cf_key(kCVPixelBufferIOSurfacePropertiesKey), &empty)]) };
    let mut out: *mut CVPixelBuffer = std::ptr::null_mut();
    // SAFETY: NSDictionary is toll-free bridged to CFDictionary; a valid out-pointer.
    let st = unsafe {
        CVPixelBufferCreate(
            None,
            width as usize,
            height as usize,
            kCVPixelFormatType_32BGRA,
            Some(&*(Retained::as_ptr(&attrs).cast::<CFDictionary>())),
            NonNull::from(&mut out),
        )
    };
    let pb = NonNull::new(out)
        .filter(|_| st == 0)
        // SAFETY: a +1 object from a Create function.
        .map(|p| unsafe { CFRetained::from_raw(p) })
        .ok_or_else(|| format!("CVPixelBufferCreate: {st}"))?;
    fill_bgra(&pb, width as usize, height as usize, rgba)?;
    Ok(PixelBuf(pb))
}

/// Seconds as a `CMTime` of `timescale`.
fn cm_time(value: i64, timescale: i32) -> CMTime {
    // SAFETY: a plain value constructor.
    unsafe { CMTime::new(value, timescale) }
}

/// `CFString` keys of CoreVideo as the `NSString` keys of a dictionary (toll-free bridged).
fn cf_key(k: &CFString) -> &NSString {
    // SAFETY: CFString and NSString are toll-free bridged; the reference keeps its lifetime.
    unsafe { &*(k as *const CFString).cast::<NSString>() }
}

fn key(k: Option<&'static NSString>) -> Result<&'static NSString, String> {
    k.ok_or_else(|| "AVFoundation: a settings key is missing".to_string())
}

fn dict(pairs: &[(&NSString, &AnyObject)]) -> Retained<NSDictionary<NSString, AnyObject>> {
    let keys: Vec<&NSString> = pairs.iter().map(|(k, _)| *k).collect();
    let values: Vec<&AnyObject> = pairs.iter().map(|(_, v)| *v).collect();
    NSDictionary::from_slices(&keys, &values)
}

fn ns_err(e: &objc2_foundation::NSError) -> String {
    e.localizedDescription().to_string()
}

/// What the file is made with.
#[derive(Clone, Debug, PartialEq)]
pub struct WriterConfig {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate: u32,
    pub keyframe_interval: u32,
    /// AAC tracks (48 kHz stereo).
    pub audio_tracks: usize,
    pub audio_bitrate: u32,
    /// Frames come as they happen (a recording): the writer does not wait for each.
    pub real_time: bool,
}

pub struct AvWriter {
    writer: Retained<AVAssetWriter>,
    video: Retained<AVAssetWriterInput>,
    adaptor: Retained<AVAssetWriterInputPixelBufferAdaptor>,
    audio: Vec<Retained<AVAssetWriterInput>>,
    pcm_format: CFRetained<CMFormatDescription>,
    fps: i32,
    width: u32,
    height: u32,
    finished: bool,
    audio_ended: bool,
}

// SAFETY: AVAssetWriter and its inputs are used from one thread at a time (the recording or
// export thread that owns the writer); they are created there or handed over before use.
unsafe impl Send for AvWriter {}

impl AvWriter {
    /// A writer into `path` (MP4 whatever the extension; the file must not exist).
    pub fn create(path: &Path, cfg: &WriterConfig) -> Result<Self, String> {
        let _ = std::fs::remove_file(path);
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
        // SAFETY: extern statics of AVFoundation, read once.
        let (mp4, vtype, atype) = unsafe { (AVFileTypeMPEG4, AVMediaTypeVideo, AVMediaTypeAudio) };
        let (mp4, vtype, atype) = (key(mp4)?, key(vtype)?, key(atype)?);
        // SAFETY: a file URL and a container type AVFoundation writes.
        let writer = unsafe { AVAssetWriter::assetWriterWithURL_fileType_error(&url, mp4) }
            .map_err(|e| format!("AVAssetWriter: {}", ns_err(&e)))?;

        // Video: H.264 High, no reordering, the key frame interval, BT.709.
        let n = |v: u32| NSNumber::new_u32(v);
        let (bitrate, gop, fps) = (n(cfg.bitrate), n(cfg.keyframe_interval.max(1)), n(cfg.fps));
        let no = NSNumber::new_bool(false);
        // SAFETY: extern statics of AVFoundation.
        let s = unsafe {
            (
                key(AVVideoAverageBitRateKey)?,
                key(AVVideoMaxKeyFrameIntervalKey)?,
                key(AVVideoAllowFrameReorderingKey)?,
                key(AVVideoProfileLevelKey)?,
                key(AVVideoProfileLevelH264HighAutoLevel)?,
                key(AVVideoExpectedSourceFrameRateKey)?,
            )
        };
        let compression = dict(&[
            (s.0, &bitrate),
            (s.1, &gop),
            (s.2, &no),
            (s.3, s.4),
            (s.5, &fps),
        ]);
        // SAFETY: extern statics of AVFoundation.
        let c = unsafe {
            (
                key(AVVideoColorPrimariesKey)?,
                key(AVVideoColorPrimaries_ITU_R_709_2)?,
                key(AVVideoTransferFunctionKey)?,
                key(AVVideoTransferFunction_ITU_R_709_2)?,
                key(AVVideoYCbCrMatrixKey)?,
                key(AVVideoYCbCrMatrix_ITU_R_709_2)?,
            )
        };
        let colour = dict(&[(c.0, c.1), (c.2, c.3), (c.4, c.5)]);
        let (w, h) = (n(cfg.width), n(cfg.height));
        // SAFETY: extern statics of AVFoundation.
        let v = unsafe {
            (
                key(AVVideoCodecKey)?,
                key(AVVideoCodecTypeH264)?,
                key(AVVideoWidthKey)?,
                key(AVVideoHeightKey)?,
                key(AVVideoCompressionPropertiesKey)?,
                key(AVVideoColorPropertiesKey)?,
            )
        };
        let settings = dict(&[
            (v.0, v.1),
            (v.2, &w),
            (v.3, &h),
            (v.4, &compression),
            (v.5, &colour),
        ]);
        // SAFETY: a media type and output settings of the documented keys.
        let video = unsafe {
            AVAssetWriterInput::assetWriterInputWithMediaType_outputSettings(vtype, Some(&settings))
        };
        // SAFETY: plain setter.
        unsafe { video.setExpectsMediaDataInRealTime(cfg.real_time) };
        // The adaptor's pool: BGRA, IOSurface-backed, the output's size (for the export).
        let bgra = n(kCVPixelFormatType_32BGRA);
        let empty: Retained<NSDictionary<NSString, AnyObject>> = NSDictionary::new();
        // SAFETY: CoreVideo's extern CFString keys.
        let attrs = unsafe {
            dict(&[
                (cf_key(kCVPixelBufferPixelFormatTypeKey), &bgra),
                (cf_key(kCVPixelBufferWidthKey), &w),
                (cf_key(kCVPixelBufferHeightKey), &h),
                (cf_key(kCVPixelBufferIOSurfacePropertiesKey), &empty),
            ])
        };
        // SAFETY: the input above and pixel buffer attributes of CoreVideo's keys.
        let adaptor = unsafe {
            AVAssetWriterInputPixelBufferAdaptor::assetWriterInputPixelBufferAdaptorWithAssetWriterInput_sourcePixelBufferAttributes(
                &video,
                Some(&attrs),
            )
        };
        // SAFETY: inputs are added before writing starts.
        unsafe {
            if !writer.canAddInput(&video) {
                return Err("AVAssetWriter: the video input is refused".into());
            }
            writer.addInput(&video);
        }

        // Sound: AAC-LC 48 kHz stereo per track.
        let mut audio = Vec::new();
        for _ in 0..cfg.audio_tracks {
            let (aac, rate, ch, br) = (
                n(kAudioFormatMPEG4AAC),
                NSNumber::new_f64(48_000.0),
                n(2),
                n(cfg.audio_bitrate),
            );
            // SAFETY: extern statics of AVFAudio.
            let k = unsafe {
                (
                    key(AVFormatIDKey)?,
                    key(AVSampleRateKey)?,
                    key(AVNumberOfChannelsKey)?,
                    key(AVEncoderBitRateKey)?,
                )
            };
            let s = dict(&[(k.0, &aac), (k.1, &rate), (k.2, &ch), (k.3, &br)]);
            // SAFETY: a media type and AAC settings of the documented keys.
            let input = unsafe {
                AVAssetWriterInput::assetWriterInputWithMediaType_outputSettings(atype, Some(&s))
            };
            // SAFETY: plain setter; inputs are added before writing starts.
            unsafe {
                input.setExpectsMediaDataInRealTime(cfg.real_time);
                if !writer.canAddInput(&input) {
                    return Err("AVAssetWriter: an audio input is refused".into());
                }
                writer.addInput(&input);
            }
            audio.push(input);
        }

        // The format of the PCM the recorder hands over: s16 interleaved stereo, 48 kHz.
        let asbd = AudioStreamBasicDescription {
            mSampleRate: 48_000.0,
            mFormatID: kAudioFormatLinearPCM,
            mFormatFlags: kAudioFormatFlagIsSignedInteger | kAudioFormatFlagIsPacked,
            mBytesPerPacket: 4,
            mFramesPerPacket: 1,
            mBytesPerFrame: 4,
            mChannelsPerFrame: 2,
            mBitsPerChannel: 16,
            mReserved: 0,
        };
        let mut fmt: *const objc2_core_media::CMAudioFormatDescription = std::ptr::null();
        // SAFETY: a valid ASBD and out-pointer; no layout, no cookie, no extensions.
        let st = unsafe {
            CMAudioFormatDescriptionCreate(
                None,
                NonNull::from(&asbd),
                0,
                std::ptr::null(),
                0,
                std::ptr::null(),
                None,
                NonNull::from(&mut fmt),
            )
        };
        let pcm_format = NonNull::new(fmt.cast_mut().cast::<CMFormatDescription>())
            .filter(|_| st == 0)
            // SAFETY: a +1 object from a Create function.
            .map(|p| unsafe { CFRetained::from_raw(p) })
            .ok_or_else(|| format!("CMAudioFormatDescriptionCreate: {st}"))?;

        // SAFETY: all inputs are added; the session starts at zero (slot times are from zero).
        unsafe {
            if !writer.startWriting() {
                return Err(format!(
                    "AVAssetWriter: {}",
                    writer.error().map(|e| ns_err(&e)).unwrap_or_default()
                ));
            }
            writer.startSessionAtSourceTime(cm_time(0, 1));
        }
        Ok(Self {
            writer,
            video,
            adaptor,
            audio,
            pcm_format,
            fps: cfg.fps.max(1) as i32,
            width: cfg.width,
            height: cfg.height,
            finished: false,
            audio_ended: false,
        })
    }

    fn failure(&self) -> String {
        // SAFETY: plain getters.
        unsafe {
            match self.writer.error() {
                Some(e) => ns_err(&e),
                None => format!("status {:?}", self.writer.status()),
            }
        }
    }

    /// An input that takes nothing: busy (`Ok(false)`), or the writer has failed (its error).
    fn busy_or_failed(&self) -> Result<bool, String> {
        // SAFETY: plain getter.
        match unsafe { self.writer.status() } {
            AVAssetWriterStatus::Failed | AVAssetWriterStatus::Cancelled => {
                Err(format!("AVAssetWriter: {}", self.failure()))
            }
            _ => Ok(false),
        }
    }

    /// The video input takes more now (a real-time input says no while the encoder is behind).
    pub fn video_ready(&self) -> bool {
        // SAFETY: plain getter.
        unsafe { self.video.isReadyForMoreMediaData() }
    }

    /// Appends a frame at slot `slot` (time `slot / fps`). `Ok(false)`: the encoder is busy.
    pub fn append_frame(&mut self, frame: &CVPixelBuffer, slot: i64) -> Result<bool, String> {
        if !self.video_ready() {
            return self.busy_or_failed();
        }
        // SAFETY: a live pixel buffer of the writer's size; times only grow (the recorder's slots).
        let ok = unsafe {
            self.adaptor
                .appendPixelBuffer_withPresentationTime(frame, cm_time(slot, self.fps))
        };
        if ok {
            Ok(true)
        } else {
            Err(format!("AVAssetWriter: {}", self.failure()))
        }
    }

    /// Appends PCM s16 stereo of track `track` starting at audio frame `index` (48 kHz).
    /// `Ok(false)`: the encoder is busy.
    pub fn append_pcm(&mut self, track: usize, pcm: &[i16], index: i64) -> Result<bool, String> {
        let Some(input) = self.audio.get(track) else {
            return Ok(true);
        };
        if pcm.len() < 2 {
            return Ok(true);
        }
        // SAFETY: plain getter.
        if !unsafe { input.isReadyForMoreMediaData() } {
            return self.busy_or_failed();
        }
        let bytes = std::mem::size_of_val(pcm);
        let mut block: *mut CMBlockBuffer = std::ptr::null_mut();
        // SAFETY: CoreMedia allocates a block of `bytes`; our bytes are copied in right after.
        let block = unsafe {
            let st = CMBlockBuffer::create_with_memory_block(
                None,
                std::ptr::null_mut(),
                bytes,
                None,
                std::ptr::null(),
                0,
                bytes,
                0,
                NonNull::from(&mut block),
            );
            let b = NonNull::new(block)
                .filter(|_| st == 0)
                .map(|p| CFRetained::from_raw(p))
                .ok_or_else(|| format!("CMBlockBufferCreateWithMemoryBlock: {st}"))?;
            let st = CMBlockBuffer::replace_data_bytes(
                NonNull::new(pcm.as_ptr().cast_mut().cast()).ok_or("empty PCM")?,
                &b,
                0,
                bytes,
            );
            if st != 0 {
                return Err(format!("CMBlockBufferReplaceDataBytes: {st}"));
            }
            b
        };
        let frames = (pcm.len() / 2) as isize;
        let mut sample: *mut CMSampleBuffer = std::ptr::null_mut();
        // SAFETY: the block holds `frames` packed s16 stereo frames of `pcm_format`.
        let sample = unsafe {
            let st = CMAudioSampleBufferCreateReadyWithPacketDescriptions(
                None,
                &block,
                &self.pcm_format,
                frames,
                cm_time(index, 48_000),
                std::ptr::null(),
                NonNull::from(&mut sample),
            );
            NonNull::new(sample)
                .filter(|_| st == 0)
                .map(|p| CFRetained::from_raw(p))
                .ok_or_else(|| format!("CMAudioSampleBufferCreateReady: {st}"))?
        };
        // SAFETY: a ready audio sample buffer of the format the input converts from.
        if unsafe { input.appendSampleBuffer(&sample) } {
            Ok(true)
        } else {
            Err(format!("AVAssetWriter (sound): {}", self.failure()))
        }
    }

    /// A BGRA buffer of the writer's pool with `rgba` (`width`×`height`, tightly packed) in it.
    pub fn pixel_buffer_from_rgba(&self, rgba: &[u8]) -> Result<PixelBuf, String> {
        let (w, h) = (self.width as usize, self.height as usize);
        if rgba.len() < w * h * 4 {
            return Err("the frame is smaller than the video".into());
        }
        // SAFETY: plain getter; the pool exists once writing has started.
        let pool: Retained<CVPixelBufferPool> = unsafe { self.adaptor.pixelBufferPool() }
            .ok_or("AVAssetWriter: no pixel buffer pool")?;
        let mut out: *mut CVPixelBuffer = std::ptr::null_mut();
        // SAFETY: a valid pool and out-pointer.
        let st =
            unsafe { CVPixelBufferPool::create_pixel_buffer(None, &pool, NonNull::from(&mut out)) };
        let pb = NonNull::new(out)
            .filter(|_| st == 0)
            // SAFETY: a +1 object from a Create function.
            .map(|p| unsafe { CFRetained::from_raw(p) })
            .ok_or_else(|| format!("CVPixelBufferPoolCreatePixelBuffer: {st}"))?;
        fill_bgra(&pb, w, h, rgba)?;
        Ok(PixelBuf(pb))
    }

    /// No more sound: the audio inputs are closed, so the writer stops holding the video back for
    /// them (it interleaves the tracks and waits for the slower one).
    pub fn end_audio(&mut self) {
        if self.audio_ended {
            return;
        }
        self.audio_ended = true;
        for a in &self.audio {
            // SAFETY: each input is marked finished once.
            unsafe { a.markAsFinished() };
        }
    }

    /// Finishes the file (`moov`), waiting for AVFoundation.
    pub fn finish(&mut self) -> Result<(), String> {
        if self.finished {
            return Ok(());
        }
        self.finished = true;
        // SAFETY: the inputs are marked finished once, then the writer finishes asynchronously;
        // the block only signals the channel.
        unsafe {
            self.video.markAsFinished();
            if !self.audio_ended {
                for a in &self.audio {
                    a.markAsFinished();
                }
            }
            let (tx, rx) = mpsc::channel::<()>();
            let tx = std::sync::Mutex::new(Some(tx));
            let block = block2::RcBlock::new(move || {
                if let Some(t) = tx.lock().ok().and_then(|mut t| t.take()) {
                    let _ = t.send(());
                }
            });
            self.writer.finishWritingWithCompletionHandler(&block);
            let _ = rx.recv_timeout(std::time::Duration::from_secs(30));
            if self.writer.status() == AVAssetWriterStatus::Completed {
                Ok(())
            } else {
                Err(format!("AVAssetWriter: {}", self.failure()))
            }
        }
    }
}

impl Drop for AvWriter {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}
