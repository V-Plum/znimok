//! The capture overlay (ZK-39/40, both OSes): the display under the pointer is frozen, shown
//! full-screen in a borderless top-most window, and the user picks what to keep:
//! drag = region, click = the window under the pointer (or the whole screen over the desktop),
//! Space = whole screen, Enter = what is highlighted, Shift on release = straight to the
//! clipboard and the library, Esc / right click / the capture key again = cancel.
//! Guides through the pointer across the whole screen are the cursor (Little Helpers).
//! The magnifier is off until the wheel turns it on (×4 → ×8 → ×16 → off, as in LH CAPS-86);
//! it is drawn here pixel by pixel (nearest neighbour, grid from ×8, the centre pixel boxed).
//! Regions and the whole screen are cut from the frozen frame (instant, identical on both OSes);
//! a clicked window is captured alone (without what overlaps it), falling back to the cut.
//! Not yet: countdown, "over the screen" (Alt, ZK-58), regions spanning several displays.

use std::cell::RefCell;

use slint::ComponentHandle;

use crate::capture::{Frozen, PxRect};
use crate::{Overlay, io};

struct Session {
    ui: Overlay,
    frozen: Frozen,
    /// Frame pixels per logical pixel of the overlay window, horizontally and vertically.
    kx: f32,
    ky: f32,
    drag_start: Option<(f32, f32)>,
    dragging: bool,
    sel: Option<PxRect>,
    /// Id of the highlighted window when `sel` is a window.
    window: Option<u64>,
    /// 0 = magnifier off, else 4 / 8 / 16 screen pixels per frame pixel.
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

/// macOS keeps ordinary windows below the menu bar, so a window the size of the display ends up
/// shifted down and squeezed (the frozen menu bar showed twice, owner's live test 28.09).
/// "Simple fullscreen" covers the whole display — menu bar and Dock hidden — without the
/// animated move to a separate Space that native fullscreen makes.
fn cover_display(ui: &Overlay, on: bool) {
    #[cfg(target_os = "macos")]
    {
        use slint::winit_030::WinitWindowAccessor;
        use slint::winit_030::winit::platform::macos::WindowExtMacOS;
        ui.window().with_winit_window(|w| {
            w.set_simple_fullscreen(on);
        });
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (ui, on);
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
                match s.zoom {
                    0 => 4,
                    z => (z * 2).min(16),
                }
            } else if s.zoom <= 4 {
                0
            } else {
                s.zoom / 2
            };
            let (x, y) = s.last_pointer;
            s.update_lens(x, y);
            None
        })
    });
    ui.show()?;
    cover_display(&ui, true);
    {
        use slint::winit_030::WinitWindowAccessor;
        ui.window().with_winit_window(|w| w.focus_window());
    }
    ui.invoke_grab_focus();
    let session = Session {
        kx: 1.0,
        ky: 1.0,
        ui,
        frozen,
        drag_start: None,
        dragging: false,
        sel: None,
        window: None,
        zoom: 0,
        last_pointer: (-100.0, -100.0),
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
    // Give the menu bar and the Dock back before the window goes.
    cover_display(&session.ui, false);
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

/// Frame pixels across the magnifier: odd, so the pointer's pixel sits in the middle
/// (Little Helpers `RgnLensSide`: 144 points of lens, at least 5 pixels).
fn lens_pixels(zoom: u32, scale: f32) -> i32 {
    let mut n = (144.0 * scale) as i32 / zoom.max(1) as i32;
    if n % 2 == 0 {
        n -= 1;
    }
    n.max(5)
}

/// The magnifier picture: `n × n` frame pixels around (cx, cy), each `z × z` screen pixels,
/// a faint grid from ×8, the centre pixel boxed white-on-black (visible on any background).
fn render_lens(f: &znimok_core::Raster, cx: i32, cy: i32, n: i32, z: i32) -> (Vec<u8>, u32) {
    let side = (n * z) as usize;
    let mut out = vec![0u8; side * side * 4];
    let (fw, fh) = (f.width as i32, f.height as i32);
    for j in 0..n {
        for i in 0..n {
            let (sx, sy) = (cx - n / 2 + i, cy - n / 2 + j);
            let px = if sx >= 0 && sy >= 0 && sx < fw && sy < fh {
                let o = ((sy * fw + sx) * 4) as usize;
                [f.rgba[o], f.rgba[o + 1], f.rgba[o + 2], 255]
            } else {
                [0, 0, 0, 255]
            };
            for y in 0..z {
                let row = ((j * z + y) as usize * side + (i * z) as usize) * 4;
                for x in 0..z as usize {
                    out[row + x * 4..row + x * 4 + 4].copy_from_slice(&px);
                }
            }
        }
    }
    let mut put = |x: i32, y: i32, c: [u8; 3], a: u8| {
        if x < 0 || y < 0 || x >= side as i32 || y >= side as i32 {
            return;
        }
        let o = (y as usize * side + x as usize) * 4;
        for (k, ck) in c.iter().enumerate() {
            let v = out[o + k] as u32 * (255 - a as u32) + *ck as u32 * a as u32;
            out[o + k] = (v / 255) as u8;
        }
    };
    if z >= 8 {
        for i in 1..n {
            for t in 0..side as i32 {
                put(i * z, t, [0, 0, 0], 55);
                put(t, i * z, [0, 0, 0], 55);
            }
        }
    }
    let c = (n / 2) * z;
    for t in -1..=z {
        for (x, y) in [
            (c + t, c - 1),
            (c + t, c + z),
            (c - 1, c + t),
            (c + z, c + t),
        ] {
            put(x, y, [0, 0, 0], 255);
        }
    }
    for t in 0..z {
        for (x, y) in [
            (c + t, c),
            (c + t, c + z - 1),
            (c, c + t),
            (c + z - 1, c + t),
        ] {
            put(x, y, [255, 255, 255], 255);
        }
    }
    let s = side as i32;
    for t in 0..s {
        for (x, y) in [(t, 0), (t, s - 1), (0, t), (s - 1, t)] {
            put(x, y, [255, 255, 255], 235);
        }
    }
    (out, side as u32)
}

impl Session {
    fn update_k(&mut self) {
        let size = self.ui.window().size();
        let sf = self.ui.window().scale_factor().max(0.1);
        let (lw, lh) = (size.width as f32 / sf, size.height as f32 / sf);
        if lw > 1.0 && lh > 1.0 {
            self.kx = self.frozen.raster.width as f32 / lw;
            self.ky = self.frozen.raster.height as f32 / lh;
        }
    }

    fn px(&self, x: f32, y: f32) -> (i32, i32) {
        ((x * self.kx).floor() as i32, (y * self.ky).floor() as i32)
    }

    fn window_at(&self, x: i32, y: i32) -> Option<(PxRect, u64)> {
        self.frozen
            .windows
            .iter()
            .find(|w| w.rect.contains(x, y))
            .map(|w| (w.rect, w.id))
    }

    /// Colour shown on screen at a frame pixel: the frozen frame, darkened by the veil outside
    /// the selection (veil = black at 0x73 / 255, see `app.slint`).
    fn shown(&self, x: i32, y: i32) -> [u8; 3] {
        let f = &self.frozen.raster;
        let o = ((y * f.width as i32 + x) * 4) as usize;
        let c = [f.rgba[o], f.rgba[o + 1], f.rgba[o + 2]];
        let lit = self.sel.is_some_and(|r| r.contains(x, y));
        if lit {
            c
        } else {
            c.map(|v| ((v as u32 * (255 - 0x73)) / 255) as u8)
        }
    }

    /// A guide line: black over light pixels, white over dark ones (owner, 28.09).
    fn guide(&self, horizontal: bool, at: i32) -> slint::Image {
        let (fw, fh) = (
            self.frozen.raster.width as i32,
            self.frozen.raster.height as i32,
        );
        let len = if horizontal { fw } else { fh };
        let mut rgba = Vec::with_capacity(len as usize * 4);
        for t in 0..len {
            let (x, y) = if horizontal {
                (t, at.clamp(0, fh - 1))
            } else {
                (at.clamp(0, fw - 1), t)
            };
            let [r, g, b] = self.shown(x, y);
            // Relative luminance (sRGB weights) against the middle grey.
            let lum = 0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32;
            let v = if lum > 128.0 { 0 } else { 255 };
            rgba.extend_from_slice(&[v, v, v, 230]);
        }
        let (w, h) = if horizontal {
            (len as u32, 1)
        } else {
            (1, len as u32)
        };
        let buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(&rgba, w, h);
        slint::Image::from_rgba8(buf)
    }

    fn update_lens(&mut self, x: f32, y: f32) {
        let ui = &self.ui;
        ui.set_pointer_x(x);
        ui.set_pointer_y(y);
        let (gx, gy) = self.px(x, y);
        ui.set_guide_h(self.guide(true, gy));
        ui.set_guide_v(self.guide(false, gx));
        if self.zoom == 0 {
            ui.set_lens_visible(false);
            return;
        }
        let (fw, fh) = (
            self.frozen.raster.width as i32,
            self.frozen.raster.height as i32,
        );
        let (px, py) = self.px(x, y);
        let (px, py) = (px.clamp(0, fw - 1), py.clamp(0, fh - 1));
        // One frame pixel = `zoom` screen (= frame) pixels; the lens is sized in frame pixels.
        let n = lens_pixels(self.zoom, self.kx);
        let (rgba, side) = render_lens(&self.frozen.raster, px, py, n, self.zoom as i32);
        let buf =
            slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(&rgba, side, side);
        ui.set_lens(slint::Image::from_rgba8(buf));
        ui.set_lens_size(side as f32 / self.kx.max(0.01));
        let coords = match self.sel.filter(|_| self.dragging) {
            Some(r) => format!("{px}, {py}   {} × {}", r.w, r.h),
            None => format!("{px}, {py}   ×{}", self.zoom),
        };
        ui.set_lens_coords(coords.into());
        let o = ((py * fw + px) * 4) as usize;
        let p = &self.frozen.raster.rgba[o..o + 3];
        ui.set_lens_hex(format!("#{:02X}{:02X}{:02X}", p[0], p[1], p[2]).into());
        ui.set_lens_color(slint::Color::from_rgb_u8(p[0], p[1], p[2]));
        ui.set_lens_visible(true);
    }

    fn show(&self) {
        let ui = &self.ui;
        match self.sel {
            Some(r) => {
                ui.set_has_sel(true);
                ui.set_sel_x(r.x as f32 / self.kx.max(0.01));
                ui.set_sel_y(r.y as f32 / self.ky.max(0.01));
                ui.set_sel_w(r.w as f32 / self.kx.max(0.01));
                ui.set_sel_h(r.h as f32 / self.ky.max(0.01));
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
                self.show();
                self.update_lens(x, y);
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
