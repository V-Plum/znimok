//! Pauses in wall-clock time (CAPS-101 + CAPS-83, §2.4 and §5.5).
//!
//! The recording loop shifts `t0` on resume ([`crate::cfr::Cfr::resume`]), so video time has no
//! pauses in it. Events stamped with the WALL clock (the browser log: CDP timestamps, `Date.now()`)
//! need the pauses to land on the same timeline; the loop records each pause as
//! `{wall_from, wall_to}` — also when the recording is stopped while paused.

/// One pause, wall clock ms since the epoch, `[from, to)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PauseSpan {
    pub from_ms: f64,
    pub to_ms: f64,
}

impl PauseSpan {
    pub fn duration_ms(&self) -> f64 {
        self.to_ms - self.from_ms
    }
}

/// How far before the first frame a context event (tab / navigation / info) may be and still be
/// kept, put at 0 — it tells which page the recording starts on.
pub const CONTEXT_BEFORE_MS: f64 = 5000.0;
/// Events up to this long after the last frame still belong to the video.
pub const AFTER_END_MS: f64 = 250.0;

/// Pauses of one recording, in order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PauseLog {
    pub spans: Vec<PauseSpan>,
}

impl PauseLog {
    pub fn push(&mut self, from_ms: f64, to_ms: f64) {
        self.spans.push(PauseSpan { from_ms, to_ms });
    }

    /// Total paused wall time, ms.
    pub fn total_ms(&self) -> f64 {
        self.spans.iter().map(PauseSpan::duration_ms).sum()
    }

    /// Where a wall-clock event at `t` lands in the video, ms (rounded), or `None` when it is
    /// dropped (`DevBuildBlob`):
    /// - before the first frame (`wall0`, with 1 ms of slack) — dropped, except a context event
    ///   (`context`: `k` is `tab`, `nav` or `info`) not older than 5 s, which goes to 0;
    /// - inside a pause — dropped;
    /// - after each earlier pause — shifted back by its duration;
    /// - more than 250 ms after the end (`duration_ms`) — dropped.
    pub fn video_ms(&self, t: f64, wall0: f64, duration_ms: f64, context: bool) -> Option<i32> {
        if wall0.is_nan() || wall0 <= 0.0 {
            return None;
        }
        if t < wall0 - 1.0 && !(context && wall0 - t < CONTEXT_BEFORE_MS) {
            return None;
        }
        let mut shift = 0.0;
        for p in &self.spans {
            if t >= p.from_ms && t < p.to_ms {
                return None;
            }
            if t >= p.to_ms {
                shift += p.to_ms - p.from_ms;
            }
        }
        let vt = (t - wall0 - shift).max(0.0);
        if vt > duration_ms + AFTER_END_MS {
            return None;
        }
        Some((vt + 0.5) as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W0: f64 = 1_790_000_000_000.0;

    /// §5.5 / §2.4: events after a pause are shifted back by its duration; inside it — dropped.
    #[test]
    fn events_around_pauses() {
        let mut p = PauseLog::default();
        p.push(W0 + 1000.0, W0 + 4000.0);
        p.push(W0 + 5000.0, W0 + 5500.0);
        assert_eq!(p.video_ms(W0 + 500.0, W0, 10_000.0, false), Some(500));
        assert_eq!(p.video_ms(W0 + 2000.0, W0, 10_000.0, false), None);
        assert_eq!(p.video_ms(W0 + 4000.0, W0, 10_000.0, false), Some(1000));
        assert_eq!(p.video_ms(W0 + 5200.0, W0, 10_000.0, false), None);
        assert_eq!(p.video_ms(W0 + 6000.0, W0, 10_000.0, false), Some(2500));
        assert_eq!(p.total_ms(), 3500.0);
    }

    /// §5.5: before the first frame only context events younger than 5 s survive, at 0.
    #[test]
    fn before_first_frame() {
        let p = PauseLog::default();
        assert_eq!(
            p.video_ms(W0 - 0.5, W0, 1000.0, false),
            Some(0),
            "1 ms slack"
        );
        assert_eq!(p.video_ms(W0 - 100.0, W0, 1000.0, false), None);
        assert_eq!(p.video_ms(W0 - 4999.0, W0, 1000.0, true), Some(0));
        assert_eq!(p.video_ms(W0 - 5001.0, W0, 1000.0, true), None);
        assert_eq!(p.video_ms(W0, 0.0, 1000.0, true), None, "no first frame");
    }

    /// §5.5: after the end + 250 ms — dropped.
    #[test]
    fn after_end() {
        let p = PauseLog::default();
        assert_eq!(p.video_ms(W0 + 1250.0, W0, 1000.0, false), Some(1250));
        assert_eq!(p.video_ms(W0 + 1251.0, W0, 1000.0, false), None);
    }
}
