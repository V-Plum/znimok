//! The card after a quick capture (ZK-41): Shift on release in the overlay puts the shot into
//! the clipboard and the library, and this card appears in the corner of that display for six
//! seconds — Edit (or a click on the picture), drag the picture out as a file, save it as a file,
//! show the library. The pointer on the card stops the clock. Windows: bottom right of the work
//! area (above the taskbar, where the system's own notifications are); macOS: top right under
//! the menu bar, as the system's notifications. The window never takes the focus.

use std::cell::RefCell;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use slint::ComponentHandle;
use znimok_core::Raster;
use znimok_platform::Rect;

use crate::{Pill, io};

/// Title the window-attributes hook in main.rs recognises.
pub const TITLE: &str = "Znimok pill";
const SHOW_FOR: Duration = Duration::from_secs(6);
const TICK: Duration = Duration::from_millis(40);

struct State {
    ui: Pill,
    raster: Raster,
    /// The document in the library.
    path: PathBuf,
    name: String,
    left: Duration,
    last: Instant,
    paused: bool,
    timer: slint::Timer,
}

thread_local! {
    static PILL: RefCell<Option<State>> = const { RefCell::new(None) };
}

/// Shows the card for a shot that went to the clipboard and the library.
pub fn show(
    raster: Raster,
    path: PathBuf,
    name: String,
    heading: String,
    sub: String,
    display: Rect,
) {
    let ui = match PILL.with(|p| p.borrow_mut().take()) {
        Some(old) => {
            old.timer.stop();
            old.ui
        }
        None => match Pill::new() {
            Ok(ui) => {
                wire(&ui);
                ui
            }
            Err(_) => return,
        },
    };
    ui.global::<crate::Theme>()
        .set_mode(crate::app::THEME_MODE.with(|m| m.get()));
    ui.global::<crate::Theme>()
        .set_system_dark(crate::app::SYSTEM_DARK.with(|d| d.get()));
    ui.set_thumb(thumbnail(&raster));
    ui.set_heading(heading.into());
    ui.set_sub(sub.into());
    ui.set_progress(1.0);
    ui.set_shown(false);
    place(&ui, display);
    #[cfg(windows)]
    let before = win::foreground();
    let _ = ui.show();
    place(&ui, display);
    #[cfg(windows)]
    win::no_activate(ui.window(), before);
    crate::frame::round_window(ui.window());
    #[cfg(target_os = "macos")]
    mac::round(ui.window());
    // One frame later, so the slide-in animates from the start position.
    let weak = ui.as_weak();
    slint::Timer::single_shot(Duration::from_millis(30), move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_shown(true);
        }
    });
    let timer = slint::Timer::default();
    timer.start(slint::TimerMode::Repeated, TICK, tick);
    PILL.with(|p| {
        *p.borrow_mut() = Some(State {
            ui,
            raster,
            path,
            name,
            left: SHOW_FOR,
            last: Instant::now(),
            paused: false,
            timer,
        })
    });
}

pub fn is_open() -> bool {
    PILL.with(|p| {
        p.borrow()
            .as_ref()
            .is_some_and(|s| s.ui.window().is_visible())
    })
}

/// For the self-test.
pub fn with_window<R>(f: impl FnOnce(&slint::Window) -> R) -> Option<R> {
    PILL.with(|p| p.borrow().as_ref().map(|s| f(s.ui.window())))
}

