//! Deterministic sources and sinks for tests — here and in the conformance tests of the OS
//! backends. The idea is LH's test build (`VidSynthFill`, `VidAudSynthPoll`, `vidcheck`), not its
//! code:
//! - [`SyntheticScreen`] stamps every frame with the slot number it was taken for (LH drew it as
//!   16 binary bars), so a decoded file tells which slot each frame shows;
//! - [`SyntheticAudio`] makes a frame-coded 1 kHz tone — amplitude `(frame % 8 + 1)/8` of 3000
//!   for the video frame the audio belongs to — or a quiet 440 Hz tone for the microphone;
//! - [`MemorySink`] keeps what was written and can play back through [`MemoryDecoder`];
//! - [`check_sync`] / [`check_audio_sync`] are `vidcheck`: no frame from the future (`syncBad`),
//!   how far content lags (`lagMax`), a continuous timeline.
//!
//! Everything runs on a [`ManualClock`]: sources sleep on it, so time passes exactly as the test
//! says.

use crate::audio::{self, AudioSampleTime, TimelineProbe};
use crate::cfr::VideoSampleTime;
use crate::clock::{Clock, ManualClock, ticks_to_hns};
use crate::traits::*;
use crate::{HNS_PER_SEC, Result, VideoError};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

// ---------------------------------------------------------------------------------------------
// Frames

/// A synthetic frame: which slot it was taken for, and a serial number of the "screen change".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SynthFrame {
    pub slot: i64,
    pub change: u64,
}

/// A screen whose content changes every `change_every`, with knobs for the awkward cases.
pub struct SyntheticScreen {
    clock: ManualClock,
    opened_at: i64,
    /// First real frame this long after opening (Duplication: the first one after
    /// `DuplicateOutput` is empty and skipped, §7 item 27).
    first_after: i64,
    change_every: i64,
    last_change: Option<u64>,
    frame: SynthFrame,
    have: bool,
    closes_at: Option<i64>,
    /// Taking the picture of slot `k` costs this long (the loop falls behind).
    stalls: Vec<(i64, Duration)>,
    /// How many times the source was pulled / asked for a slot.
    pub pulls: u64,
    pub grabs: u64,
}

impl SyntheticScreen {
    pub fn new(clock: ManualClock) -> Self {
        let now = clock.ticks();
        let f = clock.frequency();
        Self {
            clock,
            opened_at: now,
            first_after: 0,
            change_every: f / 60,
            last_change: None,
            frame: SynthFrame::default(),
            have: false,
            closes_at: None,
            stalls: Vec::new(),
            pulls: 0,
            grabs: 0,
        }
    }

    /// First real frame only after `d` (the empty first frame and the like).
    pub fn first_frame_after(mut self, d: Duration) -> Self {
        self.first_after = self.clock.ticks_for(d);
        self
    }

    /// The screen changes this often (Duplication gives frames only on changes).
    pub fn change_every(mut self, d: Duration) -> Self {
        self.change_every = self.clock.ticks_for(d).max(1);
        self
    }

    /// The window is closed `d` after opening.
    pub fn closes_after(mut self, d: Duration) -> Self {
        self.closes_at = Some(self.opened_at + self.clock.ticks_for(d));
        self
    }

    /// Taking slot `slot` takes `d` (e.g. a busy GPU) — the loop falls behind.
    pub fn stall_at(mut self, slot: i64, d: Duration) -> Self {
        self.stalls.push((slot, d));
        self
    }

    fn change_now(&self) -> Option<u64> {
        let t = self.clock.ticks() - self.opened_at - self.first_after;
        (t >= 0).then(|| (t / self.change_every) as u64)
    }
}

impl FrameSource for SyntheticScreen {
    type Frame = SynthFrame;

    fn pull(&mut self, wait: Duration) -> Result<Pulled> {
        self.pulls += 1;
        if let Some(c) = self.closes_at
            && self.clock.ticks() >= c
        {
            return Ok(Pulled::Closed);
        }
        // Wait until the next change or the timeout, whichever comes first.
        let now = self.clock.ticks();
        let base = self.opened_at + self.first_after;
        let next = if now < base {
            base
        } else {
            base + ((now - base) / self.change_every + 1) * self.change_every
        };
        let w = self.clock.ticks_for(wait);
        self.clock.advance_ticks((next - now).clamp(0, w));
        match self.change_now() {
            Some(c) if self.last_change != Some(c) => {
                self.last_change = Some(c);
                self.have = true;
                self.frame.change = c;
                Ok(Pulled::Frame)
            }
            _ => Ok(Pulled::Unchanged),
        }
    }

