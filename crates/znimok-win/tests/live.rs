//! The platform conformance checks against the real Windows desktop, plus the traps from P2:
//! a window frame equals its DWM bounds (no shadow), a region is an exact crop of the display.
//! Needs an interactive desktop (a developer machine or the GitHub Windows runner).
#![cfg(windows)]

use std::process::{Child, Command};
use std::time::{Duration, Instant};
use znimok_platform::{
    Capture, CaptureOptions, CaptureTarget, PixelFormat, Rect, WindowList, conformance,
};
use znimok_win::{Api, WinCapture};

/// One desktop, one test at a time: the window test puts a topmost window on the screen that the
/// region test would capture, and parallel captures from test threads are covered by threads.rs.
static DESKTOP: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn desktop() -> std::sync::MutexGuard<'static, ()> {
    DESKTOP.lock().unwrap_or_else(|e| e.into_inner())
}

#[test]
fn conformance_capture_wgc() {
    let _desktop = desktop();
    conformance::capture(&WinCapture::new()).unwrap();
}

#[test]
fn conformance_capture_dxgi() {
    let _desktop = desktop();
    // Duplication is refused inside some sessions (RDP, service desktops) — then say so, don't fail.
    match conformance::capture(&WinCapture::with_api(Api::Dxgi)) {
        Ok(()) => {}
        Err(e)
            if e.contains("0x887a0022")
                || e.contains("0x80070005")
                || e.contains("E_ACCESSDENIED") =>
        {
            eprintln!("DXGI недоступний у цьому сеансі: {e}")
        }
        Err(e) => panic!("{e}"),
    }
}

#[test]
fn conformance_windows() {
    let _desktop = desktop();
    conformance::windows(&WinCapture::new()).unwrap();
}

#[test]
fn region_is_an_exact_crop_of_the_display() {
    let _desktop = desktop();
    let c = WinCapture::new();
    let d = &c.displays().unwrap()[0];
    if d.color.hdr {
        return; // FP16 values can differ by rounding between two captures; the SDR case is exact.
    }
    let opts = CaptureOptions::default();
    // Two captures of a live screen can differ (clock, caret): compare a region taken right after.
    let full = c
        .capture(&CaptureTarget::Display { id: d.id.clone() }, &opts)
        .unwrap();
    let r = Rect::new(d.bounds.x + 40, d.bounds.y + 30, 120, 90);
    let part = c
        .capture(&CaptureTarget::Region { rect: r }, &opts)
        .unwrap();
    assert_eq!(
        (part.width, part.height, part.format),
        (120, 90, PixelFormat::Bgra8)
    );
    let same = (0..90)
        .filter(|&y| part.row(y) == &full.row(30 + y)[40 * 4..160 * 4])
        .count();
    assert!(
        same >= 80,
        "лише {same}/90 рядків ділянки збіглися з кадром дисплея"
    );
}

struct Win(Child);
impl Drop for Win {
    fn drop(&mut self) {
        let _ = self.0.kill();
    }
}

#[test]
fn window_frame_equals_dwm_bounds() {
    let _desktop = desktop();
    // A window of our own: PowerShell with a WinForms form at a known size and title.
    let title = format!("znimok-win test {}", std::process::id());
    let script = format!(
        "Add-Type -AssemblyName System.Windows.Forms; $f = New-Object Windows.Forms.Form; \
         $f.Text = '{title}'; $f.Width = 640; $f.Height = 400; $f.StartPosition = 'Manual'; \
         $f.Left = 120; $f.Top = 120; $f.BackColor = [Drawing.Color]::FromArgb(0, 200, 0); \
         $f.TopMost = $true; [Windows.Forms.Application]::Run($f)"
    );
    let _w = Win(Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .spawn()
        .unwrap());
    let c = WinCapture::new();
    let t0 = Instant::now();
    let w = loop {
        if let Some(w) = c.windows().unwrap().into_iter().find(|w| w.title == title) {
            break w;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(20),
            "тестове вікно не з'явилось"
        );
        std::thread::sleep(Duration::from_millis(200));
    };
    std::thread::sleep(Duration::from_millis(500));
    let f = c
        .capture(
            &CaptureTarget::Window { id: w.id },
            &CaptureOptions::default(),
        )
        .unwrap();
    assert_eq!(
        (f.width, f.height),
        (w.bounds.width, w.bounds.height),
        "кадр ≠ межі DWM {:?}",
        w.bounds
    );
    assert_eq!(f.source, w.bounds);
    assert_eq!(w.app.to_ascii_lowercase(), "powershell.exe");
    assert_eq!(
        c.window_at(znimok_platform::Point {
            x: w.bounds.x + 300,
            y: w.bounds.y + 200
        })
        .unwrap()
        .map(|x| x.id),
        Some(w.id)
    );
    if f.format == PixelFormat::Bgra8 {
        // Client area is pure green; its centre must be exact.
        let row = f.row(f.height / 2 + 20);
        let o = (f.width / 2 * 4) as usize;
        assert_eq!(&row[o..o + 3], &[0, 200, 0], "колір клієнтської області");
    }
}
