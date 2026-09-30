//! The recording clock on Windows (ZK-89): the performance counter itself. WASAPI stamps its
//! packets with it (`GetBuffer(.., qpc)` is QPC in 100 ns) and the input hook reads it too, so
//! video slots, sound and clicks share one time line without conversion.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use znimok_video::clock::Clock;

#[derive(Clone, Copy, Debug)]
pub struct QpcClock {
    frequency: i64,
}

impl QpcClock {
    pub fn new() -> Self {
        let mut f = 0i64;
        // SAFETY: a valid out-pointer; never fails on Windows XP and later.
        let _ = unsafe { QueryPerformanceFrequency(&mut f) };
        Self {
            frequency: f.max(1),
        }
    }
}

impl Default for QpcClock {
    fn default() -> Self {
        Self::new()
    }
}

/// The counter now (for stamping input events on the recording clock).
pub fn qpc_now() -> i64 {
    let mut q = 0i64;
    // SAFETY: a valid out-pointer; never fails on Windows XP and later.
    let _ = unsafe { QueryPerformanceCounter(&mut q) };
    q
}

impl Clock for QpcClock {
    fn ticks(&self) -> i64 {
        qpc_now()
    }
    fn frequency(&self) -> i64 {
        self.frequency
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_real_time() {
        let c = QpcClock::new();
        let a = c.ticks();
        std::thread::sleep(Duration::from_millis(50));
        let ms = (c.ticks() - a) * 1000 / c.frequency();
        assert!((45..200).contains(&ms), "{ms} ms");
    }
}
