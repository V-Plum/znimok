//! Own title bar (owner, 28.09: "the header must be ours and follow the window's theme").
//!
//! Windows: the system frame goes (`no-frame`), the window keeps its drop shadow
//! (`set_undecorated_shadow`); our bars carry minimise / maximise / close, their empty part
//! drags the window (`drag_window` = the system move loop, so Aero Snap still works), a double
//! click maximises, and thin edges resize (`drag_resize_window`).
//!
//! macOS: the traffic lights stay native; the title bar becomes transparent and the content
//! runs under it (`FullSizeContentView`), the title is hidden and the window is dark
//! (`DarkAqua`, so the lights look right). The lights are moved down to the middle of our
//! 52 pt bar — AppKit lays them out for a 28 pt title bar, so the container is enlarged and
//! the buttons re-placed after every resize (the approach Electron uses for `trafficLightPosition`).

use slint::ComponentHandle;
use slint::winit_030::WinitWindowAccessor;
use slint::winit_030::winit;

use crate::AppWindow;

/// Height of our bars (library header and editor top bar), logical pixels.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const BAR: f64 = 52.0;

pub fn before_show(ui: &AppWindow) {
    if cfg!(windows) {
        ui.set_custom_frame(true);
    }
    if cfg!(target_os = "macos") {
        // Close/minimise/zoom (3 × 20 pt at 16 pt from the edge) plus a gap.
        ui.set_lights_inset(72.0);
    }
}

/// The native window may not exist yet right after `show()` (owner's Mac 28.09: the title bar
/// stayed white and the lights moved — the dressing ran before the window existed, the light
/// placement later on a resize). So the set-up is idempotent and runs on show, on a few
/// timers after it and on every resize.
pub fn after_show(ui: &AppWindow) {
    dress(ui);
    for ms in [30u64, 150, 400, 1000] {
        let weak = ui.as_weak();
        slint::Timer::single_shot(std::time::Duration::from_millis(ms), move || {
            if let Some(ui) = weak.upgrade() {
                dress(&ui);
            }
        });
    }
}

