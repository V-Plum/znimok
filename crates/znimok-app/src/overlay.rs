//! The capture overlay (ZK-39, first cut, both OSes): the display under the pointer is frozen,
//! shown full-screen in a borderless top-most window, and the user picks what to keep:
//! drag = region, click = the window under the pointer (or the whole screen over the desktop),
//! Space = whole screen, Shift on release = straight to the clipboard and the library,
//! Esc / right click = cancel. The result is cut from the frozen frame, so it is instant and
//! identical on Windows and macOS. Not yet: the magnifier (ZK-40), "over the screen" (Alt, ZK-58),
//! spanning several displays.

use std::cell::RefCell;

use slint::ComponentHandle;

use crate::capture::{Frozen, PxRect};
use crate::{Overlay, io};

struct Session {
    ui: Overlay,
    frozen: Frozen,
    /// Frame pixels per logical pixel of the overlay window.
    k: f32,
    drag_start: Option<(f32, f32)>,
    dragging: bool,
    sel: Option<PxRect>,
    is_window: bool,
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
    {
        ui.on_pointer(|kind, x, y, shift| with_session(|s| s.pointer(kind, x, y, shift)));
        ui.on_key(|text, shift| with_session(|s| s.key(&text, shift)));
    }
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
        is_window: false,
        editor_was_visible,
    };
    SESSION.with(|s| *s.borrow_mut() = Some(session));
    Ok(())
}

enum Outcome {
    Keep(PxRect, &'static str, bool),
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
    // Leave the overlay's event handler before opening the editor.
    // (`invoke_from_event_loop` wakes the loop; a zero timer waits for the next wake-up.)
    let _ = slint::invoke_from_event_loop(move || {
        crate::with_ctx(|a, ui| match outcome {
            Outcome::Cancel => {
                if editor_was_visible {
                    crate::show_window(ui);
                }
            }
            Outcome::Keep(rect, source, to_clipboard) => {
                let Some(raster) = frozen.crop(rect) else {
                    return;
                };
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
            }
        });
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
        ((x * self.k).round() as i32, (y * self.k).round() as i32)
    }

    fn window_at(&self, x: i32, y: i32) -> Option<PxRect> {
        self.frozen
            .windows
            .iter()
            .find(|(r, _)| r.contains(x, y))
            .map(|(r, _)| *r)
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
                ui.set_sel_label(format!("{} × {}", r.w, r.h).into());
                ui.set_is_window(self.is_window);
            }
            None => ui.set_has_sel(false),
        }
    }

    fn pointer(&mut self, kind: i32, x: f32, y: f32, shift: bool) -> Option<Outcome> {
        self.update_k();
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
                            self.is_window = false;
                        }
                    }
                    None => {
                        self.sel = self.window_at(px, py);
                        self.is_window = self.sel.is_some();
                    }
                }
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
                    return (r.w >= 3 && r.h >= 3).then_some(Outcome::Keep(r, "region", shift));
                }
                Some(match self.window_at(px, py) {
                    Some(r) => Outcome::Keep(r, "window", shift),
                    None => Outcome::Keep(self.frozen.whole(), "screen", shift),
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
            " " => Some(Outcome::Keep(self.frozen.whole(), "screen", shift)),
            "\n" | "\r" => match self.sel {
                Some(r) => Some(Outcome::Keep(
                    r,
                    if self.is_window { "window" } else { "region" },
                    shift,
                )),
                None => Some(Outcome::Keep(self.frozen.whole(), "screen", shift)),
            },
            _ => None,
        }
    }
}
