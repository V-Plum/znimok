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

pub fn after_show(ui: &AppWindow) {
    #[cfg(windows)]
    ui.window().with_winit_window(|w| {
        use winit::platform::windows::WindowExtWindows;
        w.set_undecorated_shadow(true);
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
    mac::place_lights(ui);
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
        NSAppearance, NSAppearanceNameDarkAqua, NSView, NSWindow, NSWindowButton,
        NSWindowStyleMask, NSWindowTitleVisibility,
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
        let Some(win) = ns_window(ui) else { return };
        let r = objc2::exception::catch(std::panic::AssertUnwindSafe(|| {
            win.setTitlebarAppearsTransparent(true);
            win.setTitleVisibility(NSWindowTitleVisibility::Hidden);
            win.setStyleMask(win.styleMask() | NSWindowStyleMask::FullSizeContentView);
            // SAFETY: a constant AppKit appearance name.
            if let Some(dark) = NSAppearance::appearanceNamed(unsafe { NSAppearanceNameDarkAqua }) {
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
        let Some(win) = ns_window(ui) else { return };
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
