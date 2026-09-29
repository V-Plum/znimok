//! Constant frame rate through sample **duration** (§2.3, §7 items 1 and 18; `VidRecord` +
//! `VidEmit` in LH).
//!
//! - Time starts at the first REAL frame: `t0` = clock at that moment; before it no time passes.
//! - Slot `k` is due at `t0 + k·f/fps`. When it is due, `n = (now − t0)·fps/f − k + 1` slots have
//!   come (at least 1), and ONE sample is written: time `k·1e7/fps`, duration
//!   `(k+n)·1e7/fps − k·1e7/fps`. The muxer turns the long duration into repeats; the file has an
//!   even timeline.
//! - MP4 counts time by sample DURATIONS, not by timestamps: a skipped slot silently shortens the
//!   video and audio/log drift apart. Hence never skip — stretch.
//! - Cursor and clicks are computed for the slot's moment `t0 + k·f/fps`, not for "now".
//! - Pause shifts `t0` by the paused time ([`Cfr::resume`]), so the timeline stays continuous.

use crate::HNS_PER_SEC;

/// Longest single wait for the next slot (`VidPull` with a timeout of at most 50 ms).
pub const MAX_WAIT_MS: u32 = 50;

/// Where one written video sample sits on the output timeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VideoSampleTime {
    /// First slot the sample covers (`k`).
    pub slot: i64,
    /// How many slots it covers (`n ≥ 1`; more than 1 when the loop was late).
    pub slots: i64,
    /// Sample time, 100 ns: `k·1e7/fps`.
    pub time: i64,
    /// Sample duration, 100 ns: `(k+n)·1e7/fps − k·1e7/fps` — the difference of two truncated
    /// times, so durations of consecutive samples add up exactly to the end time (no drift).
    pub duration: i64,
}

impl VideoSampleTime {
    pub fn new(slot: i64, slots: i64, fps: i64) -> Self {
        Self {
            slot,
            slots,
            time: slot_time(slot, fps),
            duration: slot_time(slot + slots, fps) - slot_time(slot, fps),
        }
    }

    /// End time of the sample, 100 ns.
    pub fn end(&self) -> i64 {
        self.time + self.duration
    }

    /// The same sample as `n` one-slot samples — for a sink that places frames by timestamp and
    /// ignores durations (AVAssetWriter, see [`crate::traits::SinkCaps::carries_duration`]).
    pub fn expand(&self, fps: i64) -> impl Iterator<Item = VideoSampleTime> + use<> {
        let s = self.slot;
        (0..self.slots).map(move |i| VideoSampleTime::new(s + i, 1, fps))
    }
}

/// Time of slot `k`, 100 ns, truncated: `k·1e7/fps`.
pub fn slot_time(k: i64, fps: i64) -> i64 {
    ((k as i128) * HNS_PER_SEC as i128 / fps as i128) as i64
}

/// What the loop should do now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    /// No real frame yet — time has not started.
    NotStarted,
    /// Paused: pull the source to keep it alive, write nothing.
    Paused,
    /// The next slot is in the future: wait for the source at most `wait_ms` (1..=50).
    Wait { wait_ms: u32 },
    /// Slot `k` is due and `n` slots have come: write one sample covering them.
    Due(VideoSampleTime),
}

/// The frame clock of one recording.
#[derive(Clone, Debug)]
pub struct Cfr {
    fps: i64,
    f: i64,
    t0: Option<i64>,
    k: i64,
    paused_at: Option<i64>,
}

impl Cfr {
    /// `frequency` — ticks per second of the clock the loop reads.
    pub fn new(fps: u32, frequency: i64) -> Self {
        assert!(fps > 0 && frequency > 0);
        Self {
            fps: fps as i64,
            f: frequency,
            t0: None,
            k: 0,
            paused_at: None,
        }
    }

    pub fn fps(&self) -> i64 {
        self.fps
    }

    /// The first real frame arrived at `now`: time starts. Later calls do nothing — a source
    /// that re-creates its pool (WGC window resized, §7 item 32) must not restart time.
    pub fn start(&mut self, now: i64) {
        if self.t0.is_none() {
            self.t0 = Some(now);
        }
    }

    pub fn started(&self) -> bool {
        self.t0.is_some()
    }

    /// Tick of the first frame, shifted by every pause so far.
    pub fn t0(&self) -> Option<i64> {
        self.t0
    }

