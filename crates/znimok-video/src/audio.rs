//! The audio timeline (§3, §7 items 41–44; `VidAudMix`, `VidAudPoll`, the audio parts of
//! `VidRecord` in LH).
//!
//! - **Format**: the OS is asked for 48 kHz stereo float right away — there is no resampler.
//! - **Scale**: an audio frame index counted from an anchor (the clock at audio start). Every
//!   packet goes where ITS timestamp says, not where a sample counter would put it: a silent
//!   loopback (no packets at all when nothing plays) becomes silence by itself; a gap or drift
//!   of more than 480 frames (10 ms) puts the packet where it really is ([`PacketPlacer`]).
//!   Packets flagged silent are not added.
//! - **Buffer** ([`AudioBuffer`]): stereo float from `base`; sources ADD (sum); what arrives late
//!   for audio that was already taken is dropped; capped at 120 s.
//! - **Lag**: audio is written 300 ms behind "now", in chunks of at least 480 frames — packets
//!   come late ([`AudioTimeline::ripe`]).
//! - **Sync with video**: audio zero = the first video frame; everything before is dropped. The
//!   sample time of audio = index from zero minus what pauses dropped. The tail ends exactly at
//!   the end of the video: `idx0 + shift + k·48000/fps`.
//! - **Pause**: what was ripe before the pause is written, what was recorded during it is
//!   dropped, all later indices are shifted by the same amount.
//! - **Mix**: one track = sum of sources with a soft limiter — `|x| > 0.8 → 0.8 + 0.2·tanh((|x|−0.8)/0.2)`
//!   ([`soft_limit`]). Znimok keeps sources as separate tracks in the project and mixes at export
//!   (PLAN decision 16); the same limiter is used for both.

use crate::HNS_PER_SEC;

/// Sample rate of every track, Hz.
pub const RATE: i64 = 48_000;
/// Channels (interleaved stereo).
pub const CHANNELS: usize = 2;
/// Audio is written this many frames behind "now" (300 ms): packets arrive late.
pub const LAG: i64 = RATE * 3 / 10;
/// A packet more than this many frames (10 ms) away from where the previous one ended is put
/// where its timestamp says (gap or clock drift).
pub const RESYNC: i64 = 480;
/// Audio is taken in chunks larger than this (10 ms).
pub const MIN_CHUNK: i64 = 480;
/// The mix buffer does not grow beyond 120 s.
pub const BUFFER_CAP_FRAMES: i64 = RATE * 120;
/// A lost device is reopened this often, ms.
pub const REOPEN_MS: u64 = 500;
/// Sources are polled this often, ms (the audio thread of LH).
pub const POLL_MS: u64 = 10;
/// Fade length at cut joints on export, frames (10 ms).
pub const FADE: i64 = RATE / 100;

/// Audio frame index of a moment, from the anchor: `(t − anchor)·48000/1e7` (both in 100 ns).
pub fn index_of(t_hns: i64, anchor_hns: i64) -> i64 {
    ((t_hns - anchor_hns) as i128 * RATE as i128 / HNS_PER_SEC as i128) as i64
}

/// Where one written audio sample sits on the output timeline (`VidEmitAudio`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioSampleTime {
    /// Index of the first frame from audio zero, pauses removed.
    pub index: i64,
    /// Frames in the sample.
    pub frames: i64,
    /// `index·1e7/48000`, 100 ns.
    pub time: i64,
    /// `(index+frames)·1e7/48000 − time`.
    pub duration: i64,
}

impl AudioSampleTime {
    pub fn new(index: i64, frames: i64) -> Self {
        let t = |i: i64| ((i as i128) * HNS_PER_SEC as i128 / RATE as i128) as i64;
        Self {
            index,
            frames,
            time: t(index),
            duration: t(index + frames) - t(index),
        }
    }
}

/// The soft limiter: identity up to 0.8, then `0.8 + 0.2·tanh((|x|−0.8)/0.2)` — never reaches 1.
pub fn soft_limit(x: f32) -> f32 {
    let ax = x.abs();
    if ax > 0.8 {
        x.signum() * (0.8 + 0.2 * ((ax - 0.8) / 0.2).tanh())
    } else {
        x
    }
}

/// Float → PCM s16 with the limiter (`(short)(x·32767)`, truncating).
pub fn to_s16(x: f32) -> i16 {
    (soft_limit(x) * 32767.0) as i16
}