fn tick() {
    // A system drag (out to a messenger) takes the mouse: the card never hears the pointer
    // leave, and the hover pause would hold it until a click (owner, 29.09, ZK-133). While
    // paused, ask the system: pointer away and no button down (the drag is over) = it left.
    let stuck = PILL.with(|p| {
        p.borrow()
            .as_ref()
            .filter(|s| s.paused && pointer_away(s.ui.window()))
            .map(|s| s.ui.clone_strong())
    });
    if let Some(ui) = stuck {
        // PointerExited clears the hover in Slint, which calls `hovered(false)`.
        #[cfg(any(windows, target_os = "macos"))]
        crate::app::release_pointer(ui.window());
        #[cfg(not(any(windows, target_os = "macos")))]
        let _ = ui;
        // …and the clock goes on even when Slint had no hover to clear.
        PILL.with(|p| {
            if let Some(s) = p.borrow_mut().as_mut() {
                unpause(s);
            }
        });
    }
    let done = PILL.with(|p| {
        let mut p = p.borrow_mut();
        let Some(s) = p.as_mut() else { return false };
        let now = Instant::now();
        if !s.paused {
            s.left = s.left.saturating_sub(now - s.last);
        }
        s.last = now;
        s.ui.set_progress(s.left.as_secs_f32() / SHOW_FOR.as_secs_f32());
        s.left.is_zero()
    });
    if done {
        close();
    }
}

/// Back from the pointer: a little time to decide, but not the full six seconds.
fn unpause(s: &mut State) {
    s.paused = false;
    s.left = s.left.max(Duration::from_millis(2500));
}

/// For the self-test: the hover pause as a system drag leaves it (ZK-133), and whether it holds.
pub fn hold_for_test() {
    PILL.with(|p| {
        if let Some(s) = p.borrow_mut().as_mut() {
            s.paused = true;
        }
    });
}

/// Whether the system tells where the pointer is (not on a disconnected remote session).
pub fn pointer_known() -> bool {
    #[cfg(windows)]
    {
        let mut p = windows::Win32::Foundation::POINT::default();
        // SAFETY: valid out-pointer.
        unsafe { windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut p) }.is_ok()
    }
    #[cfg(not(windows))]
    {
        true
    }
}

pub fn paused() -> Option<bool> {
    PILL.with(|p| p.borrow().as_ref().map(|s| s.paused))
}

/// The pointer is outside the card's window and no mouse button is down (ZK-133).
#[cfg(windows)]
fn pointer_away(w: &slint::Window) -> bool {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON};
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
    let mut p = POINT::default();
    // SAFETY: valid out-pointer; plain state queries.
    if unsafe { GetCursorPos(&mut p) }.is_err() {
        return false;
    }
    // SAFETY: plain state queries.
    let down = unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } < 0
        || unsafe { GetAsyncKeyState(VK_RBUTTON.0 as i32) } < 0;
    // Both in physical pixels of the virtual desktop (the app is per-monitor DPI aware).
    let (pos, size) = (w.position(), w.size());
    let inside = p.x >= pos.x
        && p.y >= pos.y
        && p.x < pos.x + size.width as i32
        && p.y < pos.y + size.height as i32;
    !inside && !down
}

#[cfg(target_os = "macos")]
fn pointer_away(w: &slint::Window) -> bool {
    use objc2_app_kit::{NSEvent, NSView};
    use slint::winit_030::WinitWindowAccessor;
    use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    // Pointer and window frame both in global points (bottom-left origin).
    let frame = w
        .with_winit_window(|ww| {
            let handle = ww.window_handle().ok()?;
            let RawWindowHandle::AppKit(a) = handle.as_raw() else {
                return None;
            };
            // SAFETY: winit hands out the NSView of a live window; we are on the main thread.
            let view: &NSView = unsafe { a.ns_view.cast().as_ref() };
            view.window().map(|nw| nw.frame())
        })
        .flatten();
    let Some(f) = frame else { return false };
    let p = NSEvent::mouseLocation();
    let inside = p.x >= f.origin.x
        && p.y >= f.origin.y
        && p.x < f.origin.x + f.size.width
        && p.y < f.origin.y + f.size.height;
    !inside && NSEvent::pressedMouseButtons() == 0
}

#[cfg(not(any(windows, target_os = "macos")))]
fn pointer_away(_w: &slint::Window) -> bool {
    false
}

