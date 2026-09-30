//! ZK-193: a window that never repaints is still recorded — WGC sends a window's frame only when
//! it presents, so the source takes the first one with `PrintWindow` and the recorder repeats
//! it. A plain red window on a thread of its own stands still for the whole recording.
#![cfg(windows)]

use std::sync::mpsc;
use std::time::Duration;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{CreateSolidBrush, HBRUSH};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, MSG, PostMessageW,
    PostQuitMessage, RegisterClassW, SW_SHOWNOACTIVATE, ShowWindow, TranslateMessage, WM_CLOSE,
    WM_DESTROY, WNDCLASSW, WS_EX_TOOLWINDOW, WS_POPUP,
};
use windows::core::w;
use znimok_platform::WindowId;
use znimok_video::traits::{Decoded, VideoDecoder};
use znimok_video_win::{Api, MfDecoder, RecordRequest, Recording, Target};

unsafe extern "system" fn proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_DESTROY {
        // SAFETY: called on the window's own thread.
        unsafe { PostQuitMessage(0) };
        return LRESULT(0);
    }
    // SAFETY: the default handling of the window's own message.
    unsafe { DefWindowProcW(h, msg, wp, lp) }
}

/// A red 480 × 320 window with its own message loop; its handle comes back through `tx`.
fn red_window(tx: mpsc::Sender<isize>) {
    // SAFETY: the class and the window live on this thread until its loop ends.
    unsafe {
        let class = WNDCLASSW {
            lpfnWndProc: Some(proc),
            lpszClassName: w!("ZnimokStillWindowTest"),
            // COLORREF is 0x00BBGGRR.
            hbrBackground: HBRUSH(
                CreateSolidBrush(windows::Win32::Foundation::COLORREF(0x0028_28C8)).0,
            ),
            ..Default::default()
        };
        RegisterClassW(&class);
        let Ok(h) = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            w!("ZnimokStillWindowTest"),
            w!("still"),
            WS_POPUP,
            120,
            120,
            480,
            320,
            None,
            None,
            None,
            None,
        ) else {
            let _ = tx.send(0);
            return;
        };
        let _ = ShowWindow(h, SW_SHOWNOACTIVATE);
        let _ = tx.send(h.0 as isize);
        let mut m = MSG::default();
        while GetMessageW(&mut m, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&m);
            DispatchMessageW(&m);
        }
    }
}

/// Records the red window for 1.5 s: `(frames, Y, Cr)` of the 20th frame's centre, or `None`
/// where WGC cannot record in this session.
fn record_red(dir: &std::path::Path, name: &str) -> Option<(usize, u8, u8)> {
    let (tx, rx) = mpsc::channel();
    let ui = std::thread::spawn(move || red_window(tx));
    let h = rx.recv().unwrap();
    if h == 0 {
        eprintln!("пропущено: вікно не створилось");
        return None;
    }
    // Let it paint once, then stand still.
    std::thread::sleep(Duration::from_millis(300));
    let path = dir.join(name);
    let req = RecordRequest {
        fps: 30,
        api: Api::Wgc,
        ..RecordRequest::new(
            Target::Window {
                id: WindowId(h as u64),
            },
            path.clone(),
        )
    };
    let close = || {
        // SAFETY: a window of this test; the message ends its thread's loop.
        let _ = unsafe { PostMessageW(Some(HWND(h as *mut _)), WM_CLOSE, WPARAM(0), LPARAM(0)) };
    };
    let rec = match Recording::start(req) {
        Ok(r) => r,
        Err(e) => {
            close();
            let _ = ui.join();
            eprintln!("пропущено (WGC недоступний у цьому сеансі): {e}");
            return None;
        }
    };
    std::thread::sleep(Duration::from_millis(1500));
    let fin = rec.stop();
    close();
    let _ = ui.join();
    eprintln!(
        "{name}: кадрів {}, {:.0} мс, помилка {:?}",
        fin.result.frames, fin.result.duration_ms, fin.result.error
    );
    let frames = fin.result.frames as usize;
    let Some(out) = fin.path else {
        return Some((frames, 0, 0));
    };
    let mut dec = MfDecoder::open(&out).unwrap();
    let mut n = 0;
    while let Some(s) = dec.next().unwrap() {
        if let Decoded::Video { frame, .. } = s {
            n += 1;
            if n == 20 {
                let [y, _cb, cr] = frame.ycc(frame.width / 2, frame.height / 2);
                return Some((frames, y, cr));
            }
        }
    }
    Some((frames, 0, 0))
}

