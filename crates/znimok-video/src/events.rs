//! Timed input events (mouse clicks) on the recording clock (§4, §2.4).
//!
//! The OS hook (Windows `WH_MOUSE_LL`, macOS `CGEventTap` — ZK-89) pushes events with the clock
//! tick of the moment into an [`EventQueue`]; the recording loop drains them at every frame with
//! an [`EventGate`]:
//! - events before the first frame (the hotkey press itself) are dropped;
//! - time is `(tick − t0)·1000/f` ms with the CURRENT `t0` — after a pause `t0` has moved, so
//!   later events are in video time;
//! - events during a pause are skipped: on resume the gate jumps over everything queued so far.
//!
//! Drawing rings and the cursor is the backend's (shader) business; the log format (`MOUS`) is
//! another ticket.

use crate::clock::ticks_to_ms;
use std::sync::{Arc, Mutex};

/// At most this many events per recording (a runaway hook must not eat memory).
pub const MAX_EVENTS: usize = 200_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
    Middle,
}

/// One event from the hook, in physical desktop pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputEvent {
    pub ticks: i64,
    pub x: i32,
    pub y: i32,
    pub button: Button,
    pub down: bool,
}

/// Shared between the hook thread and the recording loop.
#[derive(Clone, Debug, Default)]
pub struct EventQueue {
    inner: Arc<Mutex<Vec<InputEvent>>>,
}

impl EventQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an event; `false` once the queue is full.
    pub fn push(&self, e: InputEvent) -> bool {
        let mut v = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        if v.len() >= MAX_EVENTS {
            return false;
        }
        v.push(e);
        true
    }

    pub fn len(&self) -> usize {
        self.inner.lock().unwrap_or_else(|p| p.into_inner()).len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn with<R>(&self, f: impl FnOnce(&[InputEvent]) -> R) -> R {
        f(&self.inner.lock().unwrap_or_else(|p| p.into_inner()))
    }
}

/// An event accepted into the recording.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimedEvent {
    /// Milliseconds of video time.
    pub ms: i64,
    pub event: InputEvent,
}

/// How far the loop has read the queue (`VidOverlay::seen`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EventGate {
    seen: usize,
}

impl EventGate {
    /// New events since the last call, relative to `t0` (dropping those before it).
    pub fn drain(&mut self, q: &EventQueue, t0: i64, f: i64) -> Vec<TimedEvent> {
        q.with(|v| {
            let out = v[self.seen.min(v.len())..]
                .iter()
                .filter(|e| e.ticks >= t0)
                .map(|e| TimedEvent {
                    ms: ticks_to_ms(e.ticks - t0, f),
                    event: *e,
                })
                .collect();
            self.seen = v.len();
            out
        })
    }

    /// Resume: everything queued so far (the clicks of the pause) is not recorded.
    pub fn skip_queued(&mut self, q: &EventQueue) {
        self.seen = q.len();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(ticks: i64) -> InputEvent {
        InputEvent {
            ticks,
            x: 1,
            y: 2,
            button: Button::Left,
            down: true,
        }
    }

    /// §4: events before t0 (the hotkey) are dropped; ms = (tick − t0)·1000/f.
    #[test]
    fn before_t0_dropped() {
        let q = EventQueue::new();
        q.push(ev(50));
        q.push(ev(100 + 12_345));
        let mut g = EventGate::default();
        let got = g.drain(&q, 100, 1_000_000);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].ms, 12);
        assert!(g.drain(&q, 100, 1_000_000).is_empty(), "each event once");
    }

    /// §2.4: clicks during a pause are skipped; after resume time is relative to the shifted t0.
    #[test]
    fn pause_skips_queued_clicks() {
        let q = EventQueue::new();
        let mut g = EventGate::default();
        q.push(ev(1_000));
        assert_eq!(g.drain(&q, 0, 1000).len(), 1);
        q.push(ev(5_000)); // during the pause
        g.skip_queued(&q);
        q.push(ev(9_500));
        let got = g.drain(&q, 7_000, 1000); // t0 shifted by a 7 s pause
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].ms, 2_500);
    }

    /// §4: the queue is capped at 200 000 events.
    #[test]
    fn cap() {
        let q = EventQueue::new();
        for i in 0..MAX_EVENTS {
            assert!(q.push(ev(i as i64)));
        }
        assert!(!q.push(ev(0)));
    }
}