/// Slides the card out, then hides the window.
pub fn close() {
    let ui = PILL.with(|p| {
        p.borrow().as_ref().map(|s| {
            s.timer.stop();
            s.ui.set_shown(false);
            s.ui.as_weak()
        })
    });
    let Some(weak) = ui else { return };
    slint::Timer::single_shot(Duration::from_millis(220), move || {
        if let Some(ui) = weak.upgrade() {
            let _ = ui.hide();
        }
    });
}

fn wire(ui: &Pill) {
    ui.on_hovered(|on| {
        PILL.with(|p| {
            if let Some(s) = p.borrow_mut().as_mut() {
                if on {
                    s.paused = true;
                } else {
                    unpause(s);
                }
            }
        })
    });
    ui.on_dismiss(close);
    // «Send to …» (ZK-101): the picture as PNG to the quick target.
    // From the integrations' own copy of the settings: the card shows up inside a borrow of the
    // App (with_ctx), where the App cannot be read.
    let quick = crate::integrations::quick_target(&crate::integrations::current());
    if let Some((_, name)) = &quick {
        ui.set_send_target(name.clone().into());
    }
    ui.on_send(move || {
        let Some((key, _)) = quick.clone() else {
            return;
        };
        let job = PILL.with(|p| {
            p.borrow().as_ref().map(|s| {
                (
                    s.raster.width,
                    s.raster.height,
                    s.raster.rgba.clone(),
                    s.name.clone(),
                )
            })
        });
        let Some((w, h, rgba, name)) = job else {
            return;
        };
        close();
        let opts = io::Encode {
            format: io::Format::Png,
            quality: 90,
            lossless: true,
            white_bg: false,
        };
        let meta = crate::filemeta::FileMeta::new_shot(&name);
        let bytes = io::encode(w, h, &rgba, opts, Some(&meta));
        crate::with_ctx(move |app, ui| match bytes {
            Ok(b) => {
                let item = znimok_share::Item {
                    file_name: format!("{name}.png"),
                    mime: "image/png".into(),
                    title: name.clone(),
                    text: String::new(),
                    kind: "screenshot".into(),
                    place: String::new(),
                };
                app.share_bytes(ui, &key, item, &b);
            }
            Err(e) => app.toast(ui, format!("{} ({e})", app.tr.tr("export-error"))),
        });
    });
    ui.on_edit(|| {
        let path = PILL.with(|p| p.borrow().as_ref().map(|s| s.path.clone()));
        close();
        let Some(path) = path else { return };
        let _ = slint::invoke_from_event_loop(move || {
            let ctx = crate::CTX.with(|c| c.borrow().clone());
            if let Some((app, weak)) = ctx
                && let Some(ui) = weak.upgrade()
            {
                // Its own window, or the one that has it already (ZK-107).
                app.borrow_mut().open_path(&ui, &path);
            }
        });
    });
    ui.on_show_library(|| {
        close();
        let _ = slint::invoke_from_event_loop(|| {
            let ctx = crate::CTX.with(|c| c.borrow().clone());
            if let Some((_, weak)) = ctx
                && let Some(ui) = weak.upgrade()
            {
                crate::show_window(&ui);
            }
        });
    });
    ui.on_save_file(|| {
        let job = PILL.with(|p| {
            p.borrow().as_ref().map(|s| {
                (
                    s.raster.width,
                    s.raster.height,
                    s.raster.rgba.clone(),
                    s.name.clone(),
                )
            })
        });
        let Some((w, h, rgba, name)) = job else {
            return;
        };
        close();
        if crate::filedlg::busy() {
            return;
        }
        let dlg = rfd::FileDialog::new()
            .add_filter("PNG", &["png"])
            .add_filter("JPEG", &["jpg", "jpeg"])
            .add_filter("WebP", &["webp"])
            .set_file_name(format!("{name}.png"));
        // Off the UI thread (ZK-223), as every dialog of the app.
        crate::filedlg::save_file(dlg, move |file| {
            let Some(p) = file else { return };
            let meta = crate::filemeta::FileMeta::new_shot(&name);
            let r = io::write_image(&p, w, h, rgba, Some(&meta));
            crate::with_ctx(move |app, ui| {
                let msg = match r {
                    Ok(()) => app.tr.tr_args(
                        "export-done-toast",
                        &znimok_i18n::args!(
                            name = p
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default()
                        ),
                    ),
                    Err(e) => format!("{} ({e})", app.tr.tr("export-error")),
                };
                app.toast(ui, msg);
            });
        });
    });
    ui.on_drag_out(|| {
        let job = PILL.with(|p| {
            p.borrow_mut().as_mut().map(|s| {
                s.left = s.left.max(Duration::from_secs(3));
                (
                    s.raster.width,
                    s.raster.height,
                    s.raster.rgba.clone(),
                    s.name.clone(),
                )
            })
        });
        let Some((w, h, rgba, name)) = job else {
            return;
        };
        let dir = std::env::temp_dir().join("Znimok").join("drag");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(format!("{}.png", crate::app::file_safe(&name)));
        let meta = crate::filemeta::FileMeta::new_shot(&name);
        if io::write_image(&path, w, h, rgba, Some(&meta)).is_err() {
            return;
        }
        PILL.with(|p| {
            if let Some(s) = p.borrow().as_ref() {
                #[cfg(windows)]
                {
                    let _ = crate::dnd_win::drag_files(vec![path.clone()]);
                    crate::app::release_pointer(s.ui.window());
                }
                #[cfg(target_os = "macos")]
                {
                    crate::dnd_mac::drag_from(s.ui.window(), &path);
                    crate::app::release_pointer(s.ui.window());
                }
                #[cfg(not(any(windows, target_os = "macos")))]
                let _ = s;
            }
        });
    });
}

