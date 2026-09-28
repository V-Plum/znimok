//! The capture overlay (ZK-39/40, both OSes): the display under the pointer is frozen, shown
//! full-screen in a borderless top-most window, and the user picks what to keep:
//! drag = region, click = the window under the pointer (or the whole screen over the desktop),
//! Space = whole screen, Enter = what is highlighted, Shift on release = straight to the
//! clipboard and the library, Esc / right click / the capture key again = cancel.
//! A magnifier (×4/×8/×16, mouse wheel) follows the pointer with the pixel coordinates.
//! Regions and the whole screen are cut from the frozen frame (instant, identical on both OSes);
//! a clicked window is captured alone (without what overlaps it), falling back to the cut.
//! Not yet: countdown, "over the screen" (Alt, ZK-58), regions spanning several displays.

use std::cell::RefCell;

use slint::ComponentHandle;

use crate::capture::{Frozen, PxRect};
use crate::{Overlay, io};

/// Side of the magnifier in logical pixels (matches `app.slint`).
const LENS: f32 = 132.0;

struct Session {
    ui: Overlay,
    frozen: Frozen,
    /// Frame pixels per logical pixel of the overlay window.
    k: f32,
    drag_start: Option<(f32, f32)>,
    dragging: bool,
    sel: Option<PxRect>,
    /// Id of the highlighted window when `sel` is a window.
    window: Option<u64>,
    zoom: u32,
    last_pointer: (f32, f32),
    /// The editor window was visible before the capture (show it again on cancel).
    editor_was_visible: bool,
}

thread_local! {
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
}

/// The open overlay window, for the self-test.
pub fn handle() -> Option<Overlay> {
    SESSION.with(|s| s.borrow().as_ref().map(|s| s.ui.clone_strong()))
}

pub fn is_open() -> bool {
    SESSION.with(|s| s.borrow().is_some())
}

/// Closes the overlay without capturing (the capture key pressed again).
pub fn cancel() {
    with_session(|_| Some(Outcome::Cancel));
}

/// Shows the overlay over the frozen display. Runs on the UI thread.
pub fn open(frozen: Frozen, editor_was_visible: bool) -> Result<(), slint::PlatformError> {
    let ui = Overlay::new()?;
    let (w, h) = (frozen.raster.width, frozen.raster.height);
    let buf =
        slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(&frozen.raster.rgba, w, h);
    ui.set_shot(slint::Image::from_rgba8(buf));
    let b = frozen.bounds;
    // Desktop units are physical pixels on Windows (per-monitor DPI aware) and points on macOS.
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
    ui.on_pointer(|kind, x, y, shift| with_session(|s| s.pointer(kind, x, y, shift)));
    ui.on_key(|text, shift| with_session(|s| s.key(&text, shift)));
    ui.on_wheel(|dy| {
        with_session(|s| {
            s.zoom = if dy > 0.0 {
                (s.zoom * 2).min(16)
            } else {
                (s.zoom / 2).max(4)
            };
            let (x, y) = s.last_pointer;
            s.update_lens(x, y);
            None
        })
    });
    ui.show()?;
    {
        use slint::winit_030::WinitWindowAccessor;
        ui.window().with_winit_window(|w| w.focus_window());
    }
    ui.invoke_grab_focus();
    let session = Session {
        k: 1.0,
        ui,
        frozen,
        drag_start: None,
        dragging: false,
        sel: None,
        window: None,
        zoom: 8,
        last_pointer: (0.0, 0.0),
        editor_was_visible,
    };
    SESSION.with(|s| *s.borrow_mut() = Some(session));
    Ok(())
}

