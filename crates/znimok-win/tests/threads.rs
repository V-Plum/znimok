//! Captures from several short-lived threads at once, mixing WGC, DXGI and display enumeration.
//! Regression: WinRT used only the first thread's apartment, and when that thread ended the others
//! crashed with ACCESS_VIOLATION (5 runs of 6 before the fix). Every thread now joins the MTA itself.
#![cfg(windows)]

use znimok_platform::{Capture, CaptureOptions, CaptureTarget};
use znimok_win::{Api, WinCapture};

#[test]
fn parallel_captures_from_short_lived_threads() {
    let threads: Vec<_> = (0..6)
        .map(|i| {
            std::thread::spawn(move || {
                let c = WinCapture::with_api(if i % 2 == 0 { Api::Wgc } else { Api::Dxgi });
                for _ in 0..4 {
                    let d = c.displays().unwrap().remove(0);
                    // DXGI may be refused in this session (RDP); the point is not crashing.
                    let _ = c.capture(
                        &CaptureTarget::Display { id: d.id },
                        &CaptureOptions::default(),
                    );
                }
            })
        })
        .collect();
    for t in threads {
        t.join().expect("потік захоплення впав");
    }
}