#[test]
fn a_still_window_is_recorded() {
    let dir = std::env::temp_dir().join(format!("znimok-still-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // As WGC gives it, then with WGC's frames ignored — the window that never presents.
    for (silent, name) in [(false, "wgc.mp4"), (true, "printed.mp4")] {
        if silent {
            // SAFETY: the only test of this binary; nothing else reads the environment now.
            unsafe { std::env::set_var("ZNIMOK_TEST_WGC_SILENT", "1") };
        }
        let Some((frames, y, cr)) = record_red(&dir, name) else {
            return;
        };
        assert!(frames >= 30, "{name}: a steady stream, {frames} frames");
        // Red (200, 40, 40): Y ≈ 90, Cr ≈ 200.
        assert!(
            (60..=130).contains(&y) && cr > 170,
            "{name}: the window's red in the video: Y {y} Cr {cr}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// ZK-90: a press is drawn into the video — a yellow ring around it in the frames right after,
/// none in the frames before.
#[test]
fn a_click_ring_is_drawn() {
    use znimok_video::events::Button;
    use znimok_video_win::{MouseInput, MouseOpts};
    let (tx, rx) = mpsc::channel();
    let ui = std::thread::spawn(move || red_window(tx));
    let h = rx.recv().unwrap();
    if h == 0 {
        return;
    }
    std::thread::sleep(Duration::from_millis(300));
    let dir = std::env::temp_dir().join(format!("znimok-ring-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("ring.mp4");
    let mouse = MouseInput::start();
    let req = RecordRequest {
        fps: 30,
        api: Api::Wgc,
        events: Some(mouse.queue()),
        overlay: Some(mouse.overlay(MouseOpts {
            cursor: false,
            clicks: true,
            color: [1.0, 0.82, 0.25],
        })),
        ..RecordRequest::new(
            Target::Window {
                id: WindowId(h as u64),
            },
            path.clone(),
        )
    };
    let close = || {
        // SAFETY: a window of this test; the message ends its thread's loop.
        let _ = unsafe { PostMessageW(Some(HWND(h as *mut _)), WM_CLOSE, WPARAM(0), LPARAM(0)) };
    };
    let rec = match Recording::start(req) {
        Ok(r) => r,
        Err(e) => {
            close();
            let _ = ui.join();
            eprintln!("пропущено: {e}");
            return;
        }
    };
    // The window's middle on the desktop: 120 + 240, 120 + 160.
    let bounds = znimok_win::raw::dwm_bounds(HWND(h as *mut _)).unwrap();
    let (cx, cy) = (
        bounds.x + bounds.width as i32 / 2,
        bounds.y + bounds.height as i32 / 2,
    );
    std::thread::sleep(Duration::from_millis(600));
    mouse.note(cx, cy, Button::Left, true);
    mouse.note(cx, cy, Button::Left, false);
    std::thread::sleep(Duration::from_millis(900));
    let fin = rec.stop();
    close();
    let _ = ui.join();
    let out = fin.path.expect("committed");
    let events = fin.result.events.len();
    // Yellow on red: a pixel much greener than the red window (Y up, Cb down).
    let mut dec = MfDecoder::open(&out).unwrap();
    let mut frames = Vec::new();
    while let Some(s) = dec.next().unwrap() {
        if let Decoded::Video {
            time_hns, frame, ..
        } = s
        {
            let (w, h) = (frame.width, frame.height);
            // Around the middle, at the radii the ring spreads through (6…26 px).
            let yellow = (-30i32..=30)
                .flat_map(|dy| (-30i32..=30).map(move |dx| (dx, dy)))
                .filter(|&(dx, dy)| {
                    let [y, cb, _] =
                        frame.ycc((w as i32 / 2 + dx) as u32, (h as i32 / 2 + dy) as u32);
                    y > 150 && cb < 110
                })
                .count();
            frames.push((time_hns / 10_000, yellow));
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    let with: Vec<_> = frames.iter().filter(|f| f.1 > 20).collect();
    eprintln!(
        "подій {events}; кадри з кільцем: {with:?} з {}",
        frames.len()
    );
    assert_eq!(events, 2, "the press and the release are logged");
    assert!(!with.is_empty(), "a ring is in the video");
    assert!(
        frames.first().is_some_and(|f| f.1 <= 20),
        "no ring before the press"
    );
    let span = with.last().unwrap().0 - with.first().unwrap().0;
    assert!(span <= 400, "the ring is gone after ~350 ms: {span} ms");
}
