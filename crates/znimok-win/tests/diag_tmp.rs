#![cfg(windows)]
//! TEMPORARY: find the step that crashes on the CI runner. Markers go straight to the stderr handle
//! (not through the test capture), so they survive a process crash.
use std::io::Write;
use znimok_platform::{Capture, CaptureOptions, CaptureTarget, Cursor, WindowList};
use znimok_win::{Api, WinCapture};

fn mark(s: &str) {
    let mut e = std::io::stderr().lock();
    let _ = writeln!(e, "DIAG {s}");
    let _ = e.flush();
}

#[test]
fn diag_steps() {
    mark("start");
    let c = WinCapture::new();
    mark("new ok");
    let caps = c.caps();
    mark(&format!("caps {caps:?}"));
    let ds = c.displays();
    mark(&format!("displays {ds:?}"));
    let ws = c.windows().map(|w| w.len());
    mark(&format!("windows {ws:?}"));
    mark(&format!("cursor {:?}", c.position()));
    if let Ok(ds) = ds {
        for d in ds {
            mark(&format!("wgc display {:?} ...", d.id));
            let r = c.capture(
                &CaptureTarget::Display { id: d.id.clone() },
                &CaptureOptions::default(),
            );
            mark(&format!(
                "wgc display -> {:?}",
                r.map(|f| (f.width, f.height, f.format))
            ));
            mark("dxgi display ...");
            let r = WinCapture::with_api(Api::Dxgi).capture(
                &CaptureTarget::Display { id: d.id },
                &CaptureOptions::default(),
            );
            mark(&format!(
                "dxgi display -> {:?}",
                r.map(|f| (f.width, f.height, f.format))
            ));
        }
    }
    mark("end");
}
