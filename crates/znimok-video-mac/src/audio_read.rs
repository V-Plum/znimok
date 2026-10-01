//! The sound tracks of an MP4 as PCM s16 stereo at 48 kHz (the export's mix, ZK-205): one
//! AVAssetReader per track, AAC decoded by AudioToolbox, blocks with their times.

use std::path::Path;
use std::ptr::NonNull;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_av_foundation::{
    AVAssetReader, AVAssetReaderTrackOutput, AVAssetTrack, AVMediaTypeAudio, AVURLAsset,
};
use objc2_avf_audio::{
    AVFormatIDKey, AVLinearPCMBitDepthKey, AVLinearPCMIsBigEndianKey, AVLinearPCMIsFloatKey,
    AVLinearPCMIsNonInterleaved, AVNumberOfChannelsKey, AVSampleRateKey,
};
use objc2_core_audio_types::kAudioFormatLinearPCM;
use objc2_foundation::{NSDictionary, NSNumber, NSString, NSURL};

pub const RATE: u32 = 48_000;
pub const CHANNELS: u32 = 2;

pub struct AudioTrackReader {
    reader: Retained<AVAssetReader>,
    output: Retained<AVAssetReaderTrackOutput>,
}

// SAFETY: the reader is used by the one export thread that owns it.
unsafe impl Send for AudioTrackReader {}

fn key(k: Option<&'static NSString>) -> Result<&'static NSString, String> {
    k.ok_or_else(|| "AVFAudio: a settings key is missing".to_string())
}

impl AudioTrackReader {
    /// A reader for each sound track of the file at `path` (an `.mp4`), in the file's order.
    pub fn open_all(path: &Path) -> Result<Vec<Self>, String> {
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
        // SAFETY: a plain constructor for a file URL.
        let asset = unsafe { AVURLAsset::URLAssetWithURL_options(&url, None) };
        // SAFETY: extern static of AVFoundation.
        let atype = unsafe { AVMediaTypeAudio }.ok_or("AVFoundation: no audio media type")?;
        // SAFETY: the synchronous track list, fine for a local file.
        #[allow(deprecated)]
        let tracks = unsafe { asset.tracksWithMediaType(atype) };
        let mut out = Vec::new();
        for i in 0..tracks.count() {
            let track = tracks.objectAtIndex(i);
            out.push(Self::open(&asset, &track)?);
        }
        Ok(out)
    }

    fn open(asset: &AVURLAsset, track: &AVAssetTrack) -> Result<Self, String> {
        // SAFETY: a reader for a local asset.
        let reader = unsafe { AVAssetReader::assetReaderWithAsset_error(asset) }
            .map_err(|e| format!("AVAssetReader: {}", e.localizedDescription()))?;
        let (lpcm, rate, ch, bits, no) = (
            NSNumber::new_u32(kAudioFormatLinearPCM),
            NSNumber::new_f64(f64::from(RATE)),
            NSNumber::new_u32(CHANNELS),
            NSNumber::new_u32(16),
            NSNumber::new_bool(false),
        );
        // SAFETY: extern statics of AVFAudio.
        let k = unsafe {
            [
                key(AVFormatIDKey)?,
                key(AVSampleRateKey)?,
                key(AVNumberOfChannelsKey)?,
                key(AVLinearPCMBitDepthKey)?,
                key(AVLinearPCMIsFloatKey)?,
                key(AVLinearPCMIsBigEndianKey)?,
                key(AVLinearPCMIsNonInterleaved)?,
            ]
        };
        let v: [&AnyObject; 7] = [&lpcm, &rate, &ch, &bits, &no, &no, &no];
        let settings: Retained<NSDictionary<NSString, AnyObject>> =
            NSDictionary::from_slices(&k, &v);
        // SAFETY: the track of this asset and LPCM output settings.
        let output = unsafe {
            AVAssetReaderTrackOutput::assetReaderTrackOutputWithTrack_outputSettings(
                track,
                Some(&settings),
            )
        };
        // SAFETY: the output belongs to this reader's asset; reading starts once.
        unsafe {
            if !reader.canAddOutput(&output) {
                return Err("AVAssetReader: the sound output is refused".into());
            }
            reader.addOutput(&output);
            if !reader.startReading() {
                return Err("AVAssetReader: cannot start".into());
            }
        }
        Ok(Self { reader, output })
    }

    /// The next block: its time (100 ns) and interleaved s16 stereo samples.
    pub fn next_block(&mut self) -> Result<Option<(i64, Vec<i16>)>, String> {
        // SAFETY: samples of a started reader; the block buffer is copied out whole.
        unsafe {
            let Some(s) = self.output.copyNextSampleBuffer() else {
                return Ok(None);
            };
            let t = s.presentation_time_stamp();
            let hns = if t.timescale > 0 {
                ((t.value as i128) * 10_000_000 / t.timescale as i128) as i64
            } else {
                0
            };
            let Some(block) = s.data_buffer() else {
                return Ok(Some((hns, Vec::new())));
            };
            let len = block.data_length();
            let mut pcm = vec![0i16; len / 2];
            if !pcm.is_empty() {
                let dst = NonNull::new(pcm.as_mut_ptr().cast()).ok_or("PCM")?;
                let st = block.copy_data_bytes(0, pcm.len() * 2, dst);
                if st != 0 {
                    return Err(format!("CMBlockBufferCopyDataBytes: {st}"));
                }
            }
            Ok(Some((hns, pcm)))
        }
    }
}

impl Drop for AudioTrackReader {
    fn drop(&mut self) {
        // SAFETY: cancelling a reader that may still be reading.
        unsafe { self.reader.cancelReading() };
    }
}