thread_local! {
    /// The window covers a display "over the screen" (ZK-58): no chrome to dress.
    static OVER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub fn set_over(on: bool) {
    OVER.with(|o| o.set(on));
}

pub fn is_over() -> bool {
    OVER.with(|o| o.get())
}

fn dress(ui: &AppWindow) {
    if is_over() {
        return;
    }
    #[cfg(windows)]
    ui.window().with_winit_window(|w| {
        use winit::platform::windows::WindowExtWindows;
        w.set_undecorated_shadow(true);
        win::round_corners(w);
    });
    #[cfg(target_os = "macos")]
    mac::dress(ui);
    on_resized(ui);
}

pub fn on_resized(ui: &AppWindow) {
    let max = ui
        .window()
        .with_winit_window(|w| w.is_maximized())
        .unwrap_or(false);
    ui.set_win_maximized(max);
    #[cfg(target_os = "macos")]
    if !is_over() {
        mac::dress(ui);
        mac::place_lights(ui);
        // AppKit lays the title bar out again after a size change finishes — notably after
        // leaving full screen, when the animation ends later than our resize event (owner's
        // Mac 28.09: the lights jumped back to the top edge). Place them again once it settled.
        for ms in [120u64, 400, 900] {
            let weak = ui.as_weak();
            slint::Timer::single_shot(std::time::Duration::from_millis(ms), move || {
                if let Some(ui) = weak.upgrade() {
                    mac::place_lights(&ui);
                }
            });
        }
    }
}

#[cfg(windows)]
mod win {
    use super::winit;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{
        DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute,
    };
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    /// Windows 11 rounds the corners of framed windows only; a frameless one has to ask
    /// (owner 28.09: "every program here has rounded corners"). No effect on Windows 10.
    pub fn round_corners(w: &winit::window::Window) {
        let Ok(h) = w.window_handle() else { return };
        let RawWindowHandle::Win32(h) = h.as_raw() else {
            return;
        };
        let hwnd = HWND(h.hwnd.get() as *mut core::ffi::c_void);
        let pref = DWMWCP_ROUND;
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

/// Windows 11 rounded corners for any of our frameless windows (the card after a capture).
pub fn round_window(w: &slint::Window) {
    #[cfg(windows)]
    {
        use slint::winit_030::WinitWindowAccessor;
        w.with_winit_window(win::round_corners);
    }
    #[cfg(not(windows))]
    let _ = w;
}

/// The window chrome follows the theme (macOS: Aqua / DarkAqua for the traffic lights and menus;
/// Windows draws no system chrome here).
pub fn set_dark(ui: &AppWindow, dark: bool) {
    DARK.with(|d| d.set(dark));
    #[cfg(target_os = "macos")]
    mac::dress(ui);
    #[cfg(not(target_os = "macos"))]
    let _ = ui;
}

thread_local! {
    static DARK: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

pub fn drag(ui: &AppWindow) {
    ui.window().with_winit_window(|w| {
        let _ = w.drag_window();
    });
}

pub fn toggle_maximized(ui: &AppWindow) {
    ui.window().with_winit_window(|w| {
        w.set_maximized(!w.is_maximized());
    });
    on_resized(ui);
}

/// `dir`: 0 E, 1 N, 2 NE, 3 NW, 4 S, 5 SE, 6 SW, 7 W (as `ResizeEdge.dir` in app.slint).
pub fn resize(ui: &AppWindow, dir: i32) {
    use winit::window::ResizeDirection as R;
    let d = match dir {
        0 => R::East,
        1 => R::North,
        2 => R::NorthEast,
        3 => R::NorthWest,
        4 => R::South,
        5 => R::SouthEast,
        6 => R::SouthWest,
        _ => R::West,
    };
    ui.window().with_winit_window(|w| {
        let _ = w.drag_resize_window(d);
    });
}

#[cfg(target_os = "macos")]
mod mac {
    use super::*;
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2_app_kit::{
        NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSView, NSWindow,
        NSWindowButton, NSWindowStyleMask, NSWindowTitleVisibility,
    };
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

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

    pub fn dress(ui: &AppWindow) {
        if super::is_over() {
            return;
        }
        let Some(win) = ns_window(ui) else { return };
        let r = objc2::exception::catch(std::panic::AssertUnwindSafe(|| {
            if !win.titlebarAppearsTransparent() {
                win.setTitlebarAppearsTransparent(true);
            }
            if win.titleVisibility() != NSWindowTitleVisibility::Hidden {
                win.setTitleVisibility(NSWindowTitleVisibility::Hidden);
            }
            if !win
                .styleMask()
                .contains(NSWindowStyleMask::FullSizeContentView)
            {
                win.setStyleMask(win.styleMask() | NSWindowStyleMask::FullSizeContentView);
            }
            // SAFETY: a constant AppKit appearance name.
            // SAFETY: the appearance names are AppKit's constants.
            let name = if super::DARK.with(|d| d.get()) {
                unsafe { NSAppearanceNameDarkAqua }
            } else {
                unsafe { NSAppearanceNameAqua }
            };
            if let Some(dark) = NSAppearance::appearanceNamed(name) {
                // SAFETY: NSWindow conforms to NSAppearanceCustomization.
                let _: () = unsafe { msg_send![&*win, setAppearance: &*dark] };
            }
        }));
        if let Err(e) = r {
            eprintln!("[frame] dress threw {e:?}");
        }
    }

    /// Centres the traffic lights on our 52 pt bar.
    pub fn place_lights(ui: &AppWindow) {
        if super::is_over() {
            return;
        }
        let Some(win) = ns_window(ui) else { return };
        // In full screen the title bar is the system's own (it slides in from the top).
        if win.styleMask().contains(NSWindowStyleMask::FullScreen) {
            return;
        }
        let r = objc2::exception::catch(std::panic::AssertUnwindSafe(|| {
            let buttons: Vec<_> = [
                NSWindowButton::CloseButton,
                NSWindowButton::MiniaturizeButton,
                NSWindowButton::ZoomButton,
            ]
            .into_iter()
            .filter_map(|b| win.standardWindowButton(b))
            .collect();
            let Some(first) = buttons.first() else { return };
            // SAFETY: plain view-hierarchy queries on the main thread.
            let Some(title_view) = (unsafe { first.superview() }) else {
                return;
            };
            let Some(container) = (unsafe { title_view.superview() }) else {
                return;
            };
            let win_h = win.frame().size.height;
            let mut cf = container.frame();
            cf.size.height = BAR;
            cf.origin.y = win_h - BAR;
            container.setFrame(cf);
            let mut tf = title_view.frame();
            tf.size.height = BAR;
            tf.origin.y = 0.0;
            title_view.setFrame(tf);
            let step = if buttons.len() > 1 {
                buttons[1].frame().origin.x - buttons[0].frame().origin.x
            } else {
                20.0
            };
            for (i, b) in buttons.iter().enumerate() {
                let f = b.frame();
                let mut o = f.origin;
                o.x = 16.0 + i as f64 * step;
                o.y = (BAR - f.size.height) / 2.0;
                b.setFrameOrigin(o);
            }
        }));
        if let Err(e) = r {
            eprintln!("[frame] place_lights threw {e:?}");
        }
    }

    pub fn titlebar_state(ui: &AppWindow) -> Option<(bool, String)> {
        let win = ns_window(ui)?;
        let t = win.titlebarAppearsTransparent();
        let h = win.titleVisibility() == NSWindowTitleVisibility::Hidden;
        let f = win
            .styleMask()
            .contains(NSWindowStyleMask::FullSizeContentView);
        Some((
            t && h && f,
            format!("transparent {t} · title hidden {h} · full-size content {f}"),
        ))
    }

    /// For the self-test: vertical centre of the close button, from the top of the window.
    pub fn lights_centre(ui: &AppWindow) -> Option<f64> {
        let win = ns_window(ui)?;
        let b = win.standardWindowButton(NSWindowButton::CloseButton)?;
        // SAFETY: plain view-hierarchy query on the main thread.
        let tv = unsafe { b.superview() }?;
        let c = unsafe { tv.superview() }?;
        let f = b.frame();
        let in_container = f.origin.y + f.size.height / 2.0 + tv.frame().origin.y;
        let from_bottom = c.frame().origin.y + in_container;
        Some(win.frame().size.height - from_bottom)
    }
}

/// For the self-test (Windows): the corner preference DWM holds for our window.
pub fn corners_rounded(ui: &AppWindow) -> Option<bool> {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::Graphics::Dwm::{
            DWM_WINDOW_CORNER_PREFERENCE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
            DwmGetWindowAttribute,
        };
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        ui.window()
            .with_winit_window(|w| {
                let h = w.window_handle().ok()?;
                let RawWindowHandle::Win32(h) = h.as_raw() else {
                    return None;
                };
                let hwnd = HWND(h.hwnd.get() as *mut core::ffi::c_void);
                let mut v = DWM_WINDOW_CORNER_PREFERENCE(0);
                // SAFETY: a live window of this process; out-pointer to a DWORD-sized value.
                unsafe {
                    DwmGetWindowAttribute(
                        hwnd,
                        DWMWA_WINDOW_CORNER_PREFERENCE,
                        (&mut v as *mut DWM_WINDOW_CORNER_PREFERENCE).cast(),
                        std::mem::size_of_val(&v) as u32,
                    )
                }
                .ok()?;
                Some(v == DWMWCP_ROUND)
            })
            .flatten()
    }
    #[cfg(not(windows))]
    {
        let _ = ui;
        None
    }
}

/// For the self-test (macOS): transparent title bar, hidden title, full-size content.
pub fn titlebar_state(ui: &AppWindow) -> Option<(bool, String)> {
    #[cfg(target_os = "macos")]
    {
        mac::titlebar_state(ui)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = ui;
        None
    }
}

/// For the self-test (macOS): the traffic lights' centre from the top of the window.
pub fn lights_centre(ui: &AppWindow) -> Option<f64> {
    #[cfg(target_os = "macos")]
    {
        mac::lights_centre(ui)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = ui;
        None
    }
}