enum Outcome {
    /// Rectangle in frame pixels, source label, window id (for an unoccluded capture), Shift.
    Keep(PxRect, &'static str, Option<u64>, bool),
    Cancel,
}

/// Runs `f` on the open session; finishes the capture when it returns an outcome.
fn with_session(f: impl FnOnce(&mut Session) -> Option<Outcome>) {
    let outcome = SESSION.with(|s| s.borrow_mut().as_mut().and_then(f));
    let Some(outcome) = outcome else { return };
    let Some(session) = SESSION.with(|s| s.borrow_mut().take()) else {
        return;
    };
    let _ = session.ui.hide();
    let Session {
        frozen,
        editor_was_visible,
        ..
    } = session;
    match outcome {
        Outcome::Cancel => {
            // (`invoke_from_event_loop` wakes the loop; a zero timer waits for the next event.)
            let _ = slint::invoke_from_event_loop(move || {
                crate::with_ctx(|_, ui| {
                    if editor_was_visible {
                        crate::show_window(ui);
                    }
                })
            });
        }
        Outcome::Keep(rect, source, Some(id), shift) => {
            // The window alone: capture it now that the overlay is gone; on failure keep the
            // cut from the frozen frame (it may include what overlapped the window).
            let fallback = frozen.crop(rect);
            std::thread::spawn(move || {
                // Give the compositor a moment to take the overlay off the screen.
                std::thread::sleep(std::time::Duration::from_millis(80));
                let raster = crate::capture::capture_window(id).ok().or(fallback);
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(r) = raster {
                        deliver(r, source, shift, editor_was_visible);
                    }
                });
            });
        }
        Outcome::Keep(rect, source, None, shift) => {
            let raster = frozen.crop(rect);
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(r) = raster {
                    deliver(r, source, shift, editor_was_visible);
                }
            });
        }
    }
}

/// The picked pixels go to the editor, or with Shift to the clipboard and the library.
fn deliver(
    raster: znimok_core::Raster,
    source: &str,
    to_clipboard: bool,
    editor_was_visible: bool,
) {
    crate::with_ctx(|a, ui| {
        if to_clipboard {
            let (w, h) = (raster.width, raster.height);
            let copied = io::copy_image(w, h, raster.rgba.clone());
            let saved = a.store_quietly(ui, raster, source);
            if editor_was_visible {
                crate::show_window(ui);
            }
            let msg = match (copied, saved) {
                (Ok(()), Ok(())) => {
                    let mut args = znimok_i18n::FluentArgs::new();
                    args.set("width", w);
                    args.set("height", h);
                    a.tr.tr_args("pill-where", &args)
                }
                (Err(e), _) | (_, Err(e)) => e,
            };
            a.toast(ui, msg);
        } else {
            crate::show_window(ui);
            a.new_document(ui, raster, source, None);
        }
    });
}

impl Session {
    fn update_k(&mut self) {
        let size = self.ui.window().size();
        let sf = self.ui.window().scale_factor().max(0.1);
        let logical_w = size.width as f32 / sf;
        if logical_w > 1.0 {
            self.k = self.frozen.raster.width as f32 / logical_w;
        }
    }

    fn px(&self, x: f32, y: f32) -> (i32, i32) {
        ((x * self.k).floor() as i32, (y * self.k).floor() as i32)
    }

    fn window_at(&self, x: i32, y: i32) -> Option<(PxRect, u64)> {
        self.window_info(x, y).map(|w| (w.rect, w.id))
    }

    fn window_info(&self, x: i32, y: i32) -> Option<&crate::capture::FrozenWindow> {
        self.frozen.windows.iter().find(|w| w.rect.contains(x, y))
    }

    /// Magnifier: an odd number of frame pixels around the pointer, so one sits in the middle.
    fn update_lens(&mut self, x: f32, y: f32) {
        let (px, py) = self.px(x, y);
        let mut n = (LENS / self.zoom as f32).round() as i32;
        if n % 2 == 0 {
            n += 1;
        }
        let (fw, fh) = (
            self.frozen.raster.width as i32,
            self.frozen.raster.height as i32,
        );
        let ui = &self.ui;
        ui.set_pointer_x(x);
        ui.set_pointer_y(y);
        ui.set_clip_size(n);
        ui.set_clip_x((px - n / 2).clamp(0, (fw - n).max(0)));
        ui.set_clip_y((py - n / 2).clamp(0, (fh - n).max(0)));
        ui.set_lens_label(
            format!(
                "{}, {}  ×{}",
                px.clamp(0, fw - 1),
                py.clamp(0, fh - 1),
                self.zoom
            )
            .into(),
        );
        ui.set_lens_visible(true);
    }

