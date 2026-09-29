//! Platform-neutral core of Znimok video (ZK-86, PLAN phase 8). Ported from Little Helpers
//! (`docs/discovery/inventory_video.md` §2.3, §2.4, §3, §7) — the logic is kept as it was there,
//! the Win32/MF parts are left to the OS crates (ZK-87 Windows, ZK-88 macOS).
//!
//! - [`clock`] — one monotonic clock for everything (QPC / `mach_absolute_time` / `Instant`),
//!   tick → 100 ns conversion, a manual clock for tests.
//! - [`cfr`] — constant frame rate through **sample duration**: slot `k` is due at
//!   `t0 + k·f/fps`; a late loop writes ONE sample that lasts `n` slots (§2.3, §7 items 1, 18).
//! - [`pause`] — pause by shifting `t0` (CAPS-101, §2.4) and mapping wall-clock events
//!   (browser log) onto video time around pauses (§5.5).
//! - [`audio`] — the audio timeline: packets placed by their timestamp, resync over 10 ms,
//!   a 300 ms lag, zero = first video frame, pause drop and shift, the tail cut at the end of the
//!   video; mixing by sum with a soft `tanh` limiter (§3, §7 items 41–44).
//! - [`traits`] — what an OS backend implements: frame source, audio source, sink (encoder +
//!   muxer), decoder.
//! - [`recorder`] — the recording loop of `VidRecord`, step by step, over those traits.
//! - [`export`] — timing for re-encoding with cuts: splitting a sample that covers several frames,
//!   cutting audio by TIME with 10 ms fades at real joints (§6.3, §7 items 17, 18).
//! - [`settings`] — frame rate, bitrate, even sizes and the like (§1, §2.6, §7 items 2, 3, 7, 15).
//! - [`stride`] — row pitch of decoded buffers and copying by rows (§7 items 5, 6).
//! - [`events`] — timed input events (clicks) gated by `t0` and pauses (§2.4, §4).
//! - [`synthetic`] — deterministic frame and audio generators, a memory sink and decoder, and the
//!   `vidcheck` sync checks, for tests here and conformance tests of the OS backends.
//!
//! Time units: clock ticks (`i64`, frequency from the clock) inside the loop; **100 ns** for
//! sample times and durations (the unit of MF and of the MP4 muxers here); audio frames at
//! 48 kHz for the audio timeline.

pub mod audio;
pub mod cfr;
pub mod check;
pub mod clock;
pub mod events;
pub mod export;
pub mod pause;
pub mod recorder;
pub mod settings;
pub mod stride;
pub mod synthetic;
pub mod traits;

pub use audio::{AudioBuffer, AudioTimeline, PacketPlacer};
pub use cfr::{Cfr, Slot, VideoSampleTime};
pub use clock::{Clock, ManualClock, MonotonicClock};
pub use pause::{PauseLog, PauseSpan};
pub use recorder::{Recorder, RecorderConfig, RecordingControl, RecordingResult};
pub use traits::*;

use std::fmt;

/// 100 ns units per second — the time base of samples.
pub const HNS_PER_SEC: i64 = 10_000_000;

pub type Result<T> = std::result::Result<T, VideoError>;

/// Errors of the video pipeline. Messages are in Ukrainian, like the other crates' errors
/// (they end up in the tray / notifications).
#[derive(Clone, Debug, PartialEq)]
pub enum VideoError {
    /// The screen cannot be captured (no display output, access denied, e.g. a minimised RDP
    /// client — §2.5).
    Screen(String),
    /// No encoder could be opened (hardware nor software).
    Encoder(String),
    /// Writing a sample failed mid-recording (the file is still finalised — §2.5).
    Write(String),
    /// The encoder kept every buffer of its pool for too long (§2.2: 250 × 2 ms).
    Backpressure,
    /// `Finalize` failed — the file has no index (`moov`) and is deleted (§7 item 8).
    Finalize(String),
    /// Decoding failed.
    Decode(String),
    /// Bad arguments (fps 0, empty frame and the like).
    Invalid(String),
    /// A file operation (`.part` rename / delete).
    Io(String),
}

impl fmt::Display for VideoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Screen(m) => write!(f, "екран недоступний: {m}"),
            Self::Encoder(m) => write!(f, "не вдалося відкрити кодувальник: {m}"),
            Self::Write(m) => write!(f, "помилка запису відео: {m}"),
            Self::Backpressure => f.write_str("кодувальник не встигає: пул кадрів зайнятий"),
            Self::Finalize(m) => write!(f, "не вдалося дописати файл відео: {m}"),
            Self::Decode(m) => write!(f, "помилка декодування: {m}"),
            Self::Invalid(m) => write!(f, "неправильні параметри: {m}"),
            Self::Io(m) => write!(f, "помилка файлу: {m}"),
        }
    }
}

impl std::error::Error for VideoError {}
