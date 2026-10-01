//! macOS backend of Znimok video (ZK-88): screen recording without a copy through the CPU.
//!
//! ```text
//! SCStream (display / region / window, the cursor drawn by the system; system sound and the
//! microphone in the same stream) ──► BGRA CVPixelBuffer (IOSurface)
//!   ──► AVAssetWriterInputPixelBufferAdaptor ──► AVAssetWriter
//!       (VideoToolbox H.264 High, no B-frames, key frame every ~0.25 s; AAC) ──► .mp4.part → .mp4
//! ```
//!
//! ScreenCaptureKit crops and scales on the GPU itself (a region is the stream's `sourceRect`,
//! the output size its `width`/`height`), so no shader of our own stands between the capture and
//! the encoder. The loop, CFR, pause and the audio timeline are `znimok-video`'s: this crate
//! implements its traits ([`source::ScSource`], [`sink::AvSink`], [`audio::ScAudio`]) and runs a
//! recording on its own thread ([`recording::Recording`]). The writer is shared with the export
//! (ZK-205: [`writer::AvWriter`] takes pixel buffers made from memory too).
//!
//! The recording clock is `mach_absolute_time` — the host clock ScreenCaptureKit stamps its
//! frames and its sound with — so video slots and sound share one time line, as QPC does on
//! Windows.
//!
//! Nothing here on other systems.

#[cfg(target_os = "macos")]
pub mod audio;
#[cfg(target_os = "macos")]
pub mod clock;
#[cfg(target_os = "macos")]
pub mod poster;
#[cfg(target_os = "macos")]
pub mod recording;
#[cfg(target_os = "macos")]
pub mod sink;
#[cfg(target_os = "macos")]
pub mod source;
#[cfg(target_os = "macos")]
pub mod writer;

#[cfg(target_os = "macos")]
pub use audio::ScAudio;
#[cfg(target_os = "macos")]
pub use clock::MachClock;
#[cfg(target_os = "macos")]
pub use recording::{Finished, RecordRequest, Recording, Started, Target};
#[cfg(target_os = "macos")]
pub use writer::{AvWriter, PixelBuf};
