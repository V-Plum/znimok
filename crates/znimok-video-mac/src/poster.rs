//! The first frame of an MP4 as RGBA (a recording's poster, ZK-88): AVAssetReader decodes it as
//! BGRA, without a GPU device.

use std::path::Path;
use std::ptr::NonNull;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_av_foundation::{AVAssetReader, AVAssetReaderTrackOutput, AVMediaTypeVideo, AVURLAsset};
use objc2_core_foundation::CFString;
use objc2_core_video::{
    CVPixelBuffer, CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow,
    CVPixelBufferGetHeight, CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress,
    CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress, kCVPixelBufferPixelFormatTypeKey,
    kCVPixelFormatType_32BGRA,
};
use objc2_foundation::{NSDictionary, NSNumber, NSString, NSURL};

/// `(width, height, rgba)` of the first video frame of `path` (an `.mp4`: AVFoundation goes by
/// the extension).
pub fn first_frame(path: &Path) -> Result<(u32, u32, Vec<u8>), String> {
    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    // SAFETY: a plain constructor for a file URL.
    let asset = unsafe { AVURLAsset::URLAssetWithURL_options(&url, None) };
    // SAFETY: extern static of AVFoundation.
    let vtype = unsafe { AVMediaTypeVideo }.ok_or("AVFoundation: no video media type")?;
    // SAFETY: the synchronous track list, fine for a local file (as znimok-play reads it).
    #[allow(deprecated)]
    let tracks = unsafe { asset.tracksWithMediaType(vtype) };
    let track = tracks.firstObject().ok_or("the file has no video")?;
    // SAFETY: a reader for a local asset.
    let reader = unsafe { AVAssetReader::assetReaderWithAsset_error(&asset) }
        .map_err(|e| format!("AVAssetReader: {}", e.localizedDescription()))?;
    // SAFETY: CoreVideo's CFString key, toll-free bridged to NSString.
    let key = unsafe { &*(kCVPixelBufferPixelFormatTypeKey as *const CFString).cast::<NSString>() };
    let bgra = NSNumber::new_u32(kCVPixelFormatType_32BGRA);
    let v: &AnyObject = &bgra;
    let settings: Retained<NSDictionary<NSString, AnyObject>> =
        NSDictionary::from_slices(&[key], &[v]);
    // SAFETY: the track of this asset and BGRA output settings.
    let output = unsafe {
        AVAssetReaderTrackOutput::assetReaderTrackOutputWithTrack_outputSettings(
            &track,
            Some(&settings),
        )
    };
    // SAFETY: the output belongs to this reader's asset; reading starts once.
    unsafe {
        if !reader.canAddOutput(&output) {
            return Err("AVAssetReader: the output is refused".into());
        }
        reader.addOutput(&output);
        if !reader.startReading() {
            return Err("AVAssetReader: cannot start".into());
        }
    }
    // SAFETY: samples are read from a started reader; the pixel buffer is locked while copied.
    unsafe {
        let pb = loop {
            let Some(s) = output.copyNextSampleBuffer() else {
                return Err("no frames".into());
            };
            if let Some(pb) = s.image_buffer() {
                break pb;
            }
        };
        let pb: &CVPixelBuffer = &pb;
        if CVPixelBufferLockBaseAddress(pb, CVPixelBufferLockFlags::ReadOnly) != 0 {
            return Err("CVPixelBufferLockBaseAddress".into());
        }
        let (w, h) = (CVPixelBufferGetWidth(pb), CVPixelBufferGetHeight(pb));
        let stride = CVPixelBufferGetBytesPerRow(pb);
        let base = NonNull::new(CVPixelBufferGetBaseAddress(pb).cast::<u8>());
        let out = base.map(|b| {
            let mut out = Vec::with_capacity(w * h * 4);
            for y in 0..h {
                let row = std::slice::from_raw_parts(b.as_ptr().add(y * stride), w * 4);
                for p in row.as_chunks::<4>().0 {
                    out.extend_from_slice(&[p[2], p[1], p[0], 255]);
                }
            }
            out
        });
        CVPixelBufferUnlockBaseAddress(pb, CVPixelBufferLockFlags::ReadOnly);
        reader.cancelReading();
        let out = out.ok_or("CVPixelBuffer: no memory")?;
        Ok((w as u32, h as u32, out))
    }
}
