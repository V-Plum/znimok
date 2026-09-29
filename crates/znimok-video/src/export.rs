//! Timing of re-encoding with cuts (§6.3 `EvExportRun`, §7 items 16–18).
//!
//! Cutting without re-encoding would only be possible at key frames (a second off) — rejected
//! (§7 item 16); the export decodes and re-encodes what is kept. This module decides where every
//! decoded sample goes:
//! - **video**: a sample may cover several frames (the recorder stretched it, §7 item 18) → it is
//!   split by the kept segments; the new timestamps are contiguous. The same frame that crossed a
//!   cut is written again as a new sample on the same buffers.
//! - **audio**: cut by TIME (segment edges in audio frames `a/fps·rate`), not by video frames —
//!   otherwise every joint adds up to half a frame of error (§7 item 17); a linear 10 ms fade in
//!   and out, ONLY at real joints (not at the start or end of the file).

use crate::HNS_PER_SEC;

/// A kept segment of the source, frames `[a, b)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeepSeg {
    pub a: i64,
    pub b: i64,
}

impl KeepSeg {
    pub const fn new(a: i64, b: i64) -> Self {
        Self { a, b }
    }
    pub fn len(&self) -> i64 {
        self.b - self.a
    }
    pub fn is_empty(&self) -> bool {
        self.b <= self.a
    }
}

/// Time of output frame `frame`, 100 ns, rounded (`EvTimeOf`).
pub fn time_of(frame: i64, fps: f64) -> i64 {
    (frame as f64 * HNS_PER_SEC as f64 / fps + 0.5) as i64
}

/// Position of source frame `idx` in the new video, `None` when it is cut (`EvMapFrame`).
/// `keep` is sorted and disjoint.
pub fn map_frame(keep: &[KeepSeg], idx: i64) -> Option<i64> {
    let mut before = 0;
    for s in keep {
        if idx < s.a {
            return None;
        }
        if idx < s.b {
            return Some(before + (idx - s.a));
        }
        before += s.len();
    }
    None
}

/// Source frame of output frame `out` — the inverse of [`map_frame`] (`EvSrcOfOut`, used by the
/// GIF palette probes, §6.4). Past the end — the last kept frame; nothing kept — 0.
pub fn src_of_out(keep: &[KeepSeg], out: i64) -> i64 {
    let mut out = out;
    for s in keep {
        if out < s.len() {
            return s.a + out;
        }
        out -= s.len();
    }
    keep.last().map_or(0, |s| s.b - 1)
}

/// Output frames in total.
pub fn kept_frames(keep: &[KeepSeg]) -> i64 {
    keep.iter().map(KeepSeg::len).sum()
}

/// One output video sample made from (part of) a decoded one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutVideo {
    /// First source frame it shows.
    pub src_frame: i64,
    /// Frames it covers.
    pub frames: i64,
    /// First output frame.
    pub out_frame: i64,
    pub time_hns: i64,
    pub duration_hns: i64,
}

/// Split a decoded sample covering source frames `[idx, idx+n)` by the kept segments. Each piece
/// keeps its duration in frames; times come from the output frame numbers, so they are contiguous.
pub fn split_video_sample(keep: &[KeepSeg], idx: i64, n: i64, fps: f64) -> Vec<OutVideo> {
    let mut out = Vec::new();
    for s in keep {
        let from = idx.max(s.a);
        let to = (idx + n).min(s.b);
        if to <= from {
            continue;
        }
        let Some(o) = map_frame(keep, from) else {
            continue;
        };
        let t0 = time_of(o, fps);
        out.push(OutVideo {
            src_frame: from,
            frames: to - from,
            out_frame: o,
            time_hns: t0,
            duration_hns: time_of(o + (to - from), fps) - t0,
        });
    }
    out
}

/// Same as [`split_video_sample`] but one output sample per frame — for a path that draws marks
/// on each frame separately (`EvMarksEmit`).
pub fn split_video_frames(keep: &[KeepSeg], idx: i64, n: i64, fps: f64) -> Vec<OutVideo> {
    split_video_sample(keep, idx, n, fps)
        .into_iter()
        .flat_map(|p| {
            (0..p.frames).map(move |i| {
                let o = p.out_frame + i;
                let t0 = time_of(o, fps);
                OutVideo {
                    src_frame: p.src_frame + i,
                    frames: 1,
                    out_frame: o,
                    time_hns: t0,
                    duration_hns: time_of(o + 1, fps) - t0,
                }
            })
        })
        .collect()
}

/// One output audio sample.
#[derive(Clone, Debug, PartialEq)]
pub struct OutAudio {
    /// Output audio frame index.
    pub index: i64,
    pub time_hns: i64,
    pub duration_hns: i64,
    /// Interleaved PCM s16 with fades applied.
    pub pcm: Vec<i16>,
}