/// A small picture for the card (the card shows it at about 104 × 76 points).
fn thumbnail(r: &Raster) -> slint::Image {
    let k = (320.0 / r.width as f64)
        .min(240.0 / r.height as f64)
        .min(1.0);
    let (w, h) = (
        ((r.width as f64 * k).round() as u32).max(1),
        ((r.height as f64 * k).round() as u32).max(1),
    );
    let small = image::RgbaImage::from_raw(r.width, r.height, r.rgba.clone())
        .map(|i| image::imageops::resize(&i, w, h, image::imageops::FilterType::Triangle));
    match small {
        Some(i) => {
            let buf =
                slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(i.as_raw(), w, h);
            slint::Image::from_rgba8(buf)
        }
        None => slint::Image::default(),
    }
}

/// The corner of the display the shot came from.
fn place(ui: &Pill, display: Rect) {
    let margin = 16.0;
    let scale = ui.window().scale_factor() as f64;
    let (pw, ph) = (380.0, 96.0);
    #[cfg(windows)]
    {
        let work = win::work_area(display).unwrap_or(display);
        let x = work.x as f64 + work.width as f64 - (pw + margin) * scale;
        let y = work.y as f64 + work.height as f64 - (ph + margin) * scale;
        ui.window().set_position(slint::PhysicalPosition::new(
            x.round() as i32,
            y.round() as i32,
        ));
    }
    #[cfg(not(windows))]
    {
        let _ = scale;
        // Points on macOS; the visible frame leaves out the menu bar (and a notch).
        let top = mac::menu_bar_bottom(display).unwrap_or(display.y as f64 + 28.0);
        let x = display.x as f64 + display.width as f64 - pw - margin;
        let y = top + margin - 4.0;
        let _ = ph;
        ui.window()
            .set_position(slint::LogicalPosition::new(x as f32, y as f32));
    }
}