    fn show(&self) {
        let ui = &self.ui;
        match self.sel {
            Some(r) => {
                let k = self.k.max(0.01);
                ui.set_has_sel(true);
                ui.set_sel_x(r.x as f32 / k);
                ui.set_sel_y(r.y as f32 / k);
                ui.set_sel_w(r.w as f32 / k);
                ui.set_sel_h(r.h as f32 / k);
                // A window shows its title too (shortened), a region only its size.
                let title = self
                    .window
                    .and_then(|id| self.frozen.windows.iter().find(|w| w.id == id))
                    .map(|w| w.title.trim())
                    .filter(|t| !t.is_empty())
                    .map(|t| {
                        let short: String = t.chars().take(48).collect();
                        if short.len() < t.len() {
                            format!("{short}… · ")
                        } else {
                            format!("{short} · ")
                        }
                    })
                    .unwrap_or_default();
                ui.set_sel_label(format!("{title}{} × {}", r.w, r.h).into());
                ui.set_is_window(self.window.is_some());
            }
            None => ui.set_has_sel(false),
        }
    }

    fn pointer(&mut self, kind: i32, x: f32, y: f32, shift: bool) -> Option<Outcome> {
        self.update_k();
        self.last_pointer = (x, y);
        let (px, py) = self.px(x, y);
        match kind {
            // left down
            0 => {
                self.drag_start = Some((x, y));
                self.dragging = false;
                None
            }
            // move
            1 => {
                match self.drag_start {
                    Some((sx, sy)) => {
                        if !self.dragging && ((x - sx).abs() > 4.0 || (y - sy).abs() > 4.0) {
                            self.dragging = true;
                        }
                        if self.dragging {
                            let (ax, ay) = self.px(sx, sy);
                            self.sel = Some(PxRect {
                                x: ax.min(px),
                                y: ay.min(py),
                                w: (px - ax).abs().max(1),
                                h: (py - ay).abs().max(1),
                            });
                            self.window = None;
                        }
                    }
                    None => {
                        let hit = self.window_at(px, py);
                        self.sel = hit.map(|(r, _)| r);
                        self.window = hit.map(|(_, id)| id);
                    }
                }
                self.update_lens(x, y);
                self.show();
                None
            }
            // left up
            2 => {
                let was_drag = self.dragging && self.drag_start.is_some();
                self.drag_start = None;
                self.dragging = false;
                if was_drag {
                    let r = self.sel?;
                    return (r.w >= 3 && r.h >= 3)
                        .then_some(Outcome::Keep(r, "region", None, shift));
                }
                Some(match self.window_at(px, py) {
                    Some((r, id)) => Outcome::Keep(r, "window", Some(id), shift),
                    None => Outcome::Keep(self.frozen.whole(), "screen", None, shift),
                })
            }
            // right button: cancel
            3 | 4 => Some(Outcome::Cancel),
            _ => None,
        }
    }

    fn key(&mut self, text: &str, shift: bool) -> Option<Outcome> {
        match text {
            "\u{1b}" => Some(Outcome::Cancel),
            " " => Some(Outcome::Keep(self.frozen.whole(), "screen", None, shift)),
            "\n" | "\r" => match self.sel {
                Some(r) => Some(match self.window {
                    Some(id) => Outcome::Keep(r, "window", Some(id), shift),
                    None => Outcome::Keep(r, "region", None, shift),
                }),
                None => Some(Outcome::Keep(self.frozen.whole(), "screen", None, shift)),
            },
            _ => None,
        }
    }
}