    fn has_frame(&self) -> bool {
        self.have
    }

    fn frame_for_slot(&mut self, slot: i64) -> Result<&SynthFrame> {
        self.grabs += 1;
        if let Some(&(_, d)) = self.stalls.iter().find(|(s, _)| *s == slot) {
            self.clock.sleep(d);
        }
        if !self.have {
            return Err(VideoError::Screen("no frame yet".into()));
        }
        self.frame.slot = slot;
        Ok(&self.frame)
    }
}

// ---------------------------------------------------------------------------------------------
// Audio

/// What a synthetic audio source plays.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tone {
    /// 1 kHz, amplitude `3000·(frame % 8 + 1)/32768` where `frame` is the video frame this audio
    /// belongs to on the OUTPUT timeline (needs the recorder's [`TimelineProbe`]); silence before
    /// the first frame.
    FrameCoded { fps: u32 },
    /// 440 Hz at 0.05 — the "microphone".
    Quiet440,
    /// A constant value (to test sums and the limiter).
    Constant(f32),
}

/// A 48 kHz stereo source that delivers 10 ms packets stamped with the clock.
pub struct SyntheticAudio {
    kind: AudioKind,
    tone: Tone,
    clock: ManualClock,
    probe: Option<Arc<TimelineProbe>>,
    /// Absolute index (from clock zero) of the next packet.
    next: Option<i64>,
    /// Packets are delivered once they are this old (frames) — `VidAudSynthPoll` waits 480.
    deliver_after: i64,
    open_error: Option<AudioError>,
    fail_read_at_ms: Option<i64>,
    lose_at_ms: Option<(i64, i64)>,
    silent: bool,
    /// Added to every packet's stamp in turn (timestamp jitter).
    jitter: Vec<i64>,
    packets: u64,
    pub opens: u32,
    pub open: bool,
}

impl SyntheticAudio {
    pub fn new(kind: AudioKind, tone: Tone, clock: ManualClock) -> Self {
        Self {
            kind,
            tone,
            clock,
            probe: None,
            next: None,
            deliver_after: audio::RESYNC,
            open_error: None,
            fail_read_at_ms: None,
            lose_at_ms: None,
            silent: false,
            jitter: Vec::new(),
            packets: 0,
            opens: 0,
            open: false,
        }
    }

    /// Needed for [`Tone::FrameCoded`].
    pub fn probe(mut self, p: Arc<TimelineProbe>) -> Self {
        self.probe = Some(p);
        self
    }

    /// Opening fails with this.
    pub fn fail_open(mut self, e: AudioError) -> Self {
        self.open_error = Some(e);
        self
    }

    /// Reading fails with `DeviceLost` once the clock (ms) passes `ms`; reopening works.
    pub fn lose_device_at_ms(mut self, ms: i64) -> Self {
        self.fail_read_at_ms = Some(ms);
        self
    }

    /// No packets for `len` ms from `at` ms (clock), then on from where time is (headphones
    /// unplugged and back — LH `synthLoseAt`).
    pub fn gap(mut self, at_ms: i64, len_ms: i64) -> Self {
        self.lose_at_ms = Some((at_ms, len_ms));
        self
    }

    /// Every packet flagged silent.
    pub fn silent(mut self) -> Self {
        self.silent = true;
        self
    }

    /// Packets arrive this late (frames) — to test the 300 ms lag.
    pub fn deliver_after(mut self, frames: i64) -> Self {
        self.deliver_after = frames;
        self
    }

    pub fn jitter(mut self, j: Vec<i64>) -> Self {
        self.jitter = j;
        self
    }

    fn abs_index(&self) -> i64 {
        ticks_to_hns(self.clock.ticks(), self.clock.frequency()) * audio::RATE / HNS_PER_SEC
    }

    fn now_ms(&self) -> i64 {
        crate::clock::ticks_to_ms(self.clock.ticks(), self.clock.frequency())
    }

    fn sample(&self, abs: i64) -> f32 {
        let a = abs % audio::RATE;
        let tau = 2.0 * std::f32::consts::PI;
        match self.tone {
            Tone::FrameCoded { fps } => {
                let Some(p) = &self.probe else { return 0.0 };
                let Some(zero) = p.zero() else { return 0.0 };
                // timeline index of this sample
                let anchor_idx = p.anchor_hns() * audio::RATE / HNS_PER_SEC;
                let idx = abs - anchor_idx;
                let fr = (idx - zero - p.shift()) * fps as i64 / audio::RATE;
                let amp = if fr >= 0 {
                    3000.0 * ((fr % 8) + 1) as f32 / 32768.0
                } else {
                    0.0
                };
                amp * (tau * 1000.0 * a as f32 / audio::RATE as f32).sin()
            }
            Tone::Quiet440 => 0.05 * (tau * 440.0 * a as f32 / audio::RATE as f32).sin(),
            Tone::Constant(v) => v,
        }
    }
}

