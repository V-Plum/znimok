//! What an OS backend implements (ZK-87 Windows: WGC/DXGI → wgpu → NV12 → Media Foundation;
//! ZK-88 macOS: ScreenCaptureKit → wgpu → VideoToolbox → AVAssetWriter). No OS types here: the
//! frame type is the backend's own (a GPU texture, a pool slot), the core only moves it from the
//! source to the sink at the right time.
//!
//! Threading follows LH (§2.2): the recording loop owns the frame source and the sink and runs
//! on one thread; audio sources are polled every 10 ms on another thread and meet the loop in
//! [`crate::audio::AudioBuffer`]s behind a mutex.

use crate::HNS_PER_SEC;
use crate::audio::AudioSampleTime;
use crate::cfr::VideoSampleTime;
use std::fmt;
use std::time::Duration;

// ---------------------------------------------------------------------------------------------
// Capture

/// Result of waiting for the frame source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pulled {
    /// A new frame replaced the latest one.
    Frame,
    /// Nothing new within the wait — the latest frame is still the picture (Duplication gives no
    /// frames between screen changes; a minimised window gives none at all → CFR by repetition).
    Unchanged,
    /// The source is gone for good (the recorded window was closed): the recording stops and is
    /// saved (§2.5).
    Closed,
}

/// Screen / window / region frames (`VidSrc`: Desktop Duplication, WGC, BitBlt, synthetic;
/// SCStream on macOS).
pub trait FrameSource {
    /// The backend's frame (GPU texture, CPU buffer...).
    type Frame: ?Sized;

    /// Wait up to `wait` for a new frame and keep it as the latest. Also called while paused, so
    /// the source keeps its state (Duplication loses it if not read). The first frame after
    /// opening Duplication is empty and must not count (§7 item 27) — the backend skips it.
    /// Losing access (UAC, mode change) is not an error: the backend reopens it itself every
    /// 250 ms and meanwhile reports [`Pulled::Unchanged`] (§2.5).
    fn pull(&mut self, wait: Duration) -> crate::Result<Pulled>;

    /// Whether a real frame has arrived since opening (`VidSrc::have`) — time starts at the
    /// first one. A size change that re-creates the frame pool must NOT clear it (§7 item 32).
    fn has_frame(&self) -> bool;

    /// The picture for slot `slot`. Sources that take a picture on demand (BitBlt, synthetic)
    /// grab it now; the others return the latest frame.
    fn frame_for_slot(&mut self, slot: i64) -> crate::Result<&Self::Frame>;
}

/// Which audio input a source is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AudioKind {
    /// What the computer plays (WASAPI loopback; ScreenCaptureKit audio).
    System,
    Microphone,
}

/// One packet from an audio source: interleaved stereo float, 48 kHz (the OS converts — there
/// is no resampler, §3).
#[derive(Clone, Debug, PartialEq)]
pub struct AudioPacket {
    /// Timestamp of the first frame on the recording clock, 100 ns
    /// ([`crate::clock::ticks_to_hns`] of the same clock the loop reads).
    pub time_hns: i64,
    /// Interleaved L/R samples; `frames()` = `data.len() / 2`.
    pub data: Vec<f32>,
    /// The OS says the packet is silence (`AUDCLNT_BUFFERFLAGS_SILENT`): it still advances the
    /// position but is not added.
    pub silent: bool,
}

impl AudioPacket {
    pub fn frames(&self) -> i64 {
        (self.data.len() / crate::audio::CHANNELS) as i64
    }
}

/// Why an audio source failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AudioError {
    /// The device went away (headphones unplugged, Bluetooth dropped): the track becomes silence
    /// and the device is reopened every 500 ms; for "default" that is the new default (§3).
    DeviceLost,
    /// Privacy settings deny the microphone (`E_ACCESSDENIED`): record without it and say why.
    PermissionDenied,
    /// Another program holds the device exclusively (`AUDCLNT_E_DEVICE_IN_USE`).
    DeviceInUse,
    Other(String),
}

impl fmt::Display for AudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DeviceLost => f.write_str("звуковий пристрій зник"),
            Self::PermissionDenied => f.write_str("немає дозволу на мікрофон"),
            Self::DeviceInUse => f.write_str("звуковий пристрій зайнятий іншою програмою"),
            Self::Other(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for AudioError {}

/// System sound or microphone (`VidAudSrc`: WASAPI shared mode; SCK audio / AVAudioEngine).
pub trait AudioSource: Send {
    fn kind(&self) -> AudioKind;

    /// The device's name as the person sees it ("Microphone (USB Audio)"), once opened; empty
    /// when unknown. It goes into the video document's track (ZK-89).
    fn label(&self) -> String {
        String::new()
    }

    /// Open (or reopen after [`AudioError::DeviceLost`]) the device, asking for 48 kHz stereo
    /// float. Sources are opened BEFORE the encoder so the track is added only when there is
    /// something to record (§7 item 43).
    fn open(&mut self) -> Result<(), AudioError>;

    /// Hand over every packet that has arrived since the last call.
    fn read(&mut self, out: &mut Vec<AudioPacket>) -> Result<(), AudioError>;

    fn close(&mut self);
}

// ---------------------------------------------------------------------------------------------
// Encoding

/// What a sink can do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SinkCaps {
    /// The muxer honours sample DURATIONS (MF SinkWriter does: one sample lasting n slots is
    /// n slots in the file). When false (AVAssetWriter places frames by timestamp, §8 "(?)"),
    /// the recorder writes a stretched sample as n one-slot samples of the same frame.
    pub carries_duration: bool,
}