/// Cutting audio of an export by time.
#[derive(Clone, Debug)]
pub struct AudioCut {
    keep: Vec<KeepSeg>,
    /// Segment edges in audio frames and their positions in the output.
    sa: Vec<i64>,
    sb: Vec<i64>,
    ob: Vec<i64>,
    rate: i64,
    channels: usize,
    /// Frames of the whole source video (a segment ending there has no joint after it).
    total_frames: i64,
}

impl AudioCut {
    pub fn new(keep: &[KeepSeg], fps: f64, rate: i64, channels: usize, total_frames: i64) -> Self {
        let edge = |f: i64| (f as f64 / fps * rate as f64 + 0.5) as i64;
        let (mut sa, mut sb, mut ob) = (Vec::new(), Vec::new(), Vec::new());
        let mut acc = 0;
        for s in keep {
            sa.push(edge(s.a));
            sb.push(edge(s.b));
            ob.push(acc);
            acc += edge(s.b) - edge(s.a);
        }
        Self {
            keep: keep.to_vec(),
            sa,
            sb,
            ob,
            rate,
            channels,
            total_frames,
        }
    }

    /// Fade length, frames (10 ms).
    pub fn fade(&self) -> i64 {
        self.rate / 100
    }

    /// Output audio frames in total.
    pub fn total_out(&self) -> i64 {
        self.sa.iter().zip(&self.sb).map(|(a, b)| b - a).sum()
    }

    /// Cut a decoded packet starting at `time_hns` (`a0 = round(ts·rate/1e7)`).
    pub fn cut(&self, time_hns: i64, pcm: &[i16]) -> Vec<OutAudio> {
        let a0 = (time_hns as f64 * self.rate as f64 / HNS_PER_SEC as f64 + 0.5) as i64;
        let ch = self.channels;
        let n = (pcm.len() / ch) as i64;
        let fade = self.fade();
        let mut out = Vec::new();
        for k in 0..self.keep.len() {
            let (sa, sb) = (self.sa[k], self.sb[k]);
            let from = a0.max(sa);
            let to = (a0 + n).min(sb);
            if to <= from {
                continue;
            }
            let cnt = to - from;
            let mut buf = pcm[(from - a0) as usize * ch..(to - a0) as usize * ch].to_vec();
            // Fades only where there really is a joint, not at the very start or end of the file.
            let cut_in = self.keep[k].a > 0;
            let cut_out = self.keep[k].b < self.total_frames;
            for f in 0..cnt {
                let af = from + f;
                let mut g = 1.0f64;
                if cut_in && af - sa < fade {
                    g = (af - sa) as f64 / fade as f64;
                }
                if cut_out && sb - 1 - af < fade {
                    g = g.min((sb - 1 - af) as f64 / fade as f64);
                }
                if g < 1.0 {
                    for c in 0..ch {
                        let s = &mut buf[f as usize * ch + c];
                        *s = (*s as f64 * g) as i16;
                    }
                }
            }
            let op0 = self.ob[k] + (from - sa);
            let t = |i: i64| (i as f64 * HNS_PER_SEC as f64 / self.rate as f64 + 0.5) as i64;
            out.push(OutAudio {
                index: op0,
                time_hns: t(op0),
                duration_hns: t(op0 + cnt) - t(op0),
                pcm: buf,
            });
        }
        out
    }