impl AudioSource for SyntheticAudio {
    fn kind(&self) -> AudioKind {
        self.kind
    }

    fn open(&mut self) -> std::result::Result<(), AudioError> {
        self.opens += 1;
        if let Some(e) = self.open_error.clone() {
            return Err(e);
        }
        self.open = true;
        self.next = None;
        Ok(())
    }

    fn read(&mut self, out: &mut Vec<AudioPacket>) -> std::result::Result<(), AudioError> {
        if let Some(ms) = self.fail_read_at_ms
            && self.now_ms() >= ms
        {
            self.fail_read_at_ms = None;
            return Err(AudioError::DeviceLost);
        }
        let now = self.abs_index();
        let next = *self.next.get_or_insert(now - audio::RESYNC);
        let mut next = next;
        const PK: i64 = 480;
        while next + PK <= now - self.deliver_after {
            if let Some((at, len)) = self.lose_at_ms {
                let from = at * audio::RATE / 1000;
                let to = (at + len) * audio::RATE / 1000;
                if next >= from && next < to {
                    next = to;
                    continue;
                }
            }
            let data: Vec<f32> = (0..PK)
                .flat_map(|i| {
                    let v = self.sample(next + i);
                    [v, v]
                })
                .collect();
            let j = if self.jitter.is_empty() {
                0
            } else {
                self.jitter[self.packets as usize % self.jitter.len()]
            };
            out.push(AudioPacket {
                time_hns: (next + j) * HNS_PER_SEC / audio::RATE,
                data,
                silent: self.silent,
            });
            self.packets += 1;
            next += PK;
        }
        self.next = Some(next);
        Ok(())
    }

    fn close(&mut self) {
        self.open = false;
    }
}

// ---------------------------------------------------------------------------------------------
// Sink and decoder

/// A written video sample.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WrittenVideo {
    pub time: VideoSampleTime,
    pub frame: SynthFrame,
}

/// A written audio sample.
#[derive(Clone, Debug, PartialEq)]
pub struct WrittenAudio {
    pub track: usize,
    pub time: AudioSampleTime,
    pub pcm: Vec<i16>,
}

/// A "file" in memory with knobs for failures.
#[derive(Clone, Debug, Default)]
pub struct MemorySink {
    pub config: Option<EncoderConfig>,
    pub video: Vec<WrittenVideo>,
    pub audio: Vec<WrittenAudio>,
    pub finalized: bool,
    pub caps: SinkCaps,
    /// Once `busy_after` samples are written, the next `busy` video writes answer `Busy`.
    pub busy: u32,
    pub busy_after: usize,
    /// Video write number `n` (0-based) fails.
    pub fail_at: Option<usize>,
    pub fail_finalize: bool,
    /// How many `Busy` answers were given.
    pub busy_given: u32,
}

impl MemorySink {
    pub fn new() -> Self {
        Self::default()
    }

    /// A sink that ignores durations (AVAssetWriter-like).
    pub fn by_timestamp() -> Self {
        Self {
            caps: SinkCaps {
                carries_duration: false,
            },
            ..Self::default()
        }
    }

    /// Audio of track `track`, concatenated in order, with the index of its first frame.
    pub fn track(&self, track: usize) -> Vec<(AudioSampleTime, &[i16])> {
        self.audio
            .iter()
            .filter(|a| a.track == track)
            .map(|a| (a.time, a.pcm.as_slice()))
            .collect()
    }

    /// Duration of the video, 100 ns (sum of sample durations — what MP4 counts).
    pub fn video_duration(&self) -> i64 {
        self.video.iter().map(|v| v.time.duration).sum()
    }

    /// Play back.
    pub fn decoder(&self, fps: u32) -> MemoryDecoder {
        MemoryDecoder::new(self, fps)
    }
}

impl VideoSink for MemorySink {
    type Frame = SynthFrame;

    fn caps(&self) -> SinkCaps {
        self.caps
    }