    pub fn paused(&self) -> bool {
        self.paused_at.is_some()
    }

    /// Slots written so far (`k`): the next sample starts here.
    pub fn frames(&self) -> i64 {
        self.k
    }

    /// Tick at which slot `k` is due: `t0 + k·f/fps`. Cursor and clicks are drawn for this
    /// moment (§2.3).
    pub fn slot_tick(&self, k: i64) -> Option<i64> {
        self.t0
            .map(|t0| t0 + ((k as i128) * self.f as i128 / self.fps as i128) as i64)
    }

    /// What to do at `now`.
    pub fn poll(&self, now: i64) -> Slot {
        let Some(t0) = self.t0 else {
            return Slot::NotStarted;
        };
        if self.paused_at.is_some() {
            return Slot::Paused;
        }
        let due = t0 + ((self.k as i128) * self.f as i128 / self.fps as i128) as i64;
        if now < due {
            let ms = ((due - now) as i128 * 1000 / self.f as i128) as i64;
            return Slot::Wait {
                wait_ms: ms.clamp(1, MAX_WAIT_MS as i64) as u32,
            };
        }
        // How many slots have come, including k.
        let n =
            (((now - t0) as i128 * self.fps as i128 / self.f as i128) as i64 - self.k + 1).max(1);
        Slot::Due(VideoSampleTime::new(self.k, n, self.fps))
    }

    /// The sample returned by [`Cfr::poll`] was written: move past it. Not called when the write
    /// failed — the recording stops there.
    pub fn commit(&mut self, s: &VideoSampleTime) {
        debug_assert_eq!(s.slot, self.k);
        self.k += s.slots;
    }

    /// Pause at `now`. Ignored before the first frame (LH reads the pause flag only once time
    /// has started) and when already paused.
    pub fn pause(&mut self, now: i64) {
        if self.t0.is_some() && self.paused_at.is_none() {
            self.paused_at = Some(now);
        }
    }

    /// Resume at `now`: `t0 += now − pause start`, so slot `k` is due right away and the paused
    /// time never reaches the file — no gap, no frozen frame. Returns the paused ticks
    /// (0 when not paused).
    pub fn resume(&mut self, now: i64) -> i64 {
        match (self.paused_at.take(), self.t0.as_mut()) {
            (Some(p), Some(t0)) => {
                let d = now - p;
                *t0 += d;
                d
            }
            _ => 0,
        }
    }

    /// Duration of what has been written, 100 ns (`k·1e7/fps`).
    pub fn duration_hns(&self) -> i64 {
        slot_time(self.k, self.fps)
    }

