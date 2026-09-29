//! One clock for the whole recording (§2.3): video slots, audio packet stamps and input events
//! are all read from the same monotonic counter — QPC on Windows, `mach_absolute_time` on macOS.
//! Audio APIs give their packet stamps in that clock too (WASAPI `GetBuffer(..., &qpc100)` is QPC
//! in 100 ns; Core Audio host time is `mach_absolute_time`), which is why the audio timeline
//! works in [`ticks_to_hns`] of this clock.
//!
//! The wall clock (`GetSystemTimePreciseAsFileTime`, ms since the Unix epoch) is read only at
//! the first frame and at pause edges — for the browser log (CAPS-83), which carries wall time.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// A monotonic clock with an integer tick and a fixed frequency.
pub trait Clock: Send + Sync {
    /// Current tick. Never goes back.
    fn ticks(&self) -> i64;
    /// Ticks per second (QPC frequency, 1e9 for [`MonotonicClock`]).
    fn frequency(&self) -> i64;
    /// Wall clock, ms since the Unix epoch, as precise as the OS gives it.
    fn wall_ms(&self) -> f64;
    /// Wait. The recorder waits through the clock so a [`ManualClock`] can make time pass in
    /// tests without sleeping.
    fn sleep(&self, d: Duration);
}

/// Ticks → 100 ns without overflow for any uptime (`VidQpcTo100`: whole seconds and the
/// remainder separately).
pub fn ticks_to_hns(q: i64, f: i64) -> i64 {
    (q / f) * crate::HNS_PER_SEC + (q % f) * crate::HNS_PER_SEC / f
}

/// Ticks → milliseconds, truncated (as the mouse log does: `(qpc − t0)·1000/f`).
pub fn ticks_to_ms(q: i64, f: i64) -> i64 {
    ((q as i128) * 1000 / f as i128) as i64
}

/// `std::time::Instant` in nanoseconds (frequency 1e9). Good enough for tests and as a fallback;
/// the OS crates use the counter their audio API stamps packets with.
#[derive(Debug)]
pub struct MonotonicClock {
    origin: Instant,
}

impl MonotonicClock {
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Default for MonotonicClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for MonotonicClock {
    fn ticks(&self) -> i64 {
        self.origin.elapsed().as_nanos() as i64
    }
    fn frequency(&self) -> i64 {
        1_000_000_000
    }
    fn wall_ms(&self) -> f64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs_f64() * 1000.0)
            .unwrap_or(0.0)
    }
    fn sleep(&self, d: Duration) {
        std::thread::sleep(d);
    }
}

/// A clock that moves only when told to — or when someone sleeps on it. Clones share the time,
/// so a test, the recorder and the synthetic sources all see the same "now".
#[derive(Clone, Debug)]
pub struct ManualClock {
    inner: Arc<ManualInner>,
}

#[derive(Debug)]
struct ManualInner {
    ticks: AtomicI64,
    frequency: i64,
    /// Wall time at tick 0, ms since the epoch.
    wall_origin_ms: f64,
}

impl ManualClock {
    /// QPC-like: 10 MHz, starting at an arbitrary non-zero tick (a real counter never starts at 0,
    /// and `t0 = 0` would hide bugs where 0 means "not started").
    pub fn new() -> Self {
        Self::with_frequency(10_000_000, 123_456_789)
    }

    pub fn with_frequency(frequency: i64, start: i64) -> Self {
        assert!(frequency > 0);
        Self {
            inner: Arc::new(ManualInner {
                ticks: AtomicI64::new(start),
                frequency,
                wall_origin_ms: 1_790_000_000_000.0,
            }),
        }
    }

    pub fn advance(&self, d: Duration) {
        let t = (d.as_nanos() * self.inner.frequency as u128 / 1_000_000_000) as i64;
        self.advance_ticks(t);
    }

    pub fn advance_ms(&self, ms: u64) {
        self.advance(Duration::from_millis(ms));
    }

    pub fn advance_ticks(&self, t: i64) {
        assert!(t >= 0, "monotonic clock cannot go back");
        self.inner.ticks.fetch_add(t, Ordering::SeqCst);
    }

    /// Ticks for a duration at this clock's frequency.
    pub fn ticks_for(&self, d: Duration) -> i64 {
        (d.as_nanos() * self.inner.frequency as u128 / 1_000_000_000) as i64
    }
}

impl Default for ManualClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for ManualClock {
    fn ticks(&self) -> i64 {
        self.inner.ticks.load(Ordering::SeqCst)
    }
    fn frequency(&self) -> i64 {
        self.inner.frequency
    }
    fn wall_ms(&self) -> f64 {
        self.inner.wall_origin_ms + self.ticks() as f64 * 1000.0 / self.inner.frequency as f64
    }
    fn sleep(&self, d: Duration) {
        self.advance(d);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §2.3 `VidQpcTo100`: the split conversion is exact and does not overflow on a long uptime
    /// (QPC at 10 MHz after 100 days is ~8.6e13; ×1e7 would overflow i64 if multiplied first).
    #[test]
    fn ticks_to_hns_is_exact_and_does_not_overflow() {
        assert_eq!(ticks_to_hns(10_000_000, 10_000_000), 10_000_000);
        assert_eq!(ticks_to_hns(3_579_545 * 2 + 1, 3_579_545), 20_000_002);
        let hundred_days = 100 * 86_400 * 24_000_000i64; // 24 MHz counter
        assert_eq!(ticks_to_hns(hundred_days, 24_000_000), 100 * 86_400 * HNS);
        const HNS: i64 = crate::HNS_PER_SEC;
    }

    /// The manual clock moves only on `advance`/`sleep`, and clones share the time.
    #[test]
    fn manual_clock_is_shared_and_moves_on_sleep() {
        let c = ManualClock::new();
        let d = c.clone();
        let t = c.ticks();
        d.sleep(Duration::from_millis(5));
        assert_eq!(c.ticks() - t, 50_000);
        let w = c.wall_ms();
        c.advance_ms(1000);
        assert!((c.wall_ms() - w - 1000.0).abs() < 1e-6);
    }

    #[test]
    fn monotonic_clock_goes_forward() {
        let c = MonotonicClock::new();
        let a = c.ticks();
        c.sleep(Duration::from_millis(1));
        assert!(c.ticks() > a);
        assert!(c.wall_ms() > 1.6e12);
    }
}