    fn write_video(
        &mut self,
        frame: &SynthFrame,
        t: VideoSampleTime,
    ) -> std::result::Result<(), SinkError> {
        if self.busy > 0 && self.video.len() >= self.busy_after {
            self.busy -= 1;
            self.busy_given += 1;
            return Err(SinkError::Busy);
        }
        if self.fail_at == Some(self.video.len()) {
            return Err(SinkError::Failed("synthetic write failure".into()));
        }
        self.video.push(WrittenVideo {
            time: t,
            frame: *frame,
        });
        Ok(())
    }

    fn write_audio(
        &mut self,
        track: usize,
        pcm: &[i16],
        t: AudioSampleTime,
    ) -> std::result::Result<(), SinkError> {
        self.audio.push(WrittenAudio {
            track,
            time: t,
            pcm: pcm.to_vec(),
        });
        Ok(())
    }

    fn finalize(&mut self) -> std::result::Result<(), SinkError> {
        if self.fail_finalize {
            return Err(SinkError::Failed("synthetic finalize failure".into()));
        }
        self.finalized = true;
        Ok(())
    }
}

/// Plays a [`MemorySink`] back in time order (video before audio at equal times).
pub struct MemoryDecoder {
    info: StreamInfo,
    items: Vec<Decoded<SynthFrame>>,
    queue: VecDeque<Decoded<SynthFrame>>,
}

impl MemoryDecoder {
    fn new(s: &MemorySink, fps: u32) -> Self {
        let mut items: Vec<(i64, u8, Decoded<SynthFrame>)> = s
            .video
            .iter()
            .map(|v| {
                (
                    v.time.time,
                    0,
                    Decoded::Video {
                        time_hns: v.time.time,
                        duration_hns: v.time.duration,
                        frame: v.frame,
                    },
                )
            })
            .chain(s.audio.iter().filter(|a| a.track == 0).map(|a| {
                (
                    a.time.time,
                    1,
                    Decoded::Audio {
                        time_hns: a.time.time,
                        pcm: a.pcm.clone(),
                    },
                )
            }))
            .collect();
        items.sort_by_key(|(t, k, _)| (*t, *k));
        let items: Vec<_> = items.into_iter().map(|(_, _, d)| d).collect();
        let info = StreamInfo {
            width: s.config.as_ref().map_or(0, |c| c.width),
            height: s.config.as_ref().map_or(0, |c| c.height),
            fps: fps as f64,
            duration_hns: s.video_duration(),
            audio: !s.audio.is_empty(),
        };
        Self {
            info,
            queue: items.iter().cloned().collect(),
            items,
        }
    }
}

impl VideoDecoder for MemoryDecoder {
    type Frame = SynthFrame;

    fn info(&self) -> &StreamInfo {
        &self.info
    }

    /// From the last video sample at or before `time_hns` (every sample is a "key frame" here).
    fn seek(&mut self, time_hns: i64) -> Result<()> {
        let start = self
            .items
            .iter()
            .rposition(|d| matches!(d, Decoded::Video { time_hns: t, .. } if *t <= time_hns))
            .unwrap_or(0);
        self.queue = self.items[start..].iter().cloned().collect();
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Decoded<SynthFrame>>> {
        Ok(self.queue.pop_front())
    }
}

// ---------------------------------------------------------------------------------------------
// vidcheck

/// What `vidcheck` reports about a recorded video.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SyncReport {
    /// Output frames (samples expanded by their duration).
    pub frames: i64,
    /// Frames whose content is from a LATER slot than their place — time was compressed.
    pub sync_bad: i64,
    /// Largest lag of content behind its place, frames (allowed: the loop fell behind and the
    /// frame was repeated).
    pub lag_max: i64,
    /// Samples not starting where the previous one ended.
    pub gaps: i64,
    /// Sum of durations, 100 ns.
    pub duration_hns: i64,
}

/// Check a written video (§7 item 1): each output frame's content slot must not be from the
/// future; the timeline must be continuous; the duration is the sum of sample durations.
pub fn check_sync(video: &[WrittenVideo], fps: u32) -> SyncReport {
    let mut r = SyncReport::default();
    let mut prev_end = video.first().map_or(0, |v| v.time.time);
    for v in video {
        if v.time.time != prev_end {
            r.gaps += 1;
        }
        prev_end = v.time.end();
        r.duration_hns += v.time.duration;
        let n = ((v.time.duration as f64 * fps as f64 / HNS_PER_SEC as f64) + 0.5) as i64;
        let first = ((v.time.time as f64 * fps as f64 / HNS_PER_SEC as f64) + 0.5) as i64;
        for i in 0..n.max(1) {
            let slot = first + i;
            let num = v.frame.slot;
            r.frames += 1;
            r.lag_max = r.lag_max.max(slot - num);
            if num > slot {
                r.sync_bad += 1;
            }
        }
    }
    r
}

