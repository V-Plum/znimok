//! Scrolling capture (ZK-141). S in the capture overlay takes the highlighted window or region:
//! Znimok turns the wheel over it, grabs the area after each turn and stitches the frames
//! (`znimok-stitch`) until the content ends, the height limit, «Готово» or Esc (nothing kept).
//! When the wheel moves nothing (a program that ignores synthetic scrolling, or macOS without
//! the Accessibility permission) it switches to manual: the person scrolls, Znimok keeps up.
//! The small panel with the height and the buttons sits outside the area, so it is never in it.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use slint::ComponentHandle;
use znimok_core::Raster;
use znimok_stitch::{Step, Stitcher};

pub const PANEL_TITLE: &str = "Znimok scroll";

/// The tallest result, in pixels (the document format's limit is higher; this keeps it usable).
const MAX_HEIGHT: u32 = 20_000;

/// Stop requests from the panel or Esc.
const RUN: u8 = 0;
const DONE: u8 = 1;
const CANCEL: u8 = 2;

struct Job {
    panel: crate::ScrollPanel,
    stop: Arc<AtomicU8>,
    editor_was_visible: bool,
}

thread_local! {
    static JOB: std::cell::RefCell<Option<Job>> = const { std::cell::RefCell::new(None) };
}

pub fn active() -> bool {
    JOB.with(|j| j.borrow().is_some())
}

/// Esc: stop and keep nothing.
pub fn cancel() {
    stop(CANCEL);
}

/// «Готово»: stop and keep what is stitched.
pub fn done() {
    stop(DONE);
}

fn stop(how: u8) {
    JOB.with(|j| {
        if let Some(job) = j.borrow().as_ref() {
            job.stop.store(how, Ordering::SeqCst);
        }
    });
}

/// Frames come from `grab` (the area, straight RGBA); `wheel` turns the content one step
/// down. Both run on a worker thread. `panel_at`: desktop point for the panel's top-left.
pub fn start_with(
    mut grab: impl FnMut() -> Option<Raster> + Send + 'static,
    mut wheel: impl FnMut() + Send + 'static,
    panel_at: (i32, i32),
    editor_was_visible: bool,
    pace: std::time::Duration,
) {
    if active() {
        return;
    }
    let Ok(panel) = crate::ScrollPanel::new() else {
        return;
    };
    panel.on_done(done);
    panel.on_cancel(cancel);
    let _ = panel.show();
    if cfg!(target_os = "macos") {
        panel.window().set_position(slint::LogicalPosition::new(
            panel_at.0 as f32,
            panel_at.1 as f32,
        ));
    } else {
        panel
            .window()
            .set_position(slint::PhysicalPosition::new(panel_at.0, panel_at.1));
    }
    crate::frame::round_window(panel.window());
    crate::hotkeys::grab_escape(true);
    let stop = Arc::new(AtomicU8::new(RUN));
    let weak = panel.as_weak();
    JOB.with(|j| {
        *j.borrow_mut() = Some(Job {
            panel,
            stop: stop.clone(),
            editor_was_visible,
        })
    });
    let manual = Arc::new(AtomicBool::new(false));
    std::thread::spawn(move || {
        let mut st = Stitcher::new(MAX_HEIGHT);
        let report = |h: u32, manual: bool| {
            let weak = weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(p) = weak.upgrade() {
                    p.set_stitched(h as i32);
                    p.set_manual(manual);
                }
            });
        };
        if let Some(f) = grab() {
            st.push(f.width, f.height, f.rgba);
        }
        report(st.height(), false);
        let (mut still, mut moved_once) = (0u32, false);
        while stop.load(Ordering::SeqCst) == RUN {
            let by_hand = manual.load(Ordering::SeqCst);
            if !by_hand {
                wheel();
            }
            std::thread::sleep(pace);
            let Some(f) = grab() else { break };
            match st.push(f.width, f.height, f.rgba) {
                Step::Added(_) => {
                    still = 0;
                    moved_once = true;
                }
                Step::Same => {
                    still += 1;
                    // The wheel moved nothing from the start: the person scrolls.
                    if !by_hand && !moved_once && still >= 2 {
                        manual.store(true, Ordering::SeqCst);
                    }
                    // The wheel moved it before and now nothing moves: the end.
                    if !by_hand && moved_once && still >= 2 {
                        break;
                    }
                }
                // Too fast (by hand) — wait for the next frame; the wheel never jumps that far.
                Step::Lost => {
                    if !by_hand {
                        break;
                    }
                }
                Step::Full => break,
            }
            report(st.height(), manual.load(Ordering::SeqCst));
        }
        let keep = stop.load(Ordering::SeqCst) != CANCEL;
        let result = if keep { st.finish() } else { None };
        let _ = slint::invoke_from_event_loop(move || finish(result));
    });
}