impl Default for SinkCaps {
    fn default() -> Self {
        Self {
            carries_duration: true,
        }
    }
}

/// Why a sink refused a sample.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SinkError {
    /// Every buffer of the encoder's pool is still held by the encoder (`MF_E_SAMPLEALLOCATOR_EMPTY`)
    /// — try again shortly; the only back-pressure of the loop (§2.2, §7 item 11).
    Busy,
    Failed(String),
}

impl fmt::Display for SinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Busy => f.write_str("пул кодувальника зайнятий"),
            Self::Failed(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for SinkError {}

/// Encoder + muxer writing one file (`VidEnc` over `IMFSinkWriter`; AVAssetWriter). It writes into
/// `<name>.part` and only [`VideoSink::finalize`] makes a playable file: without `moov` no player
/// opens it (§7 item 8). The container is chosen explicitly, not from the `.part` extension
/// (§7 item 9).
pub trait VideoSink {
    type Frame: ?Sized;

    fn caps(&self) -> SinkCaps {
        SinkCaps::default()
    }

    /// Encode `frame` at `t`. The sample may cover several slots ([`VideoSampleTime::slots`]).
    fn write_video(&mut self, frame: &Self::Frame, t: VideoSampleTime) -> Result<(), SinkError>;

    /// Write PCM s16 stereo of audio track `track` at `t`.
    fn write_audio(
        &mut self,
        track: usize,
        pcm: &[i16],
        t: AudioSampleTime,
    ) -> Result<(), SinkError>;

    /// Finish the file (index, `moov`). Called even after a write error, so what was written
    /// survives (§2.5).
    fn finalize(&mut self) -> Result<(), SinkError>;
}

/// What the sink is opened with (see [`crate::settings`] for the values).
#[derive(Clone, Debug, PartialEq)]
pub struct EncoderConfig {
    /// Even, at least 64 (§7 item 7).
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// Average bitrate, bit/s.
    pub bitrate: u32,
    /// Distance between key frames, frames (§7 item 3; decision ZK-17: ~0.25 s for Znimok).
    pub keyframe_interval: u32,
    /// Always 0: B-frames make the software H.264 encoder write a composition offset without an
    /// edit list and the whole video shifts by a frame (§7 item 2).
    pub b_frames: u32,
    /// Audio tracks (0 = no audio), each AAC-LC 48 kHz stereo at [`EncoderConfig::audio_bitrate`].
    pub audio_tracks: usize,
    pub audio_bitrate: u32,
}

// ---------------------------------------------------------------------------------------------
// Decoding

/// What a decoder knows about a file.
#[derive(Clone, Debug, PartialEq)]
pub struct StreamInfo {
    pub width: u32,
    pub height: u32,
    /// Frame rate from the stream (`MF_MT_FRAME_RATE`); may be fractional for foreign files.
    pub fps: f64,
    pub duration_hns: i64,
    pub audio: bool,
}

/// One decoded sample.
#[derive(Clone, Debug, PartialEq)]
pub enum Decoded<F> {
    /// A video sample. It may cover SEVERAL frames — the recorder stretched it (§7 item 18); use
    /// [`frames_of_sample`], not "one sample = one frame".
    Video {
        time_hns: i64,
        duration_hns: i64,
        frame: F,
    },
    /// PCM s16 stereo, 48 kHz.
    Audio { time_hns: i64, pcm: Vec<i16> },
}

/// Reader for playback, thumbnails and export (MF Source Reader; AVAssetReader).
pub trait VideoDecoder {
    type Frame;

    fn info(&self) -> &StreamInfo;

    /// Continue from the key frame before `time_hns`.
    fn seek(&mut self, time_hns: i64) -> crate::Result<()>;

    /// Next sample of any stream; `None` at the end of both.
    fn next(&mut self) -> crate::Result<Option<Decoded<Self::Frame>>>;
}

/// Frame index and frame count of a decoded video sample: `round(ts·fps/1e7)`,
/// `max(1, round(dur·fps/1e7))` (`EvExportRun`, §6.3).
pub fn frames_of_sample(time_hns: i64, duration_hns: i64, fps: f64) -> (i64, i64) {
    let idx = (time_hns as f64 * fps / HNS_PER_SEC as f64 + 0.5) as i64;
    let n = ((duration_hns as f64 * fps / HNS_PER_SEC as f64 + 0.5) as i64).max(1);
    (idx, n)
}

/// Seek target for frame `frame`: a quarter of a frame inside it, so the decoder lands exactly
/// on it (§6.1, §7 item 49).
pub fn seek_time_for_frame(frame: i64, fps: f64) -> i64 {
    ((frame as f64 + 0.25) / fps * HNS_PER_SEC as f64) as i64
}