/// Mix tracks of PCM s16 into one: sum as float, limit, back to s16 (§7 item 44: players and
/// messengers play only the first track). Tracks may differ in length; the result is as long as
/// the longest, missing samples are silence.
pub fn mix_tracks_s16(tracks: &[&[i16]]) -> Vec<i16> {
    let len = tracks.iter().map(|t| t.len()).max().unwrap_or(0);
    (0..len)
        .map(|i| {
            let s: f32 = tracks
                .iter()
                .map(|t| t.get(i).map_or(0.0, |&v| v as f32 / 32767.0))
                .sum();
            to_s16(s)
        })
        .collect()
}

/// Stereo float buffer on the audio timeline (`VidAudMix`). Several sources may add into the
/// same buffer (one mixed track) or each have its own (separate tracks).
#[derive(Clone, Debug, Default)]
pub struct AudioBuffer {
    base: i64,
    buf: Vec<f32>,
}

impl AudioBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Index of the first frame not yet taken.
    pub fn base(&self) -> i64 {
        self.base
    }

    /// Frames buffered after `base`.
    pub fn len(&self) -> i64 {
        (self.buf.len() / CHANNELS) as i64
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Add `data` (interleaved stereo) at frame `idx`. The part before `base` (already taken) is
    /// dropped. If the buffer would have to grow to 120 s or more, the packet is dropped whole.
    pub fn add(&mut self, idx: i64, data: &[f32]) {
        let n = (data.len() / CHANNELS) as i64;
        let (mut from, mut skip) = (idx, 0);
        if from < self.base {
            skip = self.base - from;
            from = self.base;
        }
        if n <= skip {
            return;
        }
        let need = ((from - self.base + n - skip) * CHANNELS as i64) as usize;
        if self.buf.len() < need && need < (BUFFER_CAP_FRAMES * CHANNELS as i64) as usize {
            self.buf.resize(need, 0.0);
        }
        if self.buf.len() >= need {
            let at = ((from - self.base) * CHANNELS as i64) as usize;
            let src = &data[skip as usize * CHANNELS..n as usize * CHANNELS];
            for (q, p) in self.buf[at..at + src.len()].iter_mut().zip(src) {
                *q += *p;
            }
        }
    }

    /// Take `[base, upto)` as float (silence where nothing was added) and move `base` to `upto`.
    /// `upto ≤ base` takes nothing.
    pub fn take_f32(&mut self, upto: i64) -> Vec<f32> {
        let n = upto - self.base;
        if n <= 0 {
            return Vec::new();
        }
        let want = n as usize * CHANNELS;
        let mut out = vec![0.0; want];
        let have = want.min(self.buf.len());
        out[..have].copy_from_slice(&self.buf[..have]);
        self.buf.drain(..have);
        self.base = upto;
        out
    }

    /// Take `[base, upto)` as PCM s16 through the soft limiter (`VidAudMix::Take`).
    pub fn take_s16(&mut self, upto: i64) -> Vec<i16> {
        self.take_f32(upto).into_iter().map(to_s16).collect()
    }
}

/// Where the packets of one source go on the timeline (`VidAudSrc::next` in `VidAudPoll`):
/// normally right after the previous packet, so small jitter of timestamps does not tear the
/// sound; when the packet's own stamp is more than 480 frames away (a gap — nothing was
/// playing — or the sound card's clock drifted), it goes where the stamp says.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PacketPlacer {
    next: Option<i64>,
}

impl PacketPlacer {
    /// Forget the position (the device was reopened).
    pub fn reset(&mut self) {
        self.next = None;
    }

    /// Index for a packet of `frames` whose timestamp says `idx`.
    pub fn place(&mut self, idx: i64, frames: i64) -> i64 {
        let at = match self.next {
            Some(n) if (idx - n).abs() <= RESYNC => n,
            _ => idx,
        };
        self.next = Some(at + frames);
        at
    }

    pub fn next(&self) -> Option<i64> {
        self.next
    }
}

/// The timeline as the recorder currently sees it, published for observers that need it —
/// the synthetic frame-coded tone (`VidAudJob::t0Idx`/`synthShift` in LH), level meters.
#[derive(Debug)]
pub struct TimelineProbe {
    anchor_hns: std::sync::atomic::AtomicI64,
    zero: std::sync::atomic::AtomicI64,
    shift: std::sync::atomic::AtomicI64,
}

impl Default for TimelineProbe {
    fn default() -> Self {
        use std::sync::atomic::AtomicI64;
        Self {
            anchor_hns: AtomicI64::new(0),
            zero: AtomicI64::new(-1),
            shift: AtomicI64::new(0),
        }
    }
}