/// Check a frame-coded audio track: in every whole 1 kHz period (48 frames) that lies inside
/// one video frame, the peak must match `(frame % 8 + 1)·3000` of that frame (±`tol` in s16).
/// Returns (periods checked, periods wrong).
pub fn check_audio_sync(track: &[(AudioSampleTime, &[i16])], fps: u32, tol: i16) -> (i64, i64) {
    let mut pcm: Vec<i16> = Vec::new();
    let start = track.first().map_or(0, |(t, _)| t.index);
    for (_, p) in track {
        pcm.extend(p.iter().step_by(2)); // left channel
    }
    // The very end may be silence: packets are delivered 10 ms late and the tail is cut at the
    // end of the video, not at the last packet.
    while pcm.last() == Some(&0) {
        pcm.pop();
    }
    let per_frame = audio::RATE / fps as i64;
    let (mut checked, mut wrong) = (0, 0);
    let mut i = 0usize;
    while i + 48 <= pcm.len() {
        let idx = start + i as i64;
        let fr = idx * fps as i64 / audio::RATE;
        // Whole periods well inside one frame (a sample of placement rounding either side).
        if idx - fr * per_frame >= 48 && (fr + 1) * per_frame - (idx + 48) >= 48 {
            let peak = pcm[i..i + 48]
                .iter()
                .map(|v| v.unsigned_abs())
                .max()
                .unwrap_or(0) as i32;
            let want = (3000.0 * ((fr % 8) + 1) as f32 / 32768.0 * 32767.0) as i32;
            checked += 1;
            if (peak - want).abs() > tol as i32 {
                wrong += 1;
            }
        }
        i += 48;
    }
    (checked, wrong)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The synthetic screen gives its first frame only after the configured delay and then one
    /// per change; pulls sleep on the clock.
    #[test]
    fn screen_changes_on_the_clock() {
        let c = ManualClock::new();
        let mut s = SyntheticScreen::new(c.clone())
            .first_frame_after(Duration::from_millis(30))
            .change_every(Duration::from_millis(10));
        assert_eq!(
            s.pull(Duration::from_millis(20)).unwrap(),
            Pulled::Unchanged
        );
        assert!(!s.has_frame());
        assert_eq!(s.pull(Duration::from_millis(50)).unwrap(), Pulled::Frame);
        assert!(s.has_frame());
        assert_eq!(s.pull(Duration::from_millis(3)).unwrap(), Pulled::Unchanged);
        assert_eq!(s.pull(Duration::from_millis(50)).unwrap(), Pulled::Frame);
        assert_eq!(s.frame_for_slot(9).unwrap().slot, 9);
    }

    /// The synthetic audio source delivers contiguous 10 ms packets that are at least 10 ms old,
    /// starting 10 ms before "now" when opened.
    #[test]
    fn audio_packets() {
        let c = ManualClock::with_frequency(10_000_000, 0);
        let mut a = SyntheticAudio::new(AudioKind::System, Tone::Constant(0.5), c.clone());
        a.open().unwrap();
        let mut v = Vec::new();
        a.read(&mut v).unwrap();
        assert!(v.is_empty());
        c.advance_ms(100);
        a.read(&mut v).unwrap();
        // from −480 (10 ms before opening) up to packets ending 10 ms before now (4800 − 480)
        assert_eq!(v.len(), 10);
        assert!(
            v.windows(2)
                .all(|w| w[1].time_hns - w[0].time_hns == 100_000)
        );
        assert!(v.iter().all(|p| p.frames() == 480 && p.data[0] == 0.5));
    }

    /// `check_sync` counts future content and lags per expanded frame.
    #[test]
    fn vidcheck_counts() {
        let w = |slot, slots, content| WrittenVideo {
            time: VideoSampleTime::new(slot, slots, 30),
            frame: SynthFrame {
                slot: content,
                change: 0,
            },
        };
        let r = check_sync(&[w(0, 1, 0), w(1, 3, 1), w(4, 1, 5)], 30);
        assert_eq!(r.frames, 5);
        assert_eq!(r.lag_max, 2);
        assert_eq!(r.sync_bad, 1);
        assert_eq!(r.gaps, 0);
        assert_eq!(r.duration_hns, crate::cfr::slot_time(5, 30));
    }
}