    /// Duration in ms as LH reports it (`k·1000/fps`, not rounded).
    pub fn duration_ms(&self) -> f64 {
        self.k as f64 * 1000.0 / self.fps as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const F: i64 = 10_000_000;

    fn due(s: Slot) -> VideoSampleTime {
        match s {
            Slot::Due(t) => t,
            other => panic!("expected Due, got {other:?}"),
        }
    }

    /// §2.3: before the first real frame no time passes.
    #[test]
    fn nothing_before_first_frame() {
        let c = Cfr::new(30, F);
        assert_eq!(c.poll(1_000_000_000), Slot::NotStarted);
        assert_eq!(c.slot_tick(5), None);
    }

    /// §2.3: slot 0 is due at t0 exactly; slot k at `t0 + k·f/fps`; before that, a wait of at
    /// most 50 ms and at least 1 ms.
    #[test]
    fn slots_are_due_at_t0_plus_k_over_fps() {
        let mut c = Cfr::new(30, F);
        let t0 = 5_000_000;
        c.start(t0);
        let s = due(c.poll(t0));
        assert_eq!((s.slot, s.slots, s.time, s.duration), (0, 1, 0, 333_333));
        c.commit(&s);
        assert_eq!(c.slot_tick(1), Some(t0 + 333_333));
        assert_eq!(c.poll(t0 + 100_000), Slot::Wait { wait_ms: 23 });
        assert_eq!(
            c.poll(t0 + 333_300),
            Slot::Wait { wait_ms: 1 },
            "never 0 ms"
        );
        let mut far = Cfr::new(1, F);
        far.start(0);
        far.commit(&due(far.poll(0)));
        assert_eq!(far.poll(0), Slot::Wait { wait_ms: 50 }, "capped at 50 ms");
    }

    /// §7 item 1 (and `VidEmit` 31410–31411): a late loop writes ONE sample whose duration
    /// covers every slot that has come, instead of skipping them — the file keeps real length.
    #[test]
    fn late_loop_writes_one_long_sample_not_a_gap() {
        let mut c = Cfr::new(30, F);
        c.start(0);
        c.commit(&due(c.poll(0)));
        // 150 ms late: slots 1..=4 have come (slot 4 at 133.3 ms, slot 5 at 166.7 ms)
        let s = due(c.poll(1_500_000));
        assert_eq!((s.slot, s.slots), (1, 4));
        assert_eq!(s.time, 333_333);
        assert_eq!(s.end(), slot_time(5, 30));
        c.commit(&s);
        assert_eq!(c.frames(), 5);
        assert!(matches!(c.poll(1_500_001), Slot::Wait { .. }));
    }

    /// §2.3: durations are differences of truncated slot times, so the sum of any run of samples
    /// ends exactly at `k·1e7/fps` — 30 fps (non-integer 333 333.3 hns) never drifts.
    #[test]
    fn durations_add_up_without_drift() {
        let mut c = Cfr::new(30, F);
        c.start(0);
        let mut now = 0;
        let mut total = 0;
        let mut last_end = 0;
        for step in 0..10_000 {
            now += [333_333, 400_000, 1_000_000, 10][step % 4];
            if let Slot::Due(s) = c.poll(now) {
                assert_eq!(s.time, last_end, "no gap, no overlap");
                total += s.duration;
                last_end = s.end();
                c.commit(&s);
            }
        }
        assert_eq!(total, slot_time(c.frames(), 30));
        assert_eq!(c.duration_hns(), total);
    }

    /// §2.4 CAPS-101: pause shifts t0 by its duration; the first slot after resume is due at
    /// once and continues the numbering — no gap and no frozen frame.
    #[test]
    fn pause_shifts_t0_and_the_timeline_is_continuous() {
        let mut c = Cfr::new(30, F);
        c.start(0);
        c.commit(&due(c.poll(0)));
        c.commit(&due(c.poll(333_334)));
        c.pause(500_000);
        assert_eq!(c.poll(9_000_000), Slot::Paused);
        assert_eq!(c.resume(20_500_000), 20_000_000);
        assert_eq!(c.t0(), Some(20_000_000));
        // slot 2 was due at 666 666 before the pause → now at 20 666 666
        assert!(matches!(c.poll(20_500_000), Slot::Wait { .. }));
        let s = due(c.poll(20_666_666));
        assert_eq!((s.slot, s.slots, s.time), (2, 1, 666_666));
    }

    /// §2.4: pause before the first frame is not a pause (LH reads the flag only after time has
    /// started); a second pause/resume is harmless.
    #[test]
    fn pause_edges() {
        let mut c = Cfr::new(60, F);
        c.pause(10);
        assert!(!c.paused());
        assert_eq!(c.resume(100), 0);
        c.start(1000);
        c.pause(2000);
        c.pause(5000); // already paused: keeps the first moment
        assert_eq!(c.resume(7000), 5000);
        assert_eq!(c.resume(9000), 0, "not paused any more");
        assert_eq!(c.t0(), Some(6000));
    }

    /// §7 item 32: a second `start` (source re-created its pool) must not restart time.
    #[test]
    fn start_is_once() {
        let mut c = Cfr::new(30, F);
        c.start(100);
        c.start(9_999);
        assert_eq!(c.t0(), Some(100));
    }

    /// §7 item 18 / AVAssetWriter note in §8: a stretched sample expands into per-slot samples
    /// with the same boundaries.
    #[test]
    fn expand_keeps_boundaries() {
        let s = VideoSampleTime::new(7, 3, 30);
        let v: Vec<_> = s.expand(30).collect();
        assert_eq!(v.len(), 3);
        assert_eq!(v[0].time, s.time);
        assert_eq!(v[2].end(), s.end());
        assert!(v.windows(2).all(|w| w[0].end() == w[1].time));
    }

    /// Duration in ms is `k·1000/fps` (LH `durMs`).
    #[test]
    fn duration_ms() {
        let mut c = Cfr::new(30, F);
        c.start(0);
        c.commit(&due(c.poll(F)));
        assert_eq!(c.frames(), 31);
        assert!((c.duration_ms() - 1033.333).abs() < 0.001);
    }
}
