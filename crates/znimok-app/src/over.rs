//! "Over the screen" (ZK-58): the editor window itself becomes the capture's frame. It covers
//! the frozen display without chrome, above everything (on macOS above the menu bar and the
//! Dock, the same way as the capture overlay), and comes back to where and how it was.
//!
//! One window and one editor for both modes (the decision of 22.09 in LH, kept for Znimok):
//! a second copy of the editor for the overlay would drift apart from the window one.

use slint::ComponentHandle;
use slint::winit_030::WinitWindowAccessor;
use slint::winit_030::winit;

use crate::AppWindow;

/// How the window was before it covered the display.
pub struct Restore {
    pos: slint::PhysicalPosition,
    size: slint::PhysicalSize,
    maximized: bool,
    #[cfg(target_os = "macos")]
    mac: Option<mac::Saved>,
}

/// Covers `display` (desktop units: pixels on Windows, points on macOS) with the window.
pub fn enter(ui: &AppWindow, display: znimok_platform::Rect) -> Restore {
    crate::frame::set_over(true);
    let maximized = ui
        .window()
        .with_winit_window(|w| w.is_maximized())
        .unwrap_or(false);
    if maximized {
        ui.window().with_winit_window(|w| w.set_maximized(false));
    }
    let restore = Restore {
        pos: ui.window().position(),
        size: ui.window().size(),
        maximized,
        #[cfg(target_os = "macos")]
        mac: mac::save(ui),
    };
    place(ui, display);
    #[cfg(windows)]
    ui.window().with_winit_window(|w| {
        w.set_window_level(winit::window::WindowLevel::AlwaysOnTop);
        win::square_corners(w, true);
    });
    #[cfg(target_os = "macos")]
    mac::cover(ui, display);
    // Slint and winit apply window properties after this returns; set the frame again once the
    // window has settled (as for the capture overlay).
    for ms in [30u64, 150, 400] {
        let weak = ui.as_weak();
        slint::Timer::single_shot(std::time::Duration::from_millis(ms), move || {
            if let Some(ui) = weak.upgrade()
                && crate::frame::is_over()
            {
                place(&ui, display);
                #[cfg(target_os = "macos")]
                mac::cover(&ui, display);
            }
        });
    }
    restore
}

/// Back to the window as it was.
pub fn leave(ui: &AppWindow, r: Restore) {
    crate::frame::set_over(false);
    #[cfg(windows)]
    ui.window().with_winit_window(|w| {
        w.set_window_level(winit::window::WindowLevel::Normal);
        win::square_corners(w, false);
    });
    #[cfg(target_os = "macos")]
    if let Some(s) = r.mac {
        mac::restore(ui, s);
    }
    // (macOS: the saved frame above already put it back.)
    if cfg!(not(target_os = "macos")) {
        ui.window().set_size(r.size);
        ui.window().set_position(r.pos);
    }
    if r.maximized {
        ui.window().with_winit_window(|w| w.set_maximized(true));
    }
    crate::frame::after_show(ui);
}

fn place(ui: &AppWindow, b: znimok_platform::Rect) {
    if cfg!(target_os = "macos") {
        ui.window()
            .set_position(slint::LogicalPosition::new(b.x as f32, b.y as f32));
        ui.window()
            .set_size(slint::LogicalSize::new(b.width as f32, b.height as f32));
    } else {
        ui.window()
            .set_position(slint::PhysicalPosition::new(b.x, b.y));
        ui.window()
            .set_size(slint::PhysicalSize::new(b.width, b.height));
    }
}

#[cfg(windows)]
mod win {
    use super::winit;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{
        DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DWMWCP_ROUND, DwmSetWindowAttribute,
    };
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    /// Rounded corners of a window covering the display would show the live desktop through
    /// its four corners.
    pub fn square_corners(w: &winit::window::Window, square: bool) {
        let Ok(h) = w.window_handle() else { return };
        let RawWindowHandle::Win32(h) = h.as_raw() else {
            return;
        };
        let hwnd = HWND(h.hwnd.get() as *mut core::ffi::c_void);
        let pref = if square {
            DWMWCP_DONOTROUND
        } else {
            DWMWCP_ROUND
        };
        // SAFETY: a live top-level window of this process; the value is a DWORD-sized enum.
        let _ = unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                (&pref as *const windows::Win32::Graphics::Dwm::DWM_WINDOW_CORNER_PREFERENCE)
                    .cast(),
                std::mem::size_of_val(&pref) as u32,
            )
        };
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use super::*;
    use objc2::rc::Retained;
    use objc2_app_kit::{
        NSScreen, NSScreenSaverWindowLevel, NSView, NSWindow, NSWindowCollectionBehavior,
        NSWindowStyleMask,
    };
    use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    pub struct Saved {
        style: NSWindowStyleMask,
        level: isize,
        behavior: NSWindowCollectionBehavior,
        frame: NSRect,
    }

    fn ns_window(ui: &AppWindow) -> Option<Retained<NSWindow>> {
        ui.window()
            .with_winit_window(|w| {
                let handle = w.window_handle().ok()?;
                let RawWindowHandle::AppKit(a) = handle.as_raw() else {
                    return None;
                };
                // SAFETY: winit hands out the NSView of a live window; main thread.
                let view: &NSView = unsafe { a.ns_view.cast().as_ref() };
                view.window()
            })
            .flatten()
    }

    fn guarded(step: &str, f: impl FnOnce()) {
        if let Err(e) = objc2::exception::catch(std::panic::AssertUnwindSafe(f)) {
            eprintln!("[over] {step} threw {e:?}");
        }
    }

    pub fn save(ui: &AppWindow) -> Option<Saved> {
        let win = ns_window(ui)?;
        Some(Saved {
            style: win.styleMask(),
            level: win.level(),
            behavior: win.collectionBehavior(),
            frame: win.frame(),
        })
    }

    /// The display in AppKit's coordinates (origin at the bottom left of the main screen).
    fn cocoa_rect(b: znimok_platform::Rect) -> NSRect {
        let main_h = MainThreadMarker::new()
            .and_then(|m| NSScreen::screens(m).firstObject())
            .map(|s| s.frame().size.height)
            .unwrap_or(b.height as f64);
        NSRect::new(
            NSPoint::new(b.x as f64, main_h - (b.y as f64 + b.height as f64)),
            NSSize::new(b.width as f64, b.height as f64),
        )
    }

    pub fn cover(ui: &AppWindow, b: znimok_platform::Rect) {
        let Some(win) = ns_window(ui) else { return };
        crate::overlay::unconstrain_windows();
        guarded("style", || {
            if win.styleMask() != NSWindowStyleMask::Borderless {
                win.setStyleMask(NSWindowStyleMask::Borderless);
            }
        });
        guarded("level", || win.setLevel(NSScreenSaverWindowLevel));
        guarded("behavior", || {
            win.setCollectionBehavior(
                NSWindowCollectionBehavior::CanJoinAllSpaces
                    | NSWindowCollectionBehavior::FullScreenAuxiliary
                    | NSWindowCollectionBehavior::Stationary,
            )
        });
        let want = cocoa_rect(b);
        guarded("setFrame", || win.setFrame_display(want, true));
    }

    pub fn restore(ui: &AppWindow, s: Saved) {
        let Some(win) = ns_window(ui) else { return };
        guarded("style", || win.setStyleMask(s.style));
        guarded("level", || win.setLevel(s.level));
        guarded("behavior", || win.setCollectionBehavior(s.behavior));
        guarded("setFrame", || win.setFrame_display(s.frame, true));
    }
}
