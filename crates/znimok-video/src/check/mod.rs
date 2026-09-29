//! Checks of what the recorder and the exporter actually wrote (ZK-99, PLAN phase 8, test side).
//! Ports of the Little Helpers harness tools in `_design_preview/caps73`:
//!
//! - [`mp4`] — a bounded ISO BMFF reader (`moov/trak/mdia/minf/stbl`, `edts/elst`) and the
//!   container checks: `mp4boxes.py` (sample count, key frames from `stss`, `stts`), the summary
//!   line of `vidcheck.cpp` (`frames first last maxGap keyframes duration`) and the traps of
//!   `inventory_video.md` §7 items 1–3, 7, 8, 12, 18 (durations, CFR through durations, B-frames
//!   without an edit list, GOP = fps, even sides, `moov` present, chunk offsets inside `mdat`).
//! - [`gif`] — `gifcheck.py`: header, palette, loop, frames (delays, rectangles, transparency),
//!   full LZW decoding, the bar numbers of composed frames; plus the expectations of
//!   `write_geom_test.py` and the rules of `GifStream` / `GifDelayFor` (§6.4).
//! - [`probe`] — the checks that need DECODED content (a decoder lives in the OS crates, ZK-87 /
//!   ZK-88): `vidcheck --bars` (`seqBreaks`, `syncBad`, `lagMax`) and `evprobe check` (slot of
//!   every sample against the expected frame number; audio level of every frame, `levelBad`,
//!   `offset`, audio length within 50 ms).
//!
//! Everything is std only and never panics on hostile input: sizes are validated against the
//! bytes that back them before anything is allocated, and every loop consumes input or is capped.
//! [`crate::synthetic::check_sync`] is the in-memory relative of [`probe::check_bars`] (it expands
//! samples by duration; `vidcheck` looks at decoded samples as they come).

pub mod gif;
pub mod mp4;
pub mod probe;
mod read;

use std::fmt;

/// One named check with its verdict. Names are the ones the C++ / Python tools printed
/// (`syncBad`, `lagMax`, `levelBad`…) or, for the new ones, camelCase in the same spirit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Check {
    pub name: &'static str,
    pub ok: bool,
    pub detail: String,
}

impl Check {
    pub fn new(name: &'static str, ok: bool, detail: impl Into<String>) -> Self {
        Self {
            name,
            ok,
            detail: detail.into(),
        }
    }
}

impl fmt::Display for Check {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let v = if self.ok { "OK  " } else { "FAIL" };
        write!(f, "{v} {}: {}", self.name, self.detail)
    }
}

/// `true` when every check passed.
pub fn all_ok(checks: &[Check]) -> bool {
    checks.iter().all(|c| c.ok)
}

/// The failed checks.
pub fn failed(checks: &[Check]) -> impl Iterator<Item = &Check> {
    checks.iter().filter(|c| !c.ok)
}

/// Why a file could not be read at all. Messages are Ukrainian like [`crate::VideoError`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CheckError {
    /// The data ends inside a structure (`what`).
    Truncated(&'static str),
    /// A declared size is over the reader's cap (`what`, declared size).
    TooLarge(&'static str, u64),
    /// Not the format or broken structure.
    Malformed(String),
    /// A required part is missing (`moov` of an unfinalised MP4 — §7 item 8).
    Missing(&'static str),
    /// Reading the file failed.
    Io(String),
}

impl fmt::Display for CheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated(w) => write!(f, "файл обірвано: {w}"),
            Self::TooLarge(w, n) => write!(f, "завеликий розмір {w}: {n}"),
            Self::Malformed(m) => write!(f, "зламана структура: {m}"),
            Self::Missing(w) => write!(f, "немає {w}"),
            Self::Io(m) => write!(f, "помилка читання: {m}"),
        }
    }
}

impl std::error::Error for CheckError {}

impl From<std::io::Error> for CheckError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

pub type CheckResult<T> = std::result::Result<T, CheckError>;
