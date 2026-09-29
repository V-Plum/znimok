//! Checks on DECODED content of a synthetic recording or export (ZK-99): the frame number baked
//! into every frame (LH drew it as 16 binary bars, [`crate::synthetic::SyntheticScreen`] keeps
//! it as `slot`) and the frame-coded 1 kHz tone (amplitude `(frame % 8 + 1)·3000`). The decoder
//! is the OS backend's (ZK-87 / ZK-88); these functions take what it returned.
//!
//! - [`check_bars`] — `vidcheck.cpp --bars`: `seqBreaks`, `syncBad`, `lagMax`, `lastNum`;
//! - [`check_cut_video`] — `evprobe.cpp` `Check`, video part: every sample at its slot shows the
//!   expected frame of the kept segments (`bad`, `slots == expect`), with the `EV_STRETCH` rule;
//! - [`check_levels`] — `evprobe.cpp` `Check`, audio part: RMS of the middle of every frame →
//!   level 1..8 must equal `expect[k] % 8 + 1` (silence in `EV_SILENT` segments), at the best
//!   offset within ±30 ms (AAC priming); audio length within 50 ms of the video.

use super::Check;
use crate::HNS_PER_SEC;
use crate::export::{KeepSeg, kept_frames};

/// Content may lag its place by this long (`vid_test.ps1`: "відставання вмісту в межах 0,5 с",
/// `lagMax ≤ 15` at 30 fps, `≤ 30` at 60 fps).
pub const LAG_MAX_S: f64 = 0.5;
/// Audio length against the video, seconds (`evprobe`: `fabs(aDur − expect) > 0.05`).
pub const AUDIO_LEN_TOL_S: f64 = 0.05;
/// Offset search of [`check_levels`], audio frames: `for (off = −1440; off ≤ 1440; off += 48)`.
pub const OFFSET_RANGE: i64 = 1440;
pub const OFFSET_STEP: i64 = 48;
/// Amplitude of level 1 of the frame-coded tone (`3000·(frame % 8 + 1)`).
pub const LEVEL_AMP: f64 = 3000.0;

/// `vidcheck --bars` over decoded frames.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BarsReport {
    pub frames: i64,
    /// Number in the last frame (−1 when there were none).
    pub last_num: i64,
    /// Frames whose number is not the previous one + 1.
    pub seq_breaks: i64,
    /// Frames whose number is from a LATER slot than their timestamp — time was compressed and
    /// the video is shorter than reality.
    pub sync_bad: i64,
    /// Largest lag of content behind its slot, frames (the recorder caught up by repeating).
    pub lag_max: i64,
}

/// `vidcheck.cpp`: for each decoded frame (timestamp 100 ns, bar number) —
/// `slot = round(ts·fps_n/fps_d/1e7)`; `lagMax = max(slot − num)`; `num > slot` → `syncBad`;
/// `num ≠ prev + 1` → `seqBreaks`.
pub fn check_bars(frames: &[(i64, i64)], fps_num: u32, fps_den: u32) -> BarsReport {
    let mut r = BarsReport {
        last_num: -1,
        ..BarsReport::default()
    };
    let fps = if fps_den == 0 {
        0.0
    } else {
        fps_num as f64 / fps_den as f64
    };
    let mut prev: Option<i64> = None;
    for &(ts, num) in frames {
        if let Some(p) = prev
            && num != p + 1
        {
            r.seq_breaks += 1;
        }
        let slot = (ts as f64 * fps / HNS_PER_SEC as f64 + 0.5) as i64;
        r.lag_max = r.lag_max.max(slot - num);
        if num > slot {
            r.sync_bad += 1;
        }
        prev = Some(num);
        r.frames += 1;
    }
    r.last_num = prev.unwrap_or(-1);
    r
}

impl BarsReport {
    /// The harness verdicts: `syncBad = 0`, `lagMax ≤ fps·0.5`; `seqBreaks = 0` when the file must
    /// have no repeats (an export, `marks_test.ps1`: "номери кадрів ідуть підряд").
    pub fn checks(&self, fps: f64, require_sequence: bool) -> Vec<Check> {
        let lag = (fps * LAG_MAX_S).round() as i64;
        let mut v = vec![
            Check::new(
                "syncBad",
                self.sync_bad == 0,
                format!("{} кадрів «з майбутнього»", self.sync_bad),
            ),
            Check::new(
                "lagMax",
                self.lag_max <= lag,
                format!("{} кадрів (не більше {lag})", self.lag_max),
            ),
        ];
        if require_sequence {
            v.push(Check::new(
                "seqBreaks",
                self.seq_breaks == 0,
                format!("{} розривів, lastNum={}", self.seq_breaks, self.last_num),
            ));
        }
        v
    }
}

/// One decoded video sample: timestamp and duration (100 ns) and its bar number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecodedFrame {
    pub time_hns: i64,
    pub duration_hns: i64,
    pub num: i64,
}

/// Source frames the export must show, in order (`expect`).
pub fn expected_frames(keep: &[KeepSeg]) -> Vec<i64> {
    let mut v = Vec::with_capacity(kept_frames(keep).max(0) as usize);
    for s in keep {
        v.extend(s.a..s.b);
    }
    v
}

/// What is visible in each slot when the source had stretched samples (`evprobe gen … stretch`:
/// frames 6 and 7 of every ten repeat frame 5 — `EV_STRETCH`).
pub fn shown_frames(expect: &[i64], stretch: bool) -> Vec<i64> {
    expect
        .iter()
        .map(|&v| {
            if stretch && (v % 10 == 6 || v % 10 == 7) {
                v - v % 10 + 5
            } else {
                v
            }
        })
        .collect()
}