impl TimelineProbe {
    pub fn set_anchor(&self, hns: i64) {
        self.anchor_hns
            .store(hns, std::sync::atomic::Ordering::SeqCst);
    }
    pub fn set_zero(&self, idx: i64) {
        self.zero.store(idx, std::sync::atomic::Ordering::SeqCst);
    }
    pub fn set_shift(&self, frames: i64) {
        self.shift
            .store(frames, std::sync::atomic::Ordering::SeqCst);
    }
    /// Clock time of timeline index 0, 100 ns.
    pub fn anchor_hns(&self) -> i64 {
        self.anchor_hns.load(std::sync::atomic::Ordering::SeqCst)
    }
    /// Index of the first video frame, `None` before it.
    pub fn zero(&self) -> Option<i64> {
        let z = self.zero.load(std::sync::atomic::Ordering::SeqCst);
        (z >= 0).then_some(z)
    }
    /// Frames removed by pauses so far.
    pub fn shift(&self) -> i64 {
        self.shift.load(std::sync::atomic::Ordering::SeqCst)
    }
}

/// What the recorder does with the audio buffers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioStep {
    /// Take `[base, upto)` from every buffer and write it at `sample` on the output timeline.
    Emit { upto: i64, sample: AudioSampleTime },
    /// Take `[base, upto)` and throw it away (before the first frame, during a pause).
    Discard { upto: i64 },
}

/// The audio side of one recording: which part of the timeline has been written, and how much
/// pauses have removed (`aIdx0`, `aw`, `aShift`, `aPauseIdx` in `VidRecord`).
#[derive(Clone, Debug, Default)]
pub struct AudioTimeline {
    idx0: Option<i64>,
    written: i64,
    shift: i64,
    pause_idx: Option<i64>,
}

impl AudioTimeline {
    pub fn new() -> Self {
        Self::default()
    }

    /// Index of the first video frame (audio zero), once started.
    pub fn zero(&self) -> Option<i64> {
        self.idx0
    }

    /// Frames removed by pauses so far.
    pub fn shift(&self) -> i64 {
        self.shift
    }

    /// Index up to which audio has been taken (`aw`).
    pub fn written(&self) -> i64 {
        self.written
    }

    fn emit(&mut self, upto: i64) -> AudioStep {
        let idx0 = self.idx0.unwrap_or(0);
        let s = AudioSampleTime::new(self.written - idx0 - self.shift, upto - self.written);
        self.written = upto;
        AudioStep::Emit { upto, sample: s }
    }

    /// The first video frame is at `idx0`: audio zero. Everything before it is dropped.
    pub fn start(&mut self, idx0: i64) -> AudioStep {
        self.idx0 = Some(idx0);
        self.written = idx0;
        AudioStep::Discard { upto: idx0 }
    }

    /// While recording: write what is ripe — older than [`LAG`] — once there is more than
    /// [`MIN_CHUNK`] of it. `now_idx` is the timeline index of "now".
    pub fn ripe(&mut self, now_idx: i64) -> Option<AudioStep> {
        self.idx0?;
        let upto = now_idx - LAG;
        (upto > self.written + MIN_CHUNK).then(|| self.emit(upto))
    }

    /// Pause at `now_idx`.
    pub fn pause(&mut self, now_idx: i64) {
        if self.idx0.is_some() {
            self.pause_idx = Some(now_idx);
        }
    }

    /// While paused: audio from before the pause is still ripening — write it, but never past
    /// the pause.
    pub fn ripe_paused(&mut self, now_idx: i64) -> Option<AudioStep> {
        let p = self.pause_idx?;
        let upto = (now_idx - LAG).min(p);
        (upto > self.written + MIN_CHUNK).then(|| self.emit(upto))
    }

    /// Resume at `now_idx`: the rest of the audio before the pause is written (even if not ripe
    /// yet), what was recorded during the pause is dropped, and every later index shifts by the
    /// dropped amount.
    pub fn resume(&mut self, now_idx: i64) -> Vec<AudioStep> {
        let Some(p) = self.pause_idx.take() else {
            return Vec::new();
        };
        let mut steps = Vec::with_capacity(2);
        if p > self.written {
            steps.push(self.emit(p));
        }
        steps.push(AudioStep::Discard { upto: now_idx });
        if now_idx > self.written {
            self.shift += now_idx - self.written;
            self.written = now_idx;
        }
        steps
    }