    /// Whether a packet ending at audio frame `end` reaches past the last kept audio.
    pub fn done_at(&self, end: i64) -> bool {
        self.sb.last().is_none_or(|&b| end >= b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §6.2: frame → position in the new video, cut → None.
    #[test]
    fn map_frames() {
        let keep = [KeepSeg::new(10, 20), KeepSeg::new(30, 35)];
        assert_eq!(map_frame(&keep, 5), None);
        assert_eq!(map_frame(&keep, 10), Some(0));
        assert_eq!(map_frame(&keep, 19), Some(9));
        assert_eq!(map_frame(&keep, 25), None);
        assert_eq!(map_frame(&keep, 30), Some(10));
        assert_eq!(map_frame(&keep, 35), None);
        assert_eq!(kept_frames(&keep), 15);
    }

    /// §7 item 18: a recorded sample covering 6 frames across a cut is split in two, each keeps
    /// its frame count, the output timeline is contiguous.
    #[test]
    fn stretched_sample_split_across_cut() {
        let keep = [KeepSeg::new(0, 12), KeepSeg::new(14, 100)];
        let p = split_video_sample(&keep, 10, 6, 30.0);
        assert_eq!(p.len(), 2);
        assert_eq!((p[0].src_frame, p[0].frames, p[0].out_frame), (10, 2, 10));
        assert_eq!((p[1].src_frame, p[1].frames, p[1].out_frame), (14, 2, 12));
        assert_eq!(p[0].time_hns + p[0].duration_hns, p[1].time_hns);
        assert_eq!(p[1].time_hns, time_of(12, 30.0));
    }

    /// §7 item 18: the frame-by-frame split gives one sample per kept frame of the long sample.
    #[test]
    fn stretched_sample_per_frame() {
        let keep = [KeepSeg::new(0, 100)];
        let p = split_video_frames(&keep, 3, 4, 30.0);
        assert_eq!(
            p.iter().map(|x| x.out_frame).collect::<Vec<_>>(),
            [3, 4, 5, 6]
        );
        assert!(
            p.iter()
                .all(|x| x.frames == 1 && x.src_frame == x.out_frame)
        );
    }

    /// Fully cut sample → nothing.
    #[test]
    fn cut_sample_disappears() {
        let keep = [KeepSeg::new(0, 10), KeepSeg::new(20, 30)];
        assert!(split_video_sample(&keep, 12, 5, 30.0).is_empty());
    }

    /// §7 item 17: audio is cut by TIME — segment edges at `a/fps·rate` rounded — so at 30 fps
    /// every segment is exactly 1600 frames per video frame and the joints do not accumulate
    /// error; at 29.97 fps the edges are rounded individually, not per-frame.
    #[test]
    fn audio_cut_by_time() {
        let keep = [KeepSeg::new(0, 10), KeepSeg::new(20, 30)];
        let c = AudioCut::new(&keep, 30.0, 48_000, 2, 30);
        assert_eq!(c.total_out(), 32_000);
        let pcm = vec![10_000i16; 48_000 * 2];
        let parts = c.cut(0, &pcm);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].index, 0);
        assert_eq!(parts[0].pcm.len(), 16_000 * 2);
        assert_eq!(parts[1].index, 16_000);
        assert_eq!(parts[1].time_hns, parts[0].time_hns + parts[0].duration_hns);
        let ntsc = AudioCut::new(
            &[KeepSeg::new(0, 7), KeepSeg::new(8, 15)],
            30000.0 / 1001.0,
            48_000,
            2,
            15,
        );
        // 1601.6 audio frames per video frame: edges 0, 11211 | 12813, 24024 — each rounded once
        assert_eq!(
            (ntsc.sa.clone(), ntsc.sb.clone()),
            (vec![0, 12_813], vec![11_211, 24_024])
        );
        assert_eq!(ntsc.total_out(), 22_422);
    }

    /// §3 / §7 item 17: 10 ms linear fades only at real joints — segment 1 starts at the file
    /// start (no fade in) and ends at a cut (fade out); segment 2 starts at a cut (fade in) and
    /// ends at the file end (no fade out).
    #[test]
    fn fades_only_at_real_joints() {
        let keep = [KeepSeg::new(0, 10), KeepSeg::new(20, 30)];
        let c = AudioCut::new(&keep, 30.0, 48_000, 2, 30);
        let pcm = vec![10_000i16; 48_000 * 2];
        let parts = c.cut(0, &pcm);
        let a = &parts[0].pcm;
        let b = &parts[1].pcm;
        assert_eq!(a[0], 10_000, "file start — no fade in");
        assert_eq!(*a.last().unwrap(), 0, "joint — faded out to 0");
        assert_eq!(
            a[a.len() - 2 * 480],
            (10_000 * 479 / 480) as i16,
            "fade is 480 frames long"
        );
        assert_eq!(a[a.len() - 2 * 481], 10_000, "before the fade — untouched");
        assert_eq!(b[0], 0, "joint — fade in from 0");
        assert_eq!(b[2 * 240], 5_000);
        assert_eq!(*b.last().unwrap(), 10_000, "file end — no fade out");
    }

    /// Export timing: packets are cut where they fall; the stream is done at the last kept edge.
    #[test]
    fn packets_and_done() {
        let keep = [KeepSeg::new(30, 60)];
        let c = AudioCut::new(&keep, 30.0, 48_000, 2, 90);
        let pkt = vec![1i16; 1024 * 2];
        assert!(c.cut(0, &pkt).is_empty());
        let t = 48_000 * HNS_PER_SEC / 48_000 - 100 * HNS_PER_SEC / 48_000; // 100 frames before the edge
        let p = c.cut(t, &pkt);
        assert_eq!(p[0].index, 0);
        assert_eq!(p[0].pcm.len(), (1024 - 100) * 2);
        assert!(!c.done_at(48_000 + 924));
        assert!(c.done_at(96_000));
    }

    /// §6.1 / §7 item 49 and §6.3: seek target a quarter frame inside; sample → frames rounding.
    #[test]
    fn decoder_helpers() {
        use crate::traits::{frames_of_sample, seek_time_for_frame};
        assert_eq!(seek_time_for_frame(0, 30.0), 83_333);
        assert_eq!(seek_time_for_frame(30, 30.0), 10_083_333);
        assert_eq!(frames_of_sample(333_333, 1_333_334, 30.0), (1, 4));
        assert_eq!(frames_of_sample(0, 1, 30.0), (0, 1), "at least one frame");
    }
}
