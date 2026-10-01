//! The recording clock on macOS: `mach_absolute_time`, the host clock ScreenCaptureKit stamps its
//! samples with (their presentation times are host-clock seconds), so video slots and sound
//! share one time line without conversion.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use znimok_video::clock::Clock;

#[repr(C)]
struct TimebaseInfo {
    numer: u32,
    denom: u32,
}

unsafe extern "C" {
    fn mach_absolute_time() -> u64;
    fn mach_timebase_info(info: *mut TimebaseInfo) -> i32;
}

#[derive(Clone, Copy, Debug)]
pub struct MachClock {
    numer: u32,
    denom: u32,
}

impl MachClock {
    pub fn new() -> Self {
        let mut tb = TimebaseInfo { numer: 0, denom: 0 };
        // SAFETY: a valid out-pointer.
        unsafe { mach_timebase_info(&mut tb) };
        Self {
            numer: tb.numer.max(1),
            denom: tb.denom.max(1),
        }
    }

    /// Nanoseconds of the host clock now.
    pub fn now_ns(&self) -> i64 {
        // SAFETY: no arguments, never fails.
        let t = unsafe { mach_absolute_time() };
        (u128::from(t) * u128::from(self.numer) / u128::from(self.denom)) as i64
    }
}

impl Default for MachClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for MachClock {
    fn ticks(&self) -> i64 {
        self.now_ns()
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

/// A host-clock time (`CMTime` value / timescale, as ScreenCaptureKit stamps) in 100 ns.
pub fn host_to_hns(value: i64, timescale: i32) -> i64 {
    if timescale <= 0 {
        return 0;
    }
    ((value as i128) * 10_000_000 / timescale as i128) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_real_time() {
        let c = MachClock::new();
        let a = c.ticks();
        std::thread::sleep(Duration::from_millis(50));
        let ms = (c.ticks() - a) / 1_000_000;
        assert!((45..200).contains(&ms), "{ms} ms");
        assert_eq!(host_to_hns(3, 2), 15_000_000);
    }
}