    /// Stop after `frames` video frames at `fps`: the tail ends exactly at the end of the video,
    /// `idx0 + shift + frames·48000/fps`. `None` when nothing is left or time never started.
    pub fn finish(&mut self, frames: i64, fps: i64) -> Option<AudioStep> {
        let idx0 = self.idx0?;
        let end = idx0 + self.shift + ((frames as i128) * RATE as i128 / fps as i128) as i64;
        self.pause_idx = None;
        (end > self.written).then(|| self.emit(end))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stereo(n: usize, v: f32) -> Vec<f32> {
        vec![v; n * CHANNELS]
    }

    /// §3 / §7 item 44: sources ADD into the buffer; the sum goes through the soft limiter.
    #[test]
    fn sources_add() {
        let mut b = AudioBuffer::new();
        b.add(0, &stereo(10, 0.25));
        b.add(5, &stereo(10, 0.25));
        let out = b.take_f32(15);
        assert_eq!(out.len(), 30);
        assert_eq!(out[0], 0.25);
        assert_eq!(out[5 * 2], 0.5);
        assert_eq!(out[10 * 2], 0.25);
        assert_eq!(b.base(), 15);
    }

    /// §3: what arrives late for audio already taken is dropped; the part after `base` stays.
    #[test]
    fn late_part_is_dropped() {
        let mut b = AudioBuffer::new();
        b.take_f32(100);
        b.add(90, &stereo(20, 0.5));
        let out = b.take_f32(120);
        assert!(out[..20].iter().all(|&x| x == 0.5), "100..110 kept");
        assert!(out[20..].iter().all(|&x| x == 0.0));
        b.add(0, &stereo(50, 1.0));
        assert!(b.is_empty(), "entirely late — nothing");
    }

    /// §3: the buffer never grows to 120 s; a packet that would need it is dropped whole.
    #[test]
    fn buffer_cap_120_s() {
        let mut b = AudioBuffer::new();
        b.add(BUFFER_CAP_FRAMES - 10, &stereo(20, 0.5));
        assert!(b.is_empty());
        b.add(BUFFER_CAP_FRAMES - 30, &stereo(20, 0.5));
        assert_eq!(b.len(), BUFFER_CAP_FRAMES - 10);
    }

    /// §3: taking past the buffered data gives silence; taking backwards does nothing.
    #[test]
    fn take_silence_and_backwards() {
        let mut b = AudioBuffer::new();
        b.add(0, &stereo(4, 0.5));
        assert_eq!(b.take_s16(10).len(), 20);
        assert!(b.take_s16(5).is_empty());
        assert_eq!(b.base(), 10);
    }

    /// §3 / §7 item 44: limiter is the identity up to 0.8, soft above, never reaches full scale;
    /// s16 is truncated `x·32767`.
    #[test]
    fn soft_limiter() {
        assert_eq!(soft_limit(0.5), 0.5);
        assert_eq!(soft_limit(-0.8), -0.8);
        let a = soft_limit(1.0);
        assert!((a - (0.8 + 0.2 * 1f32.tanh())).abs() < 1e-6);
        assert!(soft_limit(10.0) <= 1.0 && soft_limit(10.0) > 0.99);
        assert_eq!(soft_limit(-1.0), -a);
        assert_eq!(to_s16(0.5), 16383);
        assert_eq!(to_s16(-0.5), -16383);
        assert!(to_s16(2.0) < i16::MAX);
    }

    /// §7 item 44 / PLAN decision 16: two tracks at export sum and go through the same limiter.
    #[test]
    fn mix_tracks() {
        let a = [16383i16, 16383, 0, 100];
        let b = [16383i16, -16383];
        let m = mix_tracks_s16(&[&a, &b]);
        assert_eq!(m.len(), 4);
        assert_eq!(m[1], 0);
        assert_eq!(m[0], to_s16(2.0 * 16383.0 / 32767.0));
        assert!(m[0] < 32767 && m[0] > 26214, "limited, not clipped");
        assert_eq!(m[3], to_s16(100.0 / 32767.0));
    }

    /// §7 item 41: packets follow each other while their stamps are within 10 ms of where the
    /// previous ended (jitter absorbed); a jump over 480 frames resyncs to the stamp.
    #[test]
    fn placer_resyncs_over_10ms() {
        let mut p = PacketPlacer::default();
        assert_eq!(p.place(1000, 480), 1000);
        assert_eq!(
            p.place(1480 + 300, 480),
            1480,
            "jitter within 480 — contiguous"
        );
        assert_eq!(
            p.place(1960 - 480, 480),
            1960,
            "exactly 480 early — still contiguous"
        );
        assert_eq!(p.place(2440 + 481, 480), 2921, "481 late — resync");
        assert_eq!(
            p.place(3401 - 481, 480),
            2920,
            "481 early (drift) — resync back"
        );
        p.reset();
        assert_eq!(p.place(99_999, 10), 99_999);
    }

    /// §3: index of a timestamp from the anchor, 48 kHz.
    #[test]
    fn index_from_anchor() {
        assert_eq!(index_of(5_000_000 + 10_000_000, 5_000_000), 48_000);
        assert_eq!(index_of(5_000_000 + 208, 5_000_000), 0);
        assert_eq!(index_of(5_000_000 + 209, 5_000_000), 1);
    }

    /// §3: sample time/duration of audio = frames at 48 kHz in 100 ns, durations add up.
    #[test]
    fn audio_sample_time() {
        let a = AudioSampleTime::new(0, 480);
        assert_eq!((a.time, a.duration), (0, 100_000));
        let b = AudioSampleTime::new(1, 1);
        let c = AudioSampleTime::new(2, 1);
        assert_eq!(b.time + b.duration, c.time);
    }

    /// §3 / §7 item 42: before audio zero everything is dropped; afterwards only what is older
    /// than 300 ms is written, and only in chunks over 480 frames.
    #[test]
    fn timeline_zero_lag_and_chunks() {
        let mut t = AudioTimeline::new();
        assert_eq!(t.ripe(1_000_000), None, "not started");
        assert_eq!(t.start(1000), AudioStep::Discard { upto: 1000 });
        assert_eq!(t.ripe(1000 + LAG + 480), None, "exactly 480 — wait");
        let s = t.ripe(1000 + LAG + 481).unwrap();
        assert_eq!(
            s,
            AudioStep::Emit {
                upto: 1481,
                sample: AudioSampleTime::new(0, 481)
            }
        );
        let s = t.ripe(1481 + LAG + 1000).unwrap();
        assert!(matches!(s, AudioStep::Emit { sample, .. } if sample.index == 481));
    }

    /// §2.4 / §3: pause — ripe audio before the pause is still written while paused (never past
    /// the pause), on resume the rest before it is written, the pause is discarded and later
    /// indices shift by it: the output timeline has no hole.
    #[test]
    fn timeline_pause() {
        let mut t = AudioTimeline::new();
        t.start(0);
        t.ripe(20_000 + LAG);
        assert_eq!(t.written(), 20_000);
        t.pause(30_000);
        // while paused: ripe capped at the pause
        let s = t.ripe_paused(26_000 + LAG).unwrap();
        assert!(matches!(s, AudioStep::Emit { upto: 26_000, .. }));
        let s = t.ripe_paused(90_000 + LAG).unwrap();
        assert!(matches!(s, AudioStep::Emit { upto: 30_000, .. }));
        assert_eq!(t.ripe_paused(99_000 + LAG), None, "nothing past the pause");
        let steps = t.resume(100_000);
        assert_eq!(steps, vec![AudioStep::Discard { upto: 100_000 }]);
        assert_eq!(t.shift(), 70_000);
        let s = t.ripe(100_000 + LAG + 4800).unwrap();
        assert!(
            matches!(s, AudioStep::Emit { sample, .. } if sample.index == 30_000),
            "continues right after the pre-pause audio"
        );
    }

    /// §2.4: a short pause (under the 300 ms lag) — on resume the not-yet-ripe audio before the
    /// pause is written at once, then the pause is discarded.
    #[test]
    fn timeline_short_pause_flushes_pre_pause_audio() {
        let mut t = AudioTimeline::new();
        t.start(0);
        t.pause(10_000);
        let steps = t.resume(12_000);
        assert_eq!(steps.len(), 2);
        assert!(
            matches!(steps[0], AudioStep::Emit { upto: 10_000, sample } if sample.index == 0 && sample.frames == 10_000)
        );
        assert_eq!(steps[1], AudioStep::Discard { upto: 12_000 });
        assert_eq!(t.shift(), 2000);
    }

    /// §3: the tail ends exactly at the end of the video — `idx0 + shift + k·48000/fps`.
    #[test]
    fn timeline_finish_matches_video_end() {
        let mut t = AudioTimeline::new();
        t.start(5000);
        t.pause(6000);
        t.resume(9000);
        let s = t.finish(90, 30).unwrap(); // 3 s of video
        match s {
            AudioStep::Emit { upto, sample } => {
                assert_eq!(upto, 5000 + 3000 + 144_000);
                assert_eq!(sample.index + sample.frames, 144_000);
            }
            _ => panic!(),
        }
        assert_eq!(t.finish(90, 30), None, "already at the end");
        assert_eq!(AudioTimeline::new().finish(90, 30), None, "never started");
    }
}