fn finish(result: Option<(u32, u32, Vec<u8>)>) {
    let Some(job) = JOB.with(|j| j.borrow_mut().take()) else {
        return;
    };
    let _ = job.panel.hide();
    crate::hotkeys::grab_escape(false);
    crate::with_ctx(|a, ui| match result {
        Some((w, h, rgba)) => {
            crate::show_window(ui);
            a.new_document(ui, Raster::new(w, h, rgba), "scroll", None);
        }
        None => {
            if job.editor_was_visible {
                crate::show_window(ui);
            }
        }
    });
}

/// The real thing: frames of `rect` (frame pixels of `display`), the wheel over its middle.
pub fn start(
    display: znimok_platform::Rect,
    frame_size: (u32, u32),
    rect: crate::capture::PxRect,
    editor_was_visible: bool,
) {
    // Frame pixels → desktop units (pixels on Windows, points on macOS).
    let k = display.width as f32 / frame_size.0.max(1) as f32;
    let cx = display.x + ((rect.x + rect.w / 2) as f32 * k) as i32;
    let cy = display.y + ((rect.y + rect.h / 2) as f32 * k) as i32;
    // Wheel steps per turn: small areas scroll a notch at a time (a jump bigger than the area
    // cannot be stitched).
    let notches = ((rect.h as f32 * k) / 400.0).clamp(1.0, 3.0) as i32;
    // The panel under the area, or above it, or in its bottom-right corner.
    let (pw, ph) = if cfg!(target_os = "macos") {
        (330, 64)
    } else {
        ((330.0 / k.max(0.01)) as i32, (64.0 / k.max(0.01)) as i32)
    };
    let (ax, ay) = (
        display.x + (rect.x as f32 * k) as i32,
        display.y + (rect.y as f32 * k) as i32,
    );
    let (ab, dbottom) = (
        ay + (rect.h as f32 * k) as i32,
        display.y + display.height as i32,
    );
    let py = if ab + 12 + ph <= dbottom {
        ab + 12
    } else if ay - 12 - ph >= display.y {
        ay - 12 - ph
    } else {
        ab - ph - 12
    };
    let px = ax.clamp(display.x + 8, display.x + display.width as i32 - pw - 8);
    let grab = move || {
        let f = crate::capture::display_frame(display).ok()?;
        crop(&f, rect)
    };
    let wheel = move || wheel_down(cx, cy, notches);
    start_with(
        grab,
        wheel,
        (px, py),
        editor_was_visible,
        std::time::Duration::from_millis(280),
    );
}

fn crop(f: &Raster, r: crate::capture::PxRect) -> Option<Raster> {
    let (fw, fh) = (f.width as i32, f.height as i32);
    let (x0, y0) = (r.x.clamp(0, fw), r.y.clamp(0, fh));
    let (x1, y1) = ((r.x + r.w).clamp(0, fw), (r.y + r.h).clamp(0, fh));
    if x1 - x0 < 1 || y1 - y0 < 1 {
        return None;
    }
    let mut rgba = Vec::with_capacity(((x1 - x0) * (y1 - y0) * 4) as usize);
    for y in y0..y1 {
        let a = ((y * fw + x0) * 4) as usize;
        rgba.extend_from_slice(&f.rgba[a..a + ((x1 - x0) * 4) as usize]);
    }
    Some(Raster::new((x1 - x0) as u32, (y1 - y0) as u32, rgba))
}

/// Turns the wheel down over (x, y) — desktop units.
#[cfg(windows)]
fn wheel_down(x: i32, y: i32, notches: i32) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_WHEEL, MOUSEINPUT, SendInput,
    };
    use windows::Win32::UI::WindowsAndMessaging::SetCursorPos;
    // SAFETY: plain Win32 input calls with a stack-allocated INPUT.
    unsafe {
        let _ = SetCursorPos(x, y);
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: (-120 * notches) as u32,
                    dwFlags: MOUSEEVENTF_WHEEL,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
    }
}

#[cfg(target_os = "macos")]
fn wheel_down(x: i32, y: i32, notches: i32) {
    use std::ffi::c_void;
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CGPoint {
        x: f64,
        y: f64,
    }
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventCreateScrollWheelEvent2(
            source: *const c_void,
            units: u32,
            count: u32,
            w1: i32,
            w2: i32,
            w3: i32,
        ) -> *mut c_void;
        fn CGEventSetLocation(event: *mut c_void, p: CGPoint);
        fn CGEventPost(tap: u32, event: *mut c_void);
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(cf: *const c_void);
    }
    // SAFETY: CoreGraphics calls with an event we create and release; kCGScrollEventUnitLine = 1,
    // kCGHIDEventTap = 0. Without the Accessibility permission the post is dropped by the system
    // (then the capture goes manual).
    unsafe {
        let ev = CGEventCreateScrollWheelEvent2(std::ptr::null(), 1, 1, -3 * notches, 0, 0);
        if ev.is_null() {
            return;
        }
        CGEventSetLocation(
            ev,
            CGPoint {
                x: x as f64,
                y: y as f64,
            },
        );
        CGEventPost(0, ev);
        CFRelease(ev);
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn wheel_down(_x: i32, _y: i32, _notches: i32) {}