/// `evprobe check`, video part.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CutVideoReport {
    /// Slots the file covers (`k0 + n` of the last sample).
    pub slots: i64,
    pub expect: i64,
    /// Samples showing the wrong frame or lying past the expected end.
    pub bad: i64,
}

impl CutVideoReport {
    pub fn check(&self) -> Check {
        Check::new(
            "videoSlots",
            self.bad == 0 && self.slots == self.expect,
            format!(
                "slots={} expect={} bad={}",
                self.slots, self.expect, self.bad
            ),
        )
    }
}

/// `evprobe.cpp` `Check`: sample at `k0 = round(ts·fps/1e7)` covering `n = max(1,
/// round(dur·fps/1e7))` slots must show `shown[k0]` (a stretched sample carries only its first
/// number, the rest are repeats).
pub fn check_cut_video(
    frames: &[DecodedFrame],
    fps: f64,
    keep: &[KeepSeg],
    stretch: bool,
) -> CutVideoReport {
    let expect = expected_frames(keep);
    let shown = shown_frames(&expect, stretch);
    let mut r = CutVideoReport {
        expect: expect.len() as i64,
        ..CutVideoReport::default()
    };
    for f in frames {
        let k0 = (f.time_hns as f64 * fps / HNS_PER_SEC as f64 + 0.5) as i64;
        let n = ((f.duration_hns as f64 * fps / HNS_PER_SEC as f64 + 0.5) as i64).max(1);
        match usize::try_from(k0).ok().and_then(|k| shown.get(k)) {
            Some(&want) if want == f.num => {}
            _ => r.bad += 1,
        }
        r.slots = k0 + n;
    }
    r
}

/// `evprobe check`, audio part.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LevelReport {
    pub rate: u32,
    pub samples: usize,
    /// Frames whose level is wrong at the best offset.
    pub level_bad: i64,
    /// The best offset, audio frames (AAC priming).
    pub offset: i64,
    pub audio_s: f64,
    pub expect_s: f64,
}

impl LevelReport {
    pub fn checks(&self) -> Vec<Check> {
        vec![
            Check::new(
                "levelBad",
                self.level_bad == 0,
                format!(
                    "{} кадрів з чужою гучністю, offset={} ({:.1} мс)",
                    self.level_bad,
                    self.offset,
                    if self.rate > 0 {
                        self.offset as f64 * 1000.0 / self.rate as f64
                    } else {
                        0.0
                    }
                ),
            ),
            Check::new(
                "audioLength",
                (self.audio_s - self.expect_s).abs() <= AUDIO_LEN_TOL_S,
                format!(
                    "{} семплів ({:.3} с, очікується {:.3})",
                    self.samples, self.audio_s, self.expect_s
                ),
            ),
        ]
    }
}

/// Level expected for source frame `f` (`want` in evprobe): inside a silent segment `[a, b)`
/// silence (0), one frame at each edge not counted (−1, the joint), else `f % 8 + 1`.
fn want_level(f: i64, silent: &[KeepSeg]) -> i64 {
    for g in silent {
        if f >= g.a - 1 && f <= g.b {
            return if f > g.a && f < g.b - 1 { 0 } else { -1 };
        }
    }
    f.rem_euclid(8) + 1
}

/// `evprobe.cpp` `Check`, audio: `left` is the first channel of the decoded PCM from the first
/// audio sample (timestamp `first_ts_hns`). For every expected frame but the first and last, the
/// RMS of `[k+0.3, k+0.7)/fps` → level `round(rms·√2/3000)`; the offset with the fewest wrong
/// levels in ±[`OFFSET_RANGE`] wins — the FIRST of equals, as in evprobe: offsets within ±0.2
/// frame of the true one score the same, so `offset` is only accurate to that (the harness asked
/// for `|offset| ≤ 1 frame`).
pub fn check_levels(
    left: &[i16],
    first_ts_hns: i64,
    rate: u32,
    fps: f64,
    expect: &[i64],
    silent: &[KeepSeg],
) -> LevelReport {
    let r = rate as f64;
    let base = (first_ts_hns as f64 * r / HNS_PER_SEC as f64) as i64;
    let mut best_bad = i64::MAX;
    let mut best_off = 0;
    let mut off = -OFFSET_RANGE;
    while off <= OFFSET_RANGE {
        let mut ab = 0;
        let inner = expect.len().saturating_sub(1);
        for (k, &want) in expect.iter().enumerate().take(inner).skip(1) {
            let c0 = ((k as f64 + 0.3) * r / fps) as i64 - base + off;
            let c1 = ((k as f64 + 0.7) * r / fps) as i64 - base + off;
            if c0 < 0 || c1 > left.len() as i64 || c1 <= c0 {
                continue;
            }
            let e: f64 = left[c0 as usize..c1 as usize]
                .iter()
                .map(|&v| v as f64 * v as f64)
                .sum();
            let rms = (e / (c1 - c0) as f64).sqrt() * std::f64::consts::SQRT_2 / LEVEL_AMP;
            let lvl = (rms + 0.5) as i64;
            let wl = want_level(want, silent);
            if wl >= 0 && lvl != wl {
                ab += 1;
            }
        }
        if ab < best_bad {
            best_bad = ab;
            best_off = off;
        }
        off += OFFSET_STEP;
    }
    LevelReport {
        rate,
        samples: left.len(),
        level_bad: best_bad,
        offset: best_off,
        audio_s: if rate > 0 { left.len() as f64 / r } else { 0.0 },
        expect_s: if fps > 0.0 {
            expect.len() as f64 / fps
        } else {
            0.0
        },
    }
}