#[cfg(windows)]
mod win {
    use slint::winit_030::WinitWindowAccessor;
    use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::{HWND, POINT};
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetForegroundWindow, GetWindowLongPtrW, SetForegroundWindow,
        SetWindowLongPtrW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    };
    use znimok_platform::Rect;

    pub fn foreground() -> HWND {
        // SAFETY: no arguments.
        unsafe { GetForegroundWindow() }
    }

    /// A notification-like window: clicks work but never activate it, no taskbar button, and
    /// the window that had the focus before it appeared gets it back.
    pub fn no_activate(w: &slint::Window, before: HWND) {
        w.with_winit_window(|ww| {
            let Ok(h) = ww.window_handle() else { return };
            let RawWindowHandle::Win32(h) = h.as_raw() else {
                return;
            };
            let hwnd = HWND(h.hwnd.get() as *mut core::ffi::c_void);
            // SAFETY: a live top-level window of this process.
            unsafe {
                let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
                SetWindowLongPtrW(
                    hwnd,
                    GWL_EXSTYLE,
                    ex | (WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0) as isize,
                );
                if !before.is_invalid() && before != hwnd {
                    let _ = SetForegroundWindow(before);
                }
            }
        });
    }

    /// The monitor's work area (without the taskbar), in physical pixels.
    pub fn work_area(display: Rect) -> Option<Rect> {
        let c = POINT {
            x: display.x + display.width as i32 / 2,
            y: display.y + display.height as i32 / 2,
        };
        // SAFETY: plain Win32 queries with a correctly sized MONITORINFO.
        unsafe {
            let m = MonitorFromPoint(c, MONITOR_DEFAULTTONEAREST);
            let mut info = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if !GetMonitorInfoW(m, &mut info).as_bool() {
                return None;
            }
            let r = info.rcWork;
            Some(Rect::new(
                r.left,
                r.top,
                (r.right - r.left) as u32,
                (r.bottom - r.top) as u32,
            ))
        }
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    use objc2_app_kit::{NSColor, NSScreen, NSView};
    use objc2_foundation::MainThreadMarker;
    use slint::winit_030::WinitWindowAccessor;
    use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use znimok_platform::Rect;

    /// Rounded corners like the system's notifications: a clear window, the content layer
    /// clipped to a 12 pt radius, the shadow following the shape.
    pub fn round(w: &slint::Window) {
        w.with_winit_window(|ww| {
            let Ok(h) = ww.window_handle() else { return };
            let RawWindowHandle::AppKit(a) = h.as_raw() else {
                return;
            };
            // SAFETY: winit hands out the NSView of a live window; main thread.
            let view: &NSView = unsafe { a.ns_view.cast().as_ref() };
            let _ = objc2::exception::catch(std::panic::AssertUnwindSafe(|| {
                if let Some(win) = view.window() {
                    win.setOpaque(false);
                    win.setBackgroundColor(Some(&NSColor::clearColor()));
                    win.setHasShadow(true);
                }
                view.setWantsLayer(true);
                // SAFETY: a layer-backed view has a CALayer; plain property setters.
                unsafe {
                    let layer: *mut AnyObject = msg_send![view, layer];
                    if !layer.is_null() {
                        let _: () = msg_send![layer, setCornerRadius: 12.0f64];
                        let _: () = msg_send![layer, setMasksToBounds: true];
                    }
                }
                if let Some(win) = view.window() {
                    win.invalidateShadow();
                }
            }));
        });
    }

    /// Top of the usable area of the screen containing `display` (points, top-left origin).
    pub fn menu_bar_bottom(display: Rect) -> Option<f64> {
        let mtm = MainThreadMarker::new()?;
        let screens = NSScreen::screens(mtm);
        let main_h = screens.firstObject()?.frame().size.height;
        for s in screens.iter() {
            let f = s.frame();
            // Cocoa's origin is bottom-left of the main screen: flip to top-left.
            let top = main_h - (f.origin.y + f.size.height);
            if (f.origin.x - display.x as f64).abs() < 1.0 && (top - display.y as f64).abs() < 1.0 {
                let v = s.visibleFrame();
                return Some(main_h - (v.origin.y + v.size.height));
            }
        }
        None
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod mac {
    use znimok_platform::Rect;
    pub fn menu_bar_bottom(_: Rect) -> Option<f64> {
        None
    }
}
