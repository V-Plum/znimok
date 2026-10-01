//! Scripted self-test: `ZNIMOK_SELFTEST=<dir> znimok-app <image>` drives the real window
//! through the same handlers the UI calls (pointer, keys, text, save, library, export), takes
//! window snapshots into `<dir>` and writes `report.txt`. Exit code 0 = every check passed.
//! Needs no desktop session and no synthetic input, so it runs over an idle RDP connection and
//! in CI.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use slint::{ComponentHandle, Model};

use crate::AppWindow;
use crate::app::App;

type Shared = Rc<RefCell<App>>;
type Step = Box<dyn FnOnce(&Shared, &AppWindow, &mut Report)>;

#[derive(Default)]
pub struct Report {
    dir: PathBuf,
    lines: Vec<String>,
    failed: u32,
}

impl Report {
    fn check(&mut self, name: &str, ok: bool, detail: String) {
        let line = format!("{} {name}: {detail}", if ok { "ok  " } else { "FAIL" });
        println!("{line}");
        self.lines.push(line);
        if !ok {
            self.failed += 1;
        }
    }

    fn snapshot(&mut self, ui: &AppWindow, name: &str) {
        self.snapshot_window(ui.window(), name);
    }

    fn snapshot_window(&mut self, window: &slint::Window, name: &str) {
        match window.take_snapshot() {
            Ok(buf) => {
                let path = self.dir.join(format!("{name}.png"));
                let img =
                    image::RgbaImage::from_raw(buf.width(), buf.height(), buf.as_bytes().to_vec());
                let saved = img.map(|i| i.save(&path).is_ok()).unwrap_or(false);
                self.check(
                    &format!("snapshot {name}"),
                    saved,
                    format!("{}×{}", buf.width(), buf.height()),
                );
            }
            Err(e) => self.check(&format!("snapshot {name}"), false, e.to_string()),
        }
    }
}

fn drag(app: &Shared, ui: &AppWindow, from: (f32, f32), to: (f32, f32)) {
    let mut a = app.borrow_mut();
    a.pointer(ui, 0, from.0, from.1, 0, false, false);
    for i in 1..=12 {
        let t = i as f32 / 12.0;
        a.pointer(
            ui,
            1,
            from.0 + (to.0 - from.0) * t,
            from.1 + (to.1 - from.1) * t,
            0,
            false,
            false,
        );
    }
    a.pointer(ui, 2, to.0, to.1, 0, false, false);
}

/// ZK-130: what the canvas shows after partial repaints equals a full repaint.
fn same_as_full(app: &Shared, ui: &AppWindow, r: &mut Report, when: &str) {
    let bad = app.borrow_mut().canvas_vs_full(ui);
    r.check(
        &format!("canvas: partial repaint = full repaint ({when})"),
        bad == 0,
        format!("{bad} pixels differ"),
    );
}

/// Ten real clicks on «Changes apply immediately» in the settings header (ZK-140).
fn ten_clicks_on_applied(ui: &AppWindow) {
    use slint::platform::{PointerEventButton, WindowEvent};
    let win = ui.window();
    let w = win.size().width as f32 / win.scale_factor();
    let at = slint::LogicalPosition::new(w - 234.0, 26.0);
    win.dispatch_event(WindowEvent::PointerMoved { position: at });
    for _ in 0..10 {
        win.dispatch_event(WindowEvent::PointerPressed {
            position: at,
            button: PointerEventButton::Left,
        });
        win.dispatch_event(WindowEvent::PointerReleased {
            position: at,
            button: PointerEventButton::Left,
        });
    }
}

/// The window a step works in: the newest editor with a document, else the library (ZK-107).
fn current(lib_app: &Shared, lib_ui: &AppWindow) -> (Shared, AppWindow) {
    crate::wins::newest_editor().unwrap_or_else(|| (lib_app.clone(), lib_ui.clone_strong()))
}

/// The editor window opened by the call just made in this step (a document opens in a window
/// of its own, ZK-107), or the window given when there is none.
fn opened(app: &Shared, ui: &AppWindow) -> (Shared, AppWindow) {
    current(app, ui)
}

/// Whether Znimok's own text reader is here (it reads Ukrainian; the system's fallback on
/// Windows may read only English).
fn uk_reader() -> bool {
    cfg!(target_os = "macos") || znimok_models::ocr::helper::find().is_some()
}

/// ZK-184/185: a clean page with real text in its pixels — Ukrainian and English.
fn text_page() -> znimok_core::Raster {
    use znimok_core::{Align, Data, Document, IRect, Object, Raster, Rgb, Style};
    let mut page = Document::from_raster("text", Raster::solid(1400, 420, Rgb::WHITE));
    for (i, t) in ["Znimok reads text 2026", "Привіт, світ зі знімка"]
        .iter()
        .enumerate()
    {
        page.push(
            Object::new(
                IRect::new(60, 60 + i as i32 * 150, 1200, 90),
                Data::Text {
                    text: t.to_string(),
                    size: 64,
                    bold: false,
                    italic: false,
                    align: Align::Left,
                    box_w: 0,
                },
            )
            .with_style(Style {
                color: Rgb::new(20, 22, 26),
                ..Style::default()
            }),
        );
    }
    let mut renderer = znimok_render::Renderer::new();
    let mut pix = znimok_render::vello_cpu::Pixmap::new(1, 1);
    renderer.render(&page, znimok_render::View::one_to_one(&page), &mut pix);
    Raster::new(
        pix.width() as u32,
        pix.height() as u32,
        znimok_render::pixmap_to_rgba(&pix),
    )
}

fn click(app: &Shared, ui: &AppWindow, at: (f32, f32)) {
    let mut a = app.borrow_mut();
    a.pointer(ui, 0, at.0, at.1, 0, false, false);
    a.pointer(ui, 2, at.0, at.1, 0, false, false);
}

fn key(app: &Shared, ui: &AppWindow, text: &str, ctrl: bool, shift: bool) {
    let _ = app.borrow_mut().key(ui, text, ctrl, shift);
}

fn text_of(app: &Shared, id: u32) -> Option<String> {
    app.borrow()
        .s
        .as_ref()
        .and_then(|s| match &s.ed.doc.get(id)?.data {
            znimok_core::Data::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
}

fn count(app: &Shared) -> usize {
    app.borrow()
        .s
        .as_ref()
        .map(|s| s.ed.doc.objects.len())
        .unwrap_or(0)
}

fn centre(ui: &AppWindow) -> (f32, f32) {
    (ui.get_canvas_width() / 2.0, ui.get_canvas_height() / 2.0)
}

/// The self-test's result when it ends through the event loop (see `ZNIMOK_SELFTEST_REAL_QUIT`).
pub static EXIT_CODE: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

pub fn start(app: Shared, ui: &AppWindow, dir: PathBuf, image: Option<PathBuf>) {
    let _ = std::fs::create_dir_all(&dir);
    let mut steps: Vec<Step> = Vec::new();

    steps.push(Box::new(|_, ui, r| {
        let n = slint::Model::row_count(&ui.get_cards());
        r.check(
            "starts on the library",
            ui.get_page() == 0,
            format!("{n} cards"),
        );
        // Own title bar: Windows has no system frame; macOS traffic lights sit mid-bar (26 pt).
        if cfg!(windows) {
            r.check(
                "own title bar (no system frame)",
                ui.get_custom_frame(),
                String::new(),
            );
        }
        if let Some(round) = crate::frame::corners_rounded(ui) {
            r.check("rounded corners (Windows 11)", round, String::new());
        }
        if let Some((ok, detail)) = crate::frame::titlebar_state(ui) {
            r.check("dark transparent title bar (macOS)", ok, detail);
        }
        if let Some(c) = crate::frame::lights_centre(ui) {
            r.check(
                "traffic lights centred on the bar",
                (c - 26.0).abs() <= 1.5,
                format!("centre {c:.1} pt"),
            );
        }
        r.snapshot(ui, "00-library");
    }));
    steps.push(Box::new(move |app, ui, r| match image {
        Some(p) => app.borrow_mut().open_path(ui, &p),
        None => r.check(
            "image argument",
            false,
            "usage: ZNIMOK_SELFTEST=<dir> znimok-app <image>".into(),
        ),
    }));

    steps.push(Box::new(|app, ui, r| {
        let open = app.borrow().s.is_some();
        r.check("document open", open, format!("page {}", ui.get_page()));
        r.snapshot(ui, "01-editor");
    }));
    // View: a picture smaller than the canvas is centred; zoom buttons animate; sideways scroll.
    steps.push(Box::new(|app, ui, r| {
        let (sc, dx, dy, _) = app.borrow().view_probe();
        r.check(
            "fit: picture centred",
            sc <= 1.0 && dx.abs() <= 1.0 && dy.abs() <= 1.0,
            format!("scale {sc:.3}, off-centre {dx:.1}, {dy:.1}"),
        );
        ui.invoke_zoom_100();
        let (s_now, ..) = app.borrow().view_probe();
        r.check(
            "100 % animates (not instant)",
            (s_now - sc).abs() < 0.05,
            format!("scale right after the click {s_now:.3}"),
        );
    }));
    steps.push(Box::new(|app, ui, r| {
        let (sc, ..) = app.borrow().view_probe();
        r.check(
            "100 % reached",
            (sc - 1.0).abs() < 1e-6,
            format!("scale {sc:.4}"),
        );
        // Zoom in with trackpad-sized steps (immediate) until the picture is wider than the
        // canvas (on a Retina canvas 100 % still fits), then scroll sideways.
        for _ in 0..12 {
            ui.invoke_wheel(300.0, 300.0, 0.0, 39.0, true, false, false);
        }
        let (_, _, _, ox) = app.borrow().view_probe();
        ui.invoke_wheel(300.0, 300.0, -120.0, 0.0, false, false, false);
        let (_, _, _, ox2) = app.borrow().view_probe();
        r.check(
            "sideways scroll moves the canvas",
            ox2 > ox + 10.0,
            format!("origin x {ox:.0} → {ox2:.0}"),
        );
        ui.invoke_zoom_fit();
    }));
    steps.push(Box::new(|app, _ui, r| {
        let (sc, dx, dy, _) = app.borrow().view_probe();
        r.check(
            "fit again: centred",
            sc <= 1.0 && dx.abs() <= 1.0 && dy.abs() <= 1.0,
            format!("scale {sc:.3}, off-centre {dx:.1}, {dy:.1}"),
        );
    }));
    // ZK-127: a fitted picture follows the window — smaller window, smaller picture; back to
    // the old size, back to the old scale (never past 100 %).
    let before = Rc::new(std::cell::Cell::new((0.0f64, 0.0f32, 0.0f32)));
    let b = before.clone();
    steps.push(Box::new(move |app, ui, _r| {
        let sf = ui.window().scale_factor();
        let size = ui.window().size();
        let (w, h) = (size.width as f32 / sf, size.height as f32 / sf);
        b.set((app.borrow().view_probe().0, w, h));
        ui.window()
            .set_size(slint::LogicalSize::new(w * 0.6, h * 0.6));
    }));
    let b = before.clone();
    steps.push(Box::new(move |app, ui, r| {
        let (sc0, w, h) = b.get();
        let (sc, ..) = app.borrow().view_probe();
        let fit = app.borrow().fit_probe();
        r.check(
            "smaller window: fitted picture shrinks with it",
            (sc - fit).abs() < 1e-3 && sc <= sc0 + 1e-6,
            format!("scale {sc0:.3} → {sc:.3} (fit {fit:.3})"),
        );
        ui.window().set_size(slint::LogicalSize::new(w, h));
    }));
    let b = before;
    steps.push(Box::new(move |app, _ui, r| {
        let (sc0, ..) = b.get();
        let (sc, ..) = app.borrow().view_probe();
        r.check(
            "window back: fitted picture grows back (≤ 100 %)",
            (sc - sc0).abs() < 1e-3 && sc <= 1.0,
            format!("scale {sc:.3}, was {sc0:.3}"),
        );
    }));
    steps.push(Box::new(|app, ui, r| {
        let (cx, cy) = centre(ui);
        app.borrow_mut().set_tool(ui, crate::app::tool::RECT);
        drag(app, ui, (cx - 240.0, cy - 150.0), (cx - 30.0, cy + 10.0));
        r.check(
            "rectangle drawn",
            count(app) == 1,
            format!("{} marks", count(app)),
        );
        // A click without movement makes no mark (no empty object, no undo step).
        click(app, ui, (cx + 250.0, cy - 160.0));
        r.check(
            "click draws nothing",
            count(app) == 1,
            format!("{} marks", count(app)),
        );
        same_as_full(app, ui, r, "drawn");
    }));
    steps.push(Box::new(|app, ui, r| {
        let (cx, cy) = centre(ui);
        key(app, ui, "l", false, false);
        drag(app, ui, (cx + 220.0, cy + 150.0), (cx + 10.0, cy + 20.0));
        // Ukrainian layout: "т" is the N key → counter.
        key(app, ui, "т", false, false);
        click(app, ui, (cx + 80.0, cy - 110.0));
        click(app, ui, (cx + 150.0, cy - 110.0));
        key(app, ui, "p", false, false);
        {
            let mut a = app.borrow_mut();
            a.pointer(ui, 0, cx - 250.0, cy + 120.0, 0, false, false);
            for i in 1..=40 {
                let t = i as f32 / 40.0;
                a.pointer(
                    ui,
                    1,
                    cx - 250.0 + 200.0 * t,
                    cy + 120.0 + 40.0 * (t * std::f32::consts::TAU).sin(),
                    0,
                    false,
                    false,
                );
            }
            a.pointer(ui, 2, cx - 50.0, cy + 120.0, 0, false, false);
        }
        key(app, ui, "b", false, false);
        drag(app, ui, (cx - 240.0, cy - 60.0), (cx - 120.0, cy - 20.0));
        r.check(
            "arrow, 2 counters, pen, hide",
            count(app) == 6,
            format!("{} marks", count(app)),
        );
        same_as_full(app, ui, r, "every kind");
        let seqs: Vec<u32> = app
            .borrow()
            .s
            .as_ref()
            .unwrap()
            .ed
            .doc
            .objects
            .iter()
            .filter_map(|o| match o.data {
                znimok_core::Data::Counter { seq, .. } => Some(seq),
                _ => None,
            })
            .collect();
        r.check(
            "counters numbered",
            seqs.len() == 2 && seqs[0] != seqs[1],
            format!("{seqs:?}"),
        );
    }));
    steps.push(Box::new(|app, ui, r| {
        let (cx, cy) = centre(ui);
        key(app, ui, "t", false, false);
        click(app, ui, (cx + 40.0, cy + 90.0));
        r.check("text editor opened", ui.get_editing(), String::new());
        // Typing goes into the document at once (ZK-49): the mark exists before Enter.
        ui.set_edit_text("Привіт".into());
        app.borrow_mut().text_edited(ui, 12, 12);
        let live = count(app);
        ui.set_edit_text("Привіт, Znimok".into());
        let n = "Привіт, Znimok".len() as i32;
        app.borrow_mut().text_edited(ui, n, n);
        r.check(
            "typing shows on the canvas at once",
            live == 7,
            format!("{live} marks while typing"),
        );
        same_as_full(app, ui, r, "typing");
        app.borrow_mut().commit_text(ui, "Привіт, Znimok");
        let w = app
            .borrow()
            .s
            .as_ref()
            .and_then(|s| s.ed.doc.objects.last().map(|o| o.rect.w))
            .unwrap_or(0);
        r.check(
            "text added and measured",
            count(app) == 7 && w > 40,
            format!("{} marks, text width {w}", count(app)),
        );
    }));
    steps.push(Box::new(|_, ui, r| r.snapshot(ui, "02-marks")));
    // ZK-49: a second text — caret on the canvas, Esc takes the typing back, one undo step,
    // alignment and block width from the inspector.
    steps.push(Box::new(|app, ui, _| {
        let (cx, cy) = centre(ui);
        key(app, ui, "t", false, false);
        click(app, ui, (cx - 200.0, cy + 150.0));
        ui.set_edit_text("Два\nрядки".into());
        let n = "Два\nрядки".len() as i32;
        app.borrow_mut().text_edited(ui, n, n);
        ui.invoke_edit_select(0, 3 * 2);
        app.borrow_mut().text_cursor(ui, 6, 0);
    }));
    steps.push(Box::new(|app, ui, r| {
        r.snapshot(ui, "23-text-caret");
        let before = count(app);
        app.borrow_mut().cancel_text(ui);
        let after = count(app);
        r.check(
            "Esc takes the new text back",
            before == 8 && after == 7 && !ui.get_editing(),
            format!("{before} → {after}"),
        );
        // Edit the first text: type, Enter; one undo step brings the old text back.
        let text_id = app.borrow().s.as_ref().and_then(|s| {
            s.ed.doc
                .objects
                .iter()
                .find(|o| o.kind() == znimok_core::Kind::Text)
                .map(|o| (o.id, o.rect.center()))
        });
        if let Some((id, (x, y))) = text_id {
            let (sx, sy) = {
                let a = app.borrow();
                let _ = &a;
                (x, y)
            };
            let _ = (sx, sy);
            app.borrow_mut().layer_click(ui, id as i32, false);
            ui.invoke_tool_chosen(0);
            // Double click with Select opens it for typing.
            let out = {
                let a = app.borrow();
                a.doc_to_logical(x, y)
            };
            app.borrow_mut().canvas_double(ui, out.0, out.1);
            let opened = ui.get_editing();
            ui.set_edit_text("Інший".into());
            app.borrow_mut().text_edited(ui, 10, 10);
            ui.set_edit_text("Інший текст".into());
            app.borrow_mut().text_edited(ui, 21, 21);
            app.borrow_mut().commit_text(ui, "Інший текст");
            let now = text_of(app, id);
            key(app, ui, "z", true, false);
            let back = text_of(app, id);
            r.check(
                "double click edits a text; typing is one undo step",
                opened
                    && now.as_deref() == Some("Інший текст")
                    && back.as_deref() == Some("Привіт, Znimok"),
                format!("{opened} {now:?} → {back:?}"),
            );
            // Alignment and block width.
            app.borrow_mut().layer_click(ui, id as i32, false);
            ui.invoke_set_prop("align".into(), 1);
            ui.invoke_set_text_box("60".into());
            let d = app
                .borrow()
                .s
                .as_ref()
                .and_then(|s| s.ed.doc.get(id).map(|o| o.data.clone()));
            let ok = matches!(
                d,
                Some(znimok_core::Data::Text {
                    align: znimok_core::Align::Center,
                    box_w: 60,
                    ..
                })
            );
            r.check("text: centre alignment, block 60 px", ok, format!("{d:?}"));
            key(app, ui, "z", true, false);
            key(app, ui, "z", true, false);
        }
    }));
    steps.push(Box::new(|app, ui, r| {
        key(app, ui, "a", true, false);
        app.borrow_mut().set_color(ui, 4);
        let blue = app
            .borrow()
            .s
            .as_ref()
            .map(|s| {
                s.ed.doc
                    .objects
                    .iter()
                    .filter(|o| o.style.color == crate::app::PALETTE[4])
                    .count()
            })
            .unwrap_or(0);
        // Colour applies to every kind but Hide (a hide mark has no stroke).
        r.check(
            "select all + colour (all but Hide)",
            blue == 6,
            format!("{blue} blue"),
        );
    }));
    steps.push(Box::new(|_, ui, r| r.snapshot(ui, "03-selected-blue")));
    steps.push(Box::new(|app, ui, r| {
        key(app, ui, "z", true, false); // colour
        key(app, ui, "я", true, false); // text (Ukrainian layout)
        r.check("undo ×2", count(app) == 6, format!("{} marks", count(app)));
        key(app, ui, "z", true, true);
        r.check("redo", count(app) == 7, format!("{} marks", count(app)));
        key(app, ui, "\u{1b}", false, false);
        let can = ui.get_can_undo();
        r.check("undo available", can, String::new());
    }));
    // ZK-52: rubber band, duplicate, align, z-order, group.
    steps.push(Box::new(|app, ui, r| {
        let (cx, cy) = centre(ui);
        key(app, ui, "\u{1b}", false, false);
        app.borrow_mut().set_tool(ui, crate::app::tool::SELECT);
        // A band over the two counters only.
        drag(app, ui, (cx + 40.0, cy - 150.0), (cx + 200.0, cy - 70.0));
        let n = ui.get_selection_count();
        r.check(
            "rubber band selects the two counters",
            n == 2,
            format!("{n} selected"),
        );
        let before = count(app);
        key(app, ui, "d", true, false);
        let (after, sel) = (count(app), ui.get_selection_count());
        r.check(
            "Ctrl+D duplicates the selection as the new selection",
            after == before + 2 && sel == 2,
            format!("{before} → {after} marks, {sel} selected"),
        );
        key(app, ui, "z", true, false);
        r.check(
            "duplicate is one undo step",
            count(app) == before,
            format!("{} marks", count(app)),
        );
        // Align the (re-selected) counters to the top edge, then bring them to the front.
        drag(app, ui, (cx + 40.0, cy - 150.0), (cx + 200.0, cy - 70.0));
        let tops = |app: &Shared| -> Vec<i32> {
            let a = app.borrow();
            let s = a.s.as_ref().unwrap();
            s.ed.selection()
                .iter()
                .filter_map(|id| s.ed.doc.get(*id).map(|o| o.bounds().y))
                .collect()
        };
        ui.invoke_align(3);
        let t = tops(app);
        r.check("align top", t.len() == 2 && t[0] == t[1], format!("{t:?}"));
        key(app, ui, "]", true, true);
        let last_selected = {
            let a = app.borrow();
            let s = a.s.as_ref().unwrap();
            let sel = s.ed.selection().to_vec();
            s.ed.doc
                .objects
                .iter()
                .rev()
                .take(2)
                .all(|o| sel.contains(&o.id))
        };
        r.check("Ctrl+Shift+] brings to front", last_selected, String::new());
        key(app, ui, "g", true, false);
        let grouped = {
            let a = app.borrow();
            let s = a.s.as_ref().unwrap();
            let sel = s.ed.selection().to_vec();
            sel.len() == 2
                && sel
                    .iter()
                    .filter_map(|id| s.ed.doc.get(*id))
                    .all(|o| o.group != 0)
        };
        r.check("Ctrl+G groups", grouped, String::new());
        // ZK-159: on the canvas a group is one — a click on a member selects all of it, a drag
        // moves all of it (owner, 29.09: members were still picked and moved one by one).
        let members: Vec<(u32, i32, i32)> = {
            let a = app.borrow();
            let s = a.s.as_ref().unwrap();
            s.ed.selection()
                .iter()
                .filter_map(|id| s.ed.doc.get(*id))
                .map(|o| (o.id, o.rect.x, o.rect.y))
                .collect()
        };
        if let Some(&(id0, _, _)) = members.first() {
            let at = {
                let a = app.borrow();
                let o = a.s.as_ref().unwrap().ed.doc.get(id0).unwrap();
                let b = o.bounds();
                a.doc_to_logical(b.x as f64 + b.w as f64 / 2.0, b.y as f64 + b.h as f64 / 2.0)
            };
            key(app, ui, "\u{1b}", false, false);
            click(app, ui, at);
            let n = ui.get_selection_count();
            r.check(
                "a click on a group member selects the whole group",
                n as usize == members.len(),
                format!("{n} of {} selected", members.len()),
            );
            drag(app, ui, at, (at.0 + 30.0, at.1 + 20.0));
            let moved = {
                let a = app.borrow();
                let s = a.s.as_ref().unwrap();
                members.iter().all(|(id, x, y)| {
                    s.ed.doc
                        .get(*id)
                        .is_some_and(|o| o.rect.x != *x && o.rect.y != *y)
                })
            };
            r.check(
                "dragging a member moves the whole group",
                moved,
                String::new(),
            );
            key(app, ui, "z", true, false);
            // Shift+click on a member takes the whole group out of the selection.
            {
                let mut a = app.borrow_mut();
                a.pointer(ui, 0, at.0, at.1, 0, true, false);
                a.pointer(ui, 2, at.0, at.1, 0, true, false);
            }
            let n2 = ui.get_selection_count();
            r.check(
                "Shift+click on a member deselects the whole group",
                n2 == 0,
                format!("{n2} selected"),
            );
            click(app, ui, at);
        }
        key(app, ui, "g", true, true);
        let ungrouped = {
            let a = app.borrow();
            let s = a.s.as_ref().unwrap();
            s.ed.selection()
                .iter()
                .filter_map(|id| s.ed.doc.get(*id))
                .all(|o| o.group == 0)
        };
        r.check("Ctrl+Shift+G ungroups", ungrouped, String::new());
        key(app, ui, "\u{1b}", false, false);
        // Ctrl held = temporary Select (owner, 28.09): with the Rectangle tool active, Ctrl+click
        // on the first rectangle selects it, Ctrl+click again deselects it, nothing is drawn.
        app.borrow_mut().set_tool(ui, crate::app::tool::RECT);
        let n0 = count(app);
        {
            let mut a = app.borrow_mut();
            a.pointer(ui, 0, cx - 135.0, cy - 70.0, 0, false, true);
            a.pointer(ui, 2, cx - 135.0, cy - 70.0, 0, false, true);
        }
        let s1 = ui.get_selection_count();
        {
            let mut a = app.borrow_mut();
            a.pointer(ui, 0, cx - 135.0, cy - 70.0, 0, false, true);
            a.pointer(ui, 2, cx - 135.0, cy - 70.0, 0, false, true);
        }
        let s2 = ui.get_selection_count();
        // Select it again, then Ctrl+click on empty space: the selection goes.
        {
            let mut a = app.borrow_mut();
            a.pointer(ui, 0, cx - 135.0, cy - 70.0, 0, false, true);
            a.pointer(ui, 2, cx - 135.0, cy - 70.0, 0, false, true);
            a.pointer(ui, 0, cx + 330.0, cy + 250.0, 0, false, true);
            a.pointer(ui, 2, cx + 330.0, cy + 250.0, 0, false, true);
        }
        let s3 = ui.get_selection_count();
        r.check(
            "Ctrl+click on empty space clears the selection",
            s3 == 0,
            format!("{s3} selected"),
        );
        r.check(
            "Ctrl+click with a drawing tool toggles selection, draws nothing",
            s1 == 1 && s2 == 0 && count(app) == n0 && ui.get_tool() == 1,
            format!(
                "{s1} → {s2} selected, {} marks, tool {}",
                count(app),
                ui.get_tool()
            ),
        );
        // Ctrl+drag on a mark with the Rectangle tool moves it (owner, 29.09), draws nothing.
        let first = app.borrow().s.as_ref().and_then(|s| {
            s.ed.doc
                .objects
                .iter()
                .find(|o| o.kind() == znimok_core::Kind::Rect)
                .map(|o| (o.id, o.rect.x))
        });
        if let Some((id, x0)) = first {
            let n1 = count(app);
            {
                let mut a = app.borrow_mut();
                a.pointer(ui, 0, cx - 135.0, cy - 70.0, 0, false, true);
                for i in 1..=6 {
                    a.pointer(
                        ui,
                        1,
                        cx - 135.0 + 5.0 * i as f32,
                        cy - 70.0,
                        0,
                        false,
                        true,
                    );
                }
                a.pointer(ui, 2, cx - 105.0, cy - 70.0, 0, false, true);
            }
            let x1 = app
                .borrow()
                .s
                .as_ref()
                .and_then(|s| s.ed.doc.get(id).map(|o| o.rect.x))
                .unwrap_or(x0);
            r.check(
                "Ctrl+drag moves a mark with a drawing tool",
                x1 > x0 && count(app) == n1 && ui.get_tool() == 1,
                format!("x {x0} → {x1}, {} marks", count(app)),
            );
            same_as_full(app, ui, r, "dragged");
            key(app, ui, "z", true, false);
        }
    }));
    // ZK-54: inspector properties, layers, meta, zoom slider; snapshots of every tab.
    steps.push(Box::new(|app, ui, r| {
        let find = |app: &Shared, k: znimok_core::Kind| -> Option<u32> {
            let a = app.borrow();
            let s = a.s.as_ref()?;
            s.ed.doc
                .objects
                .iter()
                .find(|o| o.kind() == k)
                .map(|o| o.id)
        };
        let get = |app: &Shared, id: u32| -> Option<znimok_core::Object> {
            app.borrow().s.as_ref()?.ed.doc.get(id).cloned()
        };
        // Fill a rectangle with the 3rd colour.
        if let Some(id) = find(app, znimok_core::Kind::Rect) {
            app.borrow_mut().layer_click(ui, id as i32, false);
            r.check(
                "layer click selects",
                ui.get_prop_kind() == 0 && ui.get_prop_for_selection(),
                format!("kind {}", ui.get_prop_kind()),
            );
            ui.invoke_set_prop("fill".into(), 2);
            let c2 = get(app, id).and_then(|o| o.style.color2);
            r.check(
                "fill applies to the rectangle",
                c2 == Some(crate::app::PALETTE[2]),
                format!("{c2:?}"),
            );
            // Stroke ↔ fill in one click.
            let before = get(app, id).map(|o| (o.style.color, o.style.color2));
            ui.invoke_set_prop("swap".into(), 0);
            let after = get(app, id).map(|o| (o.style.color, o.style.color2));
            r.check(
                "swap stroke and fill",
                matches!((before, after), (Some((c, Some(f))), Some((c2, Some(f2)))) if c2 == f && f2 == c),
                format!("{before:?} → {after:?}"),
            );
            // No outline, then swap: the plate's fill becomes the outline, the fill goes.
            ui.invoke_set_prop("stroke-none".into(), 0);
            let plate = get(app, id).map(|o| o.style.no_main);
            ui.invoke_set_prop("swap".into(), 0);
            let st = get(app, id).map(|o| (o.style.color, o.style.color2, o.style.no_main));
            r.check(
                "swap with no outline: fill → outline, no fill",
                plate == Some(true) && st == Some((crate::app::PALETTE[0], None, false)),
                format!("plate {plate:?} → {st:?}"),
            );
            // ZK-161 (owner, 29.09: with the outline off the fill could not be changed). A plate
            // is one colour: no outline takes the fill's colour, the fill row then recolours the
            // plate, and no fill brings the outline back in that colour — never both gone.
            let p = crate::app::PALETTE;
            let style = |app: &Shared| get(app, id).map(|o| (o.style.color, o.style.color2, o.style.no_main));
            ui.invoke_set_prop("fill".into(), 3);
            ui.invoke_set_prop("stroke-none".into(), 0);
            let a = style(app);
            let shown_a = (ui.get_color_index(), ui.get_fill_index());
            ui.invoke_set_prop("fill".into(), 5);
            let b = style(app);
            let shown_b = ui.get_fill_index();
            ui.invoke_set_prop("fill".into(), -1);
            let c = style(app);
            r.check(
                "no outline: the fill colour changes the plate; no fill brings the outline back",
                a == Some((p[3], None, true))
                    && shown_a == (-1, 3)
                    && b == Some((p[5], None, true))
                    && shown_b == 5
                    && c == Some((p[5], None, false)),
                format!("{a:?} shown {shown_a:?} → {b:?} shown {shown_b} → {c:?}"),
            );
            ui.invoke_set_prop("color".into(), 0);
            ui.invoke_set_prop("fill".into(), 4);
            r.snapshot(ui, "14-props-rect");
            // ZK-160: the colour picker — any colour, a hex code, the eyedropper, recent colours.
            {
                use slint::platform::{PointerEventButton, WindowEvent};
                let g = ui.global::<crate::ColourPick>();
                let own = znimok_core::Rgb::new(0x12, 0x34, 0x56);
                g.invoke_pick("fill".into(), slint::Color::from_rgb_u8(own.r, own.g, own.b));
                let fill = get(app, id).and_then(|o| o.style.color2);
                let shown = (ui.get_fill_index(), ui.get_fill_rgb());
                g.invoke_hex_entered("color".into(), " #abc ".into());
                let stroke = get(app, id).map(|o| o.style.color);
                g.invoke_hex_entered("color".into(), "#12zz45".into());
                let kept = get(app, id).map(|o| o.style.color);
                let recent = app.borrow().recent_colours();
                r.check(
                    "colour picker: any fill, a hex code, bad hex ignored, recent colours kept",
                    fill == Some(own)
                        && shown == (-2, slint::Color::from_rgb_u8(0x12, 0x34, 0x56))
                        && stroke == Some(znimok_core::Rgb::new(0xAA, 0xBB, 0xCC))
                        && kept == stroke
                        && recent.first() == Some(&znimok_core::Rgb::new(0xAA, 0xBB, 0xCC))
                        && recent.get(1) == Some(&own)
                        && g.get_recent().row_count() == recent.len(),
                    format!("fill {fill:?} shown {shown:?} · stroke {stroke:?} · kept {kept:?} · recent {recent:?}"),
                );
                // The eyedropper: the next canvas click takes the colour there, nothing else.
                let n = count(app);
                let (ex, ey) = (40.0, 40.0);
                let want = app.borrow().colour_probe(ex, ey);
                g.invoke_eyedrop("fill".into());
                let cursor = ui.get_canvas_cursor();
                let at = app.borrow().doc_to_logical(ex, ey);
                click(app, ui, at);
                let took = get(app, id).and_then(|o| o.style.color2);
                r.check(
                    "eyedropper: a click on the picture takes its colour, draws nothing",
                    want.is_some() && took == want && count(app) == n && cursor == 1
                        && ui.get_selection_count() == 1,
                    format!("want {want:?} took {took:?} · {} marks · cursor {cursor}", count(app)),
                );
                // The popover itself, opened by a real click on the fill row's picker chip.
                let win = ui.window();
                let w = win.size().width as f32 / win.scale_factor();
                let chip = slint::LogicalPosition::new(w - 37.0, 183.0);
                win.dispatch_event(WindowEvent::PointerMoved { position: chip });
                win.dispatch_event(WindowEvent::PointerPressed {
                    position: chip,
                    button: PointerEventButton::Left,
                });
                win.dispatch_event(WindowEvent::PointerReleased {
                    position: chip,
                    button: PointerEventButton::Left,
                });
                r.snapshot(ui, "14b-colour-picker");
                let away = slint::LogicalPosition::new(w - 200.0, 820.0);
                win.dispatch_event(WindowEvent::PointerPressed {
                    position: away,
                    button: PointerEventButton::Left,
                });
                win.dispatch_event(WindowEvent::PointerReleased {
                    position: away,
                    button: PointerEventButton::Left,
                });
                ui.invoke_set_prop("fill".into(), 4);
            }
            ui.invoke_set_prop("corners".into(), 2);
            let c = get(app, id).map(|o| o.style.corners);
            r.check(
                "corners: round",
                c == Some(znimok_core::Corners::Round),
                format!("{c:?}"),
            );
            ui.invoke_set_alpha(0.5, false);
            ui.invoke_set_alpha(0.4, true);
            let a = get(app, id).map(|o| o.style.alpha);
            r.check("opacity slider", a == Some(40), format!("{a:?}"));
            ui.invoke_set_geom("x".into(), "10".into());
            let x = get(app, id).map(|o| o.rect.x);
            r.check("X field moves the mark", x == Some(10), format!("{x:?}"));
        }
        // Type size: the ladder steps and any value typed in.
        if let Some(id) = find(app, znimok_core::Kind::Text) {
            app.borrow_mut().layer_click(ui, id as i32, false);
            let size = |app: &Shared| match get(app, id).map(|o| o.data) {
                Some(znimok_core::Data::Text { size, .. }) => size,
                _ => 0,
            };
            let s0 = size(app);
            ui.invoke_set_prop("text-step".into(), 1);
            let s1 = size(app);
            ui.invoke_set_text_size("30".into());
            let s2 = size(app);
            r.check(
                "type size: + steps up the ladder, a typed 30 is kept",
                s1 > s0 && s2 == 30 && ui.get_text_size_px() == "30",
                format!("{s0} → {s1} → {s2}"),
            );
        }
        // Arrowheads are line properties.
        if let Some(id) = find(app, znimok_core::Kind::Line) {
            app.borrow_mut().layer_click(ui, id as i32, false);
            ui.invoke_set_prop("head-start".into(), 3);
            let heads = get(app, id).map(|o| match o.data {
                znimok_core::Data::Line {
                    head_front,
                    head_back,
                    ..
                } => (head_back, head_front),
                _ => (znimok_core::Head::None, znimok_core::Head::None),
            });
            r.check(
                "line heads: start = dot, end kept",
                heads == Some((znimok_core::Head::Dot, znimok_core::Head::Triangle)),
                format!("{heads:?}"),
            );
            // Line ends as X2 / Y2; shadow from the inspector.
            ui.invoke_set_geom("x2".into(), "700".into());
            ui.invoke_set_prop("shadow".into(), 1);
            let st = get(app, id).map(|o| (o.rect.x + o.rect.w, o.style.shadow));
            r.check(
                "line: X2 moves the end, shadow light",
                st == Some((700, znimok_core::Effect::Light)) && ui.get_shadow_index() == 1,
                format!("{st:?}"),
            );
            r.snapshot(ui, "15-props-line");
            ui.invoke_layer_eye(id as i32);
            let hidden = get(app, id).map(|o| o.hidden);
            r.check(
                "eye hides the mark",
                hidden == Some(true),
                format!("{hidden:?}"),
            );
            ui.invoke_layer_eye(id as i32);
        }
        // Pen trails take heads too (ZK-48).
        if let Some(id) = find(app, znimok_core::Kind::Pen) {
            app.borrow_mut().layer_click(ui, id as i32, false);
            ui.invoke_set_prop("head-end".into(), 1);
            let heads = get(app, id).map(|o| match o.data {
                znimok_core::Data::Pen { head_front, .. } => head_front,
                _ => znimok_core::Head::None,
            });
            r.check(
                "pen: end head = triangle",
                heads == Some(znimok_core::Head::Triangle) && ui.get_head_end() == 1,
                format!("{heads:?}"),
            );
            r.snapshot(ui, "20-props-pen");
        }
        let rows = slint::Model::row_count(&ui.get_layers());
        r.check(
            "layers list = marks",
            rows == count(app),
            format!("{rows} rows"),
        );
        ui.set_insp_tab(1);
    }));
    // ZK-51: counters (shape, digit colour, a new numbering group), stamps and emoji, a dropped
    // picture becomes a mark.
    steps.push(Box::new(|app, ui, r| {
        use znimok_core::{CounterShape, Data, Kind};
        let first = app.borrow().s.as_ref().and_then(|s| {
            s.ed.doc
                .objects
                .iter()
                .find(|o| o.kind() == Kind::Counter)
                .map(|o| o.id)
        });
        if let Some(id) = first {
            ui.set_insp_tab(0);
            ui.invoke_tool_chosen(0);
            app.borrow_mut().layer_click(ui, id as i32, false);
            ui.invoke_set_prop("counter-shape".into(), 2);
            ui.invoke_set_prop("digit".into(), 2);
            let o = app
                .borrow()
                .s
                .as_ref()
                .and_then(|s| s.ed.doc.get(id).cloned());
            let ok = o.as_ref().is_some_and(|o| {
                matches!(
                    o.data,
                    Data::Counter {
                        shape: CounterShape::Pin,
                        ..
                    }
                ) && o.style.color2 == Some(crate::app::PALETTE[2])
            });
            r.check(
                "counter: pin, yellow digit",
                ok,
                format!("{:?}", o.map(|o| (o.data, o.style.color2))),
            );
        }
    }));
    steps.push(Box::new(|app, ui, r| {
        use znimok_core::{Data, Kind};
        r.snapshot(ui, "25-counter");
        // A new group: the next counter is number 1 of group 2.
        ui.invoke_set_prop("counter-group-new".into(), 0);
        let (cx, cy) = centre(ui);
        click(app, ui, (cx - 300.0, cy + 250.0));
        let last = app.borrow().s.as_ref().and_then(|s| {
            let doc = &s.ed.doc;
            let i = doc
                .objects
                .iter()
                .rposition(|o| o.kind() == Kind::Counter)?;
            Some((doc.objects[i].data.clone(), doc.counter_number(i)))
        });
        let ok = matches!(&last, Some((Data::Counter { group, .. }, Some(1))) if *group >= 2);
        r.check("new numbering group starts at 1", ok, format!("{last:?}"));
        key(app, ui, "z", true, false);
        // A stamp with an emoji.
        ui.invoke_set_prop("stamp".into(), 105);
        click(app, ui, (cx - 250.0, cy + 250.0));
        let stamp = app.borrow().s.as_ref().and_then(|s| {
            s.ed.doc
                .objects
                .iter()
                .rev()
                .find(|o| o.kind() == Kind::Stamp)
                .map(|o| o.data.clone())
        });
        r.check(
            "stamp picker: an emoji stamp",
            matches!(stamp, Some(Data::Stamp { id: 105 })) && ui.get_tool() == 9,
            format!("{stamp:?}"),
        );

        // ZK-173: fixed stamp sizes — XL is twice M, square, and the row shows it.
        let side = |app: &Shared| {
            app.borrow().s.as_ref().and_then(|s| {
                s.ed.doc
                    .objects
                    .iter()
                    .rev()
                    .find(|o| o.kind() == Kind::Stamp)
                    .map(|o| (o.rect.w, o.rect.h))
            })
        };
        let m = side(app);
        ui.invoke_set_prop("stamp-size".into(), 3);
        click(app, ui, (cx - 150.0, cy + 250.0));
        let xl = side(app);
        let shown = ui.get_stamp_size();
        // The XL one goes (before the size is reset: with it selected, the reset resizes it);
        // the emoji stamp stays for the snapshot.
        key(app, ui, "z", true, false);
        ui.invoke_set_prop("stamp-size".into(), 1);
        r.check(
            "stamp sizes: XL is twice M, square",
            matches!((m, xl), (Some((mw, mh)), Some((xw, xh)))
                if mw == mh && xw == xh && (xw as f64 / mw as f64 - 2.0).abs() < 0.1)
                && shown == 3,
            format!("M {m:?} · XL {xl:?} · shown {shown}"),
        );
    }));
    steps.push(Box::new(|app, ui, r| {
        r.snapshot(ui, "26-stamps");
        key(app, ui, "z", true, false);
        // A picture dropped on the open document: an image mark, not a new document.
        let n = count(app);
        let sample = std::env::args_os().nth(1).map(std::path::PathBuf::from);
        let dropped = sample.is_some_and(|p| app.borrow_mut().drop_image_mark(ui, &p));
        r.check(
            "a dropped picture becomes a mark",
            dropped && count(app) == n + 1 && ui.get_page() == 1,
            format!("{n} → {} marks", count(app)),
        );
        key(app, ui, "z", true, false);
        // ZK-163: the Image button / I — a picture file becomes a selected mark; a file that is
        // not a picture says so and adds nothing. I asks for the file (the dialog is main's).
        let asks = app.borrow_mut().key(ui, "i", false, false);
        let asks_uk = app.borrow_mut().key(ui, "ш", false, false);
        let sample = std::env::args_os().nth(1).map(std::path::PathBuf::from);
        let added = sample.is_some_and(|p| app.borrow_mut().insert_image_file(ui, &p));
        let kind = {
            let a = app.borrow();
            let s = a.s.as_ref().unwrap();
            let sel = s.ed.selection().to_vec();
            (sel.len() == 1)
                .then(|| s.ed.doc.get(sel[0]).map(|o| o.kind()))
                .flatten()
        };
        let not_image = r.dir.join("not-a-picture.txt");
        let _ = std::fs::write(&not_image, "text");
        let n2 = count(app);
        let refused = !app.borrow_mut().insert_image_file(ui, &not_image);
        r.check(
            "Image button / I: a picture file becomes a selected mark, other files are refused",
            matches!(asks, crate::app::KeyAction::InsertImage)
                && matches!(asks_uk, crate::app::KeyAction::InsertImage)
                && added
                && kind == Some(znimok_core::Kind::Image)
                && refused
                && count(app) == n2
                && !ui.get_toast().is_empty(),
            format!(
                "added {added} kind {kind:?} refused {refused} toast «{}»",
                ui.get_toast()
            ),
        );
        key(app, ui, "z", true, false);
        ui.invoke_tool_chosen(0);
    }));
    // ZK-171: the marker is a bar of fixed thickness along the drag, turned with it; four fixed
    // thicknesses; its own inks, apart from the other tools' colour.
    steps.push(Box::new(|app, ui, r| {
        use znimok_core::Kind;
        let (cx, cy) = centre(ui);
        let other = ui.get_color_index();
        ui.invoke_tool_chosen(7);
        let ink = (ui.get_color_index(), ui.get_color_rgb());
        drag(app, ui, (cx - 150.0, cy - 150.0), (cx + 50.0, cy + 50.0));
        let bar = |app: &Shared| {
            app.borrow().s.as_ref().and_then(|s| {
                s.ed.doc
                    .objects
                    .iter()
                    .rev()
                    .find(|o| o.kind() == Kind::Mark)
                    .map(|o| (o.rect, o.rot, o.style.color))
            })
        };
        let made = bar(app);
        ui.invoke_set_prop("marker-size".into(), 3);
        let thick = bar(app);
        ui.invoke_set_prop("color".into(), 2);
        let magenta = bar(app).map(|b| b.2);
        let shown = ui.get_color_index();
        r.check(
            "marker: a turned bar along the drag, fixed thickness, sizes, its own inks",
            matches!(made, Some((rc, 45, c)) if rc.h == 18 && rc.w > 300 && c == znimok_core::Rgb::new(255, 255, 0))
                && matches!(thick, Some((rc, 45, _)) if rc.h == 38)
                && magenta == Some(znimok_core::Rgb::new(255, 0, 255))
                && ink.0 == 0
                && shown == 2,
            format!("made {made:?} · XL {thick:?} · ink {:?} → {magenta:?} (chip {shown}) · other tools' chip was {other}", ink.0),
        );
        r.snapshot(ui, "26b-marker");
        for _ in 0..3 {
            key(app, ui, "z", true, false);
        }
        ui.invoke_tool_chosen(0);
    }));
    // ZK-164: the rotation handle — one mark turns about its centre (Shift: 45° steps), several
    // turn about the middle of their box; one undo step; the angle field.
    steps.push(Box::new(|app, ui, r| {
        use znimok_core::Kind;
        let ids = |k: Kind| -> Vec<u32> {
            app.borrow()
                .s
                .as_ref()
                .map(|s| {
                    s.ed.doc
                        .objects
                        .iter()
                        .filter(|o| o.kind() == k)
                        .map(|o| o.id)
                        .collect()
                })
                .unwrap_or_default()
        };
        let obj = |id: u32| app.borrow().s.as_ref().and_then(|s| s.ed.doc.get(id).cloned());
        // Drags the grip a quarter turn clockwise about its centre (in steps, as a hand would).
        let quarter = |shift: bool| -> bool {
            let Some((g, c)) = app.borrow().rotation_grip_probe() else {
                return false;
            };
            let (dx, dy) = (g.0 - c.0, g.1 - c.1);
            let mut a = app.borrow_mut();
            a.pointer(ui, 0, g.0, g.1, 0, shift, false);
            for i in 1..=9 {
                let t = std::f32::consts::FRAC_PI_2 * i as f32 / 9.0;
                let (x, y) = (
                    c.0 + dx * t.cos() - dy * t.sin(),
                    c.1 + dx * t.sin() + dy * t.cos(),
                );
                a.pointer(ui, 1, x, y, 0, shift, false);
            }
            a.pointer(ui, 2, c.0 - dy, c.1 + dx, 0, shift, false);
            true
        };
        let Some(&rect) = ids(Kind::Rect).first() else {
            r.check("rotation: a rectangle to turn", false, String::new());
            return;
        };
        ui.invoke_tool_chosen(0);
        app.borrow_mut().layer_click(ui, rect as i32, false);
        let grip = app.borrow().rotation_grip_probe().map(|p| p.0);
        let cursor = grip.map(|g| {
            app.borrow_mut().pointer(ui, 1, g.0, g.1, 0, false, false);
            ui.get_canvas_cursor()
        });
        let c0 = obj(rect).map(|o| o.bounds().center());
        let turned = quarter(false);
        let a1 = obj(rect).map(|o| o.rot);
        let c1 = obj(rect).map(|o| o.bounds().center());
        r.snapshot(ui, "36-rotated");
        key(app, ui, "z", true, false);
        let back = obj(rect).map(|o| o.rot);
        ui.invoke_set_geom("rot".into(), "33".into());
        let typed = obj(rect).map(|o| o.rot);
        quarter(true);
        let snapped = obj(rect).map(|o| o.rot);
        r.check(
            "rotation handle: a quarter turn about the centre, one undo step, typed angle, Shift snaps to 45°",
            turned
                && cursor == Some(10)
                && a1.is_some_and(|a| (88..=92).contains(&a))
                && c0 == c1
                && back == Some(0)
                && typed == Some(33)
                && snapped == Some(135),
            format!(
                "cursor {cursor:?} · {a1:?} centre {c0:?}→{c1:?} · undo {back:?} · typed {typed:?} · Shift {snapped:?}"
            ),
        );
        key(app, ui, "z", true, false);
        key(app, ui, "z", true, false);
        // Several marks turn about the middle of their box.
        let counters = ids(Kind::Counter);
        if counters.len() >= 2 {
            app.borrow_mut().layer_click(ui, counters[0] as i32, false);
            app.borrow_mut().layer_click(ui, counters[1] as i32, true);
            let centres = |ids: &[u32]| -> Vec<(f64, f64)> {
                ids.iter()
                    .filter_map(|id| obj(*id).map(|o| o.bounds().center()))
                    .collect()
            };
            let before = centres(&counters[..2]);
            quarter(false);
            let after = centres(&counters[..2]);
            let rots: Vec<u16> = counters[..2]
                .iter()
                .filter_map(|id| obj(*id).map(|o| o.rot))
                .collect();
            // The segment between them turns a quarter too: (x, y) becomes (-y, x).
            let ok = before.len() == 2 && after.len() == 2 && {
                let (bx, by) = (before[1].0 - before[0].0, before[1].1 - before[0].1);
                let (ax, ay) = (after[1].0 - after[0].0, after[1].1 - after[0].1);
                (ax + by).abs() <= 3.0 && (ay - bx).abs() <= 3.0
            };
            r.snapshot(ui, "36b-rotated-many");
            r.check(
                "rotation handle: several marks turn about the middle of their box",
                ok && rots.iter().all(|a| (88..=92).contains(a)),
                format!("{before:?} → {after:?} · {rots:?}"),
            );
            key(app, ui, "z", true, false);
        }
        key(app, ui, "\u{1b}", false, false);
    }));
    // ZK-168: another tool lets go of the selection; the inspector shows that tool.
    steps.push(Box::new(|app, ui, r| {
        let id = app.borrow().s.as_ref().and_then(|s| {
            s.ed.doc
                .objects
                .iter()
                .find(|o| o.kind() == znimok_core::Kind::Rect)
                .map(|o| o.id)
        });
        let Some(id) = id else { return };
        app.borrow_mut().layer_click(ui, id as i32, false);
        let had = ui.get_selection_count();
        ui.invoke_tool_chosen(2);
        let (n, kind, for_sel) = (
            ui.get_selection_count(),
            ui.get_prop_kind(),
            ui.get_prop_for_selection(),
        );
        r.check(
            "another tool drops the selection; the inspector shows the tool",
            had == 1 && n == 0 && kind == 1 && !for_sel,
            format!("selected {had} → {n} · inspector kind {kind} for selection {for_sel}"),
        );
        ui.invoke_tool_chosen(0);
    }));
    // ZK-167: handles — a counter scales as a whole by its corners and a pin is taller than a
    // circle whatever the order of choices; Shift keeps a box's ratio, Alt its centre.
    steps.push(Box::new(|app, ui, r| {
        use znimok_core::Kind;
        let first = |k: Kind| -> Option<u32> {
            app.borrow().s.as_ref().and_then(|s| {
                s.ed.doc
                    .objects
                    .iter()
                    .find(|o| o.kind() == k)
                    .map(|o| o.id)
            })
        };
        let obj = |id: u32| {
            app.borrow()
                .s
                .as_ref()
                .and_then(|s| s.ed.doc.get(id).cloned())
        };
        // Drags handle `h` of the selected mark by (dx, dy) screenshot pixels.
        let drag_handle = |id: u32, h: usize, dx: f64, dy: f64, shift: bool| {
            let Some(o) = obj(id) else { return };
            let (hx, hy) = znimok_core::hit::handles(&o)[h];
            let (a0, a1) = {
                let a = app.borrow();
                (a.doc_to_logical(hx, hy), a.doc_to_logical(hx + dx, hy + dy))
            };
            let mut a = app.borrow_mut();
            a.pointer(ui, 0, a0.0, a0.1, 0, shift, false);
            for i in 1..=6 {
                let t = i as f32 / 6.0;
                a.pointer(
                    ui,
                    1,
                    a0.0 + (a1.0 - a0.0) * t,
                    a0.1 + (a1.1 - a0.1) * t,
                    0,
                    shift,
                    false,
                );
            }
            a.pointer(ui, 2, a1.0, a1.1, 0, shift, false);
        };
        ui.invoke_tool_chosen(0);
        if let Some(c) = first(Kind::Counter) {
            app.borrow_mut().layer_click(ui, c as i32, false);
            // A circle first: a pin made one is square again.
            ui.invoke_set_prop("counter-shape".into(), 0);
            let before = obj(c).map(|o| (o.rect.w, o.rect.h));
            // The bottom-right corner (3 of the four), dragged wide and barely down.
            drag_handle(c, 2, 30.0, 4.0, false);
            let after = obj(c).map(|o| (o.rect.w, o.rect.h, o.style.thick));
            // The shape made a pin afterwards: the counter grows its point.
            ui.invoke_set_prop("counter-shape".into(), 2);
            let pin = obj(c).map(|o| (o.rect.w, o.rect.h));
            r.check(
                "counter: four corner handles, scales only as a whole; a pin is 1.3× as tall",
                obj(c).is_some_and(|o| znimok_core::hit::handles(&o).len() == 4)
                    && before.is_some_and(|(w, h)| w == h)
                    && after
                        .is_some_and(|(w, h, t)| w == h && w > before.map_or(0, |b| b.0) && t == w)
                    && pin.is_some_and(|(w, h)| h == (w * 13 + 5) / 10),
                format!("{before:?} → {after:?} → pin {pin:?}"),
            );
            key(app, ui, "z", true, false);
            key(app, ui, "z", true, false);
            key(app, ui, "z", true, false);
        }
        if let Some(rect) = first(Kind::Rect) {
            app.borrow_mut().layer_click(ui, rect as i32, false);
            let o0 = obj(rect).map(|o| o.rect).unwrap_or_default();
            drag_handle(rect, 4, 60.0, 0.0, true);
            let shifted = obj(rect).map(|o| o.rect).unwrap_or_default();
            key(app, ui, "z", true, false);
            app.borrow_mut().test_alt = true;
            drag_handle(rect, 3, 20.0, 0.0, false);
            app.borrow_mut().test_alt = false;
            let centred = obj(rect).map(|o| o.rect).unwrap_or_default();
            key(app, ui, "z", true, false);
            let ratio = |r: znimok_core::IRect| r.w as f64 / r.h.max(1) as f64;
            r.check(
                "resize: Shift keeps the ratio, Alt keeps the centre",
                (ratio(shifted) - ratio(o0)).abs() < 0.02
                    && shifted.w > o0.w
                    && shifted.x == o0.x
                    && centred.w == o0.w + 40
                    && centred.x == o0.x - 20
                    && centred.h == o0.h,
                format!("{o0:?} · Shift {shifted:?} · Alt {centred:?}"),
            );
        }
        key(app, ui, "\u{1b}", false, false);
    }));
    // ZK-169: counters of one numbering group are one group of marks; drawing selects only the
    // new one; Alt+click takes one member; the title names the group; edit / delete the group.
    steps.push(Box::new(|app, ui, r| {
        use znimok_core::Data;
        ui.invoke_set_prop("counter-group-new".into(), 0);
        let n0 = count(app);
        let spots = [(300.0, 800.0), (420.0, 800.0)];
        for (x, y) in spots {
            let at = app.borrow().doc_to_logical(x, y);
            click(app, ui, at);
        }
        let new: Vec<(u32, u32, u32)> = app
            .borrow()
            .s
            .as_ref()
            .map(|s| {
                s.ed.doc.objects[n0..]
                    .iter()
                    .filter_map(|o| match o.data {
                        Data::Counter { group, .. } => Some((o.id, o.group, group)),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let sel_after_draw: Vec<u32> = app
            .borrow()
            .s
            .as_ref()
            .map(|s| s.ed.selection().to_vec())
            .unwrap_or_default();
        let grouped = new.len() == 2 && new[0].1 != 0 && new[0].1 == new[1].1;
        let last_only = new.len() == 2 && sel_after_draw == vec![new[1].0];
        ui.invoke_tool_chosen(0);
        let first_at = app.borrow().doc_to_logical(spots[0].0, spots[0].1);
        click(app, ui, first_at);
        let whole = ui.get_selection_count();
        app.borrow_mut().test_alt = true;
        click(app, ui, first_at);
        app.borrow_mut().test_alt = false;
        let one = ui.get_selection_count();
        let title = ui.get_prop_title().to_string();
        ui.invoke_set_prop("counter-edit-group".into(), 0);
        let edit_all = ui.get_selection_count();
        let before_delete = count(app);
        ui.invoke_set_prop("counter-delete-group".into(), 0);
        let deleted = before_delete - count(app);
        key(app, ui, "z", true, false);
        let restored = count(app) == before_delete;
        r.check(
            "counter groups: one group of marks, drawing selects the new one, Alt+click one member, title, edit / delete the group",
            grouped
                && last_only
                && whole == 2
                && one == 1
                && title.contains(&new.first().map_or(0, |c| c.2).to_string())
                && edit_all == 2
                && deleted == 2
                && restored,
            format!(
                "{new:?} · selected after drawing {sel_after_draw:?} · click {whole}, Alt+click {one} · «{title}» · edit {edit_all} · deleted {deleted} · undo {restored}"
            ),
        );
        r.snapshot(ui, "38-counter-group");
        key(app, ui, "z", true, false);
        key(app, ui, "z", true, false);
        key(app, ui, "\u{1b}", false, false);
    }));
    // ZK-166: a counter's size buttons, the colour ⇄ number swap, the number upright when turned.
    steps.push(Box::new(|app, ui, r| {
        use znimok_core::Kind;
        let first = app.borrow().s.as_ref().and_then(|s| {
            s.ed.doc
                .objects
                .iter()
                .find(|o| o.kind() == Kind::Counter)
                .map(|o| o.id)
        });
        let Some(c) = first else {
            r.check("counter: one to test", false, String::new());
            return;
        };
        let obj = |id: u32| {
            app.borrow()
                .s
                .as_ref()
                .and_then(|s| s.ed.doc.get(id).cloned())
        };
        ui.invoke_tool_chosen(0);
        app.borrow_mut().layer_click(ui, c as i32, false);
        ui.invoke_set_prop("counter-shape".into(), 2);
        let centre0 = obj(c).map(|o| o.bounds().center());
        ui.invoke_set_prop("counter-size".into(), 3);
        let big = obj(c).map(|o| (o.rect.w, o.rect.h, o.bounds().center()));
        let shown = ui.get_counter_size();
        let before = obj(c).map(|o| (o.style.color, o.style.color2));
        ui.invoke_set_prop("swap-digit".into(), 0);
        let after = obj(c).map(|o| (o.style.color, o.style.color2));
        ui.invoke_set_geom("rot".into(), "120".into());
        r.snapshot(ui, "37-counter-turned");
        let near =
            |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() <= 1.0 && (a.1 - b.1).abs() <= 1.0;
        r.check(
            "counter: XL keeps the centre and the pin's proportions; swap trades colour and number",
            big.is_some_and(|(w, h, ce)| {
                w > 40 && h == (w * 13 + 5) / 10 && centre0.is_some_and(|c0| near(c0, ce))
            }) && shown == 3
                && matches!((before, after), (Some((col, d)), Some((col2, d2)))
                    if d2 == Some(col) && Some(col2) == d.or(Some(col2))),
            format!("{big:?} centre {centre0:?} · size {shown} · {before:?} → {after:?}"),
        );
        for _ in 0..5 {
            key(app, ui, "z", true, false);
        }
        key(app, ui, "\u{1b}", false, false);
    }));
    // Cursors (ZK-47) and dragging rows of the layers list (ZK-54).
    steps.push(Box::new(|app, ui, r| {
        let order = |app: &Shared| -> Vec<(u32, u32)> {
            app.borrow()
                .s
                .as_ref()
                .map(|s| s.ed.doc.objects.iter().map(|o| (o.id, o.group)).collect())
                .unwrap_or_default()
        };
        // Cursor: over the first rectangle with Select → move; far outside → the arrow;
        // Rectangle tool → crosshair; Text tool → I-beam; a selected box's corner → resize.
        let rect = app.borrow().s.as_ref().and_then(|s| {
            s.ed.doc
                .objects
                .iter()
                .find(|o| o.kind() == znimok_core::Kind::Rect)
                .map(|o| (o.id, o.rect))
        });
        if let Some((id, rc)) = rect {
            let (cx, cy) = rc.center();
            ui.invoke_tool_chosen(0);
            app.borrow_mut().layer_click(ui, id as i32, false);
            let a = app.borrow();
            let over = a.cursor_probe(cx, cy, false);
            let empty = a.cursor_probe(-500.0, -500.0, false);
            let corner = a.cursor_probe(rc.x as f64, rc.y as f64, false);
            drop(a);
            ui.invoke_tool_chosen(1);
            let draw = app.borrow().cursor_probe(-500.0, -500.0, false);
            let draw_ctrl = app.borrow().cursor_probe(cx, cy, true);
            ui.invoke_tool_chosen(5);
            let text = app.borrow().cursor_probe(-500.0, -500.0, false);
            ui.invoke_tool_chosen(0);
            r.check(
                "cursors: move over a mark, arrow on empty, resize on a corner, cross / I-beam",
                (over, empty, corner, draw, draw_ctrl, text) == (3, 0, 4, 1, 3, 2),
                format!("{over} {empty} {corner} {draw} {draw_ctrl} {text}"),
            );
        }
        ui.set_insp_tab(1);
        // Drag the front row below the last one: it goes to the very back, one undo step.
        let before = order(app);
        let front = before.last().map(|e| e.0);
        let n_rows = slint::Model::row_count(&ui.get_layers()) as f32;
        ui.invoke_layer_drag(0, n_rows * 34.0 + 10.0, 1);
        let after = order(app);
        r.check(
            "layers: drag the front row to the bottom",
            after.first().map(|e| e.0) == front && after.len() == before.len(),
            format!(
                "{:?} → {:?}",
                before.iter().map(|e| e.0).collect::<Vec<_>>(),
                after.iter().map(|e| e.0).collect::<Vec<_>>()
            ),
        );
        same_as_full(app, ui, r, "reordered");
        ui.invoke_undo();
        r.check(
            "layers: undo restores the order",
            order(app) == before,
            String::new(),
        );
        // Drop row 1 onto row 0: the two form a group, shown as a header with 2 members.
        ui.invoke_layer_drag(1, 16.0, 1);
        let now = order(app);
        let top: Vec<(u32, u32)> = now.iter().rev().take(2).copied().collect();
        let grouped = top.len() == 2 && top[0].1 != 0 && top[0].1 == top[1].1;
        let header = slint::Model::row_data(&ui.get_layers(), 0)
            .is_some_and(|row| row.is_group && row.count == 2);
        r.check(
            "layers: drop onto a row groups the two",
            grouped && header,
            format!("{top:?} header {header}"),
        );
        // A drag in progress, for the snapshot: row 4 over the middle of row 3.
        ui.set_layer_drag_from(4);
        ui.set_layer_drag_y(3.0 * 34.0 + 16.0);
        ui.invoke_layer_drag(4, 3.0 * 34.0 + 16.0, 0);
    }));
    steps.push(Box::new(|app, ui, r| {
        r.snapshot(ui, "21-layers-drag");
        ui.invoke_layer_drag(4, 0.0, 2);
        ui.set_layer_drag_from(-1);
        let _ = app;
    }));
    // The same with real pointer events (owner 29.09: dragging layers did nothing — pressing a
    // row selected it, the list was rebuilt and the pressed row lost the press). Find a row by
    // pressing and moving until a drag starts, then carry it two rows down and let go.
    steps.push(Box::new(|app, ui, r| {
        use slint::platform::{PointerEventButton, WindowEvent};
        let order = |app: &Shared| -> Vec<u32> {
            app.borrow()
                .s
                .as_ref()
                .map(|s| s.ed.doc.objects.iter().map(|o| o.id).collect())
                .unwrap_or_default()
        };
        let win = ui.window();
        let w = win.size().width as f32 / win.scale_factor();
        let x = w - 284.0 + 110.0;
        let at = |x: f32, y: f32| slint::LogicalPosition::new(x, y);
        let before = order(app);
        let mut found = None;
        let mut y = 100.0;
        while y < 420.0 && found.is_none() {
            win.dispatch_event(WindowEvent::PointerMoved { position: at(x, y) });
            win.dispatch_event(WindowEvent::PointerPressed {
                position: at(x, y),
                button: PointerEventButton::Left,
            });
            for k in 1..=4 {
                win.dispatch_event(WindowEvent::PointerMoved {
                    position: at(x, y + 3.0 * k as f32),
                });
            }
            let from = ui.get_layer_drag_from();
            if from >= 0 {
                found = Some((from, y));
            } else {
                win.dispatch_event(WindowEvent::PointerReleased {
                    position: at(x, y + 12.0),
                    button: PointerEventButton::Left,
                });
                y += 6.0;
            }
        }
        let moved = if let Some((_, y0)) = found {
            // Past the dragged row's own group (row 0 may be a group header with its members).
            let rows = slint::Model::row_count(&ui.get_layers()) as f32;
            let end = y0 + 34.0 * (rows - 1.0) + 10.0;
            let steps = 20;
            for k in 1..=steps {
                let t = k as f32 / steps as f32;
                win.dispatch_event(WindowEvent::PointerMoved {
                    position: at(x, y0 + 12.0 + (end - y0 - 12.0) * t),
                });
            }
            win.dispatch_event(WindowEvent::PointerReleased {
                position: at(x, end),
                button: PointerEventButton::Left,
            });
            order(app) != before
        } else {
            false
        };
        r.check(
            "layers: a real mouse drag of a row reorders the layers",
            moved && ui.get_layer_drag_from() < 0,
            format!("drag started at {found:?}"),
        );
        if moved {
            ui.invoke_undo();
        }
        // The outline row of a text (owner 29.09: it did nothing).
        let text_id = app.borrow().s.as_ref().and_then(|s| {
            s.ed.doc
                .objects
                .iter()
                .find(|o| o.kind() == znimok_core::Kind::Text)
                .map(|o| o.id)
        });
        let outline = |app: &Shared| {
            app.borrow().s.as_ref().and_then(|s| {
                s.ed.doc
                    .objects
                    .iter()
                    .find(|o| Some(o.id) == text_id)
                    .map(|o| o.style.color2)
            })
        };
        if let Some(id) = text_id {
            ui.invoke_layer_click(id as i32, false);
            ui.invoke_set_prop("outline".into(), 3);
            let on = outline(app);
            ui.invoke_set_prop("outline".into(), -1);
            let off = outline(app);
            r.check(
                "text: the outline colour row sets and clears the outline",
                matches!(on, Some(Some(_))) && off == Some(None) && ui.get_fill_index() < 0,
                format!("{on:?} → {off:?}"),
            );
        } else {
            r.check(
                "text: the outline colour row sets and clears the outline",
                false,
                "no text".into(),
            );
        }
    }));
    steps.push(Box::new(|app, ui, r| {
        r.snapshot(ui, "10-layers");
        ui.invoke_meta_edited("title".into(), "Тестова назва".into());
        ui.invoke_meta_edited("tags".into(), "тест, znimok ,".into());
        let (name, tags) = {
            let a = app.borrow();
            let d = &a.s.as_ref().unwrap().ed.doc;
            (d.name.clone(), d.meta.tags.clone())
        };
        r.check(
            "meta: title and tags",
            name == "Тестова назва" && tags == vec!["тест".to_string(), "znimok".to_string()],
            format!("{name} · {tags:?}"),
        );
        ui.set_insp_tab(2);
    }));
    steps.push(Box::new(|app, ui, r| {
        r.snapshot(ui, "11-image-tab");
        // ZK-53: turns, mirrors, tone — the developed picture follows the recipe.
        let size = |app: &Shared| app.borrow().s.as_ref().map(|s| s.ed.doc.image_size());
        let before = size(app);
        ui.invoke_image_action("rotate-right".into());
        let turned = size(app);
        r.check(
            "rotate right swaps width and height",
            before.zip(turned).is_some_and(|(b, t)| b == (t.1, t.0)),
            format!("{before:?} → {turned:?}"),
        );
        ui.invoke_image_action("rotate-left".into());
        ui.invoke_image_action("mirror-v".into());
        ui.invoke_image_action("mirror-v".into());
        let rec = app.borrow().s.as_ref().map(|s| s.ed.doc.recipe);
        r.check(
            "rotate back, mirror twice = as captured",
            rec.is_some_and(|r| r.rot_quarters % 4 == 0 && !r.mirror),
            format!("{rec:?}"),
        );
        ui.invoke_set_tone("exposure".into(), 0.6, false);
        ui.invoke_set_tone("exposure".into(), 0.75, true);
        ui.invoke_set_tone("contrast".into(), 0.7, true);
        let rec = app.borrow().s.as_ref().map(|s| s.ed.doc.recipe);
        r.check(
            "tone sliders: +1 EV, +20 contrast",
            rec.is_some_and(|r| r.exposure == 1.0 && r.contrast == 20),
            format!("{rec:?} · {}", ui.get_tone_exposure_text()),
        );
        ui.invoke_compare(true);
    }));
    steps.push(Box::new(|app, ui, r| {
        r.snapshot(ui, "17-compare");
        ui.invoke_compare(false);
        // One slider drag = one undo step: two drags (exposure, contrast) → two undos.
        ui.invoke_undo();
        let rec = app.borrow().s.as_ref().map(|s| s.ed.doc.recipe);
        r.check(
            "tone drag is one undo step",
            rec.is_some_and(|r| r.exposure == 1.0 && r.contrast == 0),
            format!("{rec:?}"),
        );
        ui.invoke_redo();
    }));
    steps.push(Box::new(|app, ui, r| {
        r.snapshot(ui, "16-tone");
        ui.invoke_image_action("tone-reset".into());
        let def = ui.get_tone_default();
        r.check("tone reset", def, String::new());
        // Crop tool: the whole picture in view, a 200 × 100 frame typed in, Enter applies.
        ui.invoke_tool_chosen(10);
        ui.invoke_set_crop_size("w".into(), "200".into());
        ui.invoke_set_crop_size("h".into(), "100".into());
        let _ = app;
    }));
    steps.push(Box::new(|app, ui, r| {
        r.snapshot(ui, "18-crop");
        r.check(
            "crop tool opens the Image tab",
            ui.get_tool() == 10 && ui.get_insp_tab() == 2,
            format!("tool {} tab {}", ui.get_tool(), ui.get_insp_tab()),
        );
        ui.invoke_key("\n".into(), false, false, false);
        let crop = app.borrow().s.as_ref().and_then(|s| s.ed.doc.crop);
        r.check(
            "Enter applies the crop",
            crop.is_some_and(|c| (c.w, c.h) == (200, 100)) && ui.get_tool() == 0,
            format!("{crop:?}"),
        );
        ui.invoke_undo();
        let crop = app.borrow().s.as_ref().and_then(|s| s.ed.doc.crop);
        r.check("crop is one undo step", crop.is_none(), format!("{crop:?}"));
        // Esc drops a crop being edited.
        ui.invoke_tool_chosen(10);
        ui.invoke_set_crop_size("w".into(), "50".into());
        ui.invoke_key("\u{1b}".into(), false, false, false);
        let crop = app.borrow().s.as_ref().and_then(|s| s.ed.doc.crop);
        r.check("Esc cancels the crop", crop.is_none(), format!("{crop:?}"));
        // Image size: half the width, proportions kept, then undone.
        let (w, h) = app
            .borrow()
            .s
            .as_ref()
            .map(|s| s.ed.doc.image_size())
            .unwrap_or((2, 2));
        ui.set_size_w((w / 2).to_string().into());
        ui.invoke_size_edited("w".into(), (w / 2).to_string().into());
        ui.invoke_image_action("size-apply".into());
        let now = app.borrow().s.as_ref().map(|s| s.ed.doc.image_size());
        r.check(
            "image size: half, proportions kept",
            now.is_some_and(|(nw, nh)| nw == w / 2 && (nh as i64 - (h / 2) as i64).abs() <= 1),
            format!("{w}×{h} → {now:?}"),
        );
        ui.invoke_undo();
        ui.set_insp_tab(3);
    }));
    steps.push(Box::new(|app, ui, r| {
        r.snapshot(ui, "12-meta-tab");
        ui.set_insp_tab(0);
        ui.invoke_zoom_to(0.75);
        let (sc, ..) = app.borrow().view_probe();
        r.check(
            "zoom slider 0.75 = 400 %",
            (sc - 4.0).abs() < 1e-6,
            format!("scale {sc:.3}"),
        );
        ui.invoke_zoom_fit();
        // Autosave off + a change: leaving asks in our own dialog, not a system box.
        ui.invoke_autosave_toggled(false);
        ui.invoke_meta_edited("title".into(), "Питання".into());
        ui.invoke_back();
        let buttons = slint::Model::row_count(&ui.get_dialog_buttons());
        r.check(
            "leaving unsaved asks in the window",
            ui.get_dialog_open() && buttons == 3 && ui.get_page() == 1,
            format!("open {} · {buttons} buttons", ui.get_dialog_open()),
        );
    }));
    steps.push(Box::new(|_, ui, r| {
        r.snapshot(ui, "19-dialog");
        ui.invoke_dialog_answer(1);
        r.check(
            "Cancel stays in the editor",
            !ui.get_dialog_open() && ui.get_page() == 1,
            format!("page {}", ui.get_page()),
        );
        ui.invoke_meta_edited("title".into(), "Тестова назва".into());
        ui.invoke_autosave_toggled(true);
    }));
    // ZK-41: the card after a quick capture — its own window in the corner, gone by itself.
    steps.push(Box::new(|app, _ui, r| {
        let shot = app.borrow().s.as_ref().map(|s| {
            (
                (*s.ed.doc.banks[s.ed.doc.source as usize]).clone(),
                s.path.clone(),
            )
        });
        if let Some((raster, path)) = shot {
            crate::pill::show(
                raster,
                path,
                "Знімок".into(),
                "Знімок ділянки скопійовано".into(),
                "1600 × 1000 · у буфері й бібліотеці".into(),
                znimok_platform::Rect::new(0, 0, 1920, 1080),
            );
        }
        r.check(
            "card after capture shows",
            crate::pill::is_open(),
            String::new(),
        );
    }));
    // The slide-in takes ~340 ms; the step timer is 350 ms.
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, r| {
        let shot = crate::pill::with_window(|w| w.take_snapshot().ok()).flatten();
        match shot {
            Some(buf) => {
                let path = r.dir.join("22-pill.png");
                let saved =
                    image::RgbaImage::from_raw(buf.width(), buf.height(), buf.as_bytes().to_vec())
                        .is_some_and(|i| i.save(&path).is_ok());
                r.check(
                    "snapshot 22-pill",
                    saved,
                    format!("{}×{}", buf.width(), buf.height()),
                );
            }
            None => r.check("snapshot 22-pill", false, "no card window".into()),
        }
        // ZK-133: a drag out to another app leaves the hover pause stuck; with the pointer away
        // and no button down the card must resume by itself.
        crate::pill::hold_for_test();
    }));
    steps.push(Box::new(|_, _, r| {
        // A disconnected remote session has no pointer to ask about: nothing to check there.
        if crate::pill::pointer_known() {
            r.check(
                "card resumes after a drag left the hover stuck",
                crate::pill::paused() == Some(false) && crate::pill::is_open(),
                format!("paused {:?}", crate::pill::paused()),
            );
        } else {
            println!("skip card resumes after a drag: no pointer (disconnected session)");
        }
        crate::pill::close();
    }));
    // ZK-60: Esc takes off one layer at a time, Enter repeats the last share, [ ] thickness,
    // Ctrl+= zooms in.
    steps.push(Box::new(|app, ui, r| {
        use crate::app::KeyAction;
        let k = |t: &str, ctrl: bool| app.borrow_mut().key(ui, t, ctrl, false);
        // A drawing tool first (choosing one lets go of the selection, ZK-168), then select all.
        app.borrow_mut().set_tool(ui, crate::app::tool::RECT);
        k("a", true);
        let e1 = k("\u{1b}", false);
        let sel_after = ui.get_selection_count();
        let e2 = k("\u{1b}", false);
        let tool_after = ui.get_tool();
        let e3 = k("\u{1b}", false);
        r.check(
            "Esc chain: selection, then tool, then back to the library",
            e1 == KeyAction::None
                && sel_after == 0
                && e2 == KeyAction::None
                && tool_after == 0
                && e3 == KeyAction::Back,
            format!("{e1:?} sel {sel_after} · {e2:?} tool {tool_after} · {e3:?}"),
        );
        let enter = k("\n", false);
        app.borrow_mut().set_last_share(ui, true);
        let enter2 = k("\n", false);
        app.borrow_mut().set_last_share(ui, false);
        r.check(
            "Enter repeats the last share (copy, then export)",
            enter == KeyAction::Copy && enter2 == KeyAction::ExportRepeat,
            format!("{enter:?} / {enter2:?}"),
        );
        let t0 = ui.get_thick_index();
        k("]", false);
        let t1 = ui.get_thick_index();
        k("х", false); // [ on the Ukrainian layout
        let t2 = ui.get_thick_index();
        r.check(
            "] thicker, [ thinner (either layout)",
            t1 == t0 + 1 && t2 == t0,
            format!("{t0} → {t1} → {t2}"),
        );
        app.borrow_mut().stop_anim_for_test();
        let (s0, ..) = app.borrow().view_probe();
        k("=", true);
        app.borrow_mut().stop_anim_for_test();
        let (s1, ..) = app.borrow().view_probe();
        r.check(
            "Ctrl+= zooms in half a stop",
            (s1 / s0 - 2f64.sqrt()).abs() < 1e-3,
            format!("{s0:.3} → {s1:.3}"),
        );
        ui.invoke_zoom_fit();
    }));
    // Tooltip bubble: arm it as a hover over the Undo button would, wait past the delay.
    steps.push(Box::new(|_, ui, _| {
        let tip = ui.global::<crate::Tip>();
        tip.set_text("Скасувати  Ctrl+Z".into());
        tip.set_side(0);
        tip.set_ax(ui.window().size().width as f32 / ui.window().scale_factor() - 214.0);
        tip.set_ay(42.0);
        tip.set_serial(tip.get_serial() + 1);
        tip.set_armed(true);
    }));
    // The bubble appears after 550 ms; one more step (350 ms) before the snapshot.
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, ui, r| {
        r.snapshot(ui, "13-tooltip");
        ui.global::<crate::Tip>().set_armed(false);
    }));
    // Autosave fires ~0.7 s after the last change on the 400 ms timer; give it time.
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|app, ui, r| {
        let (path, unsaved) = {
            let a = app.borrow();
            (a.s.as_ref().map(|s| s.path.clone()), a.is_unsaved())
        };
        r.check(
            "autosaved",
            !unsaved && ui.get_save_state() == 0,
            format!("state {}", ui.get_save_state()),
        );
        let peek = path
            .as_deref()
            .and_then(crate::library::read_entry)
            .map(|e| (e.width, e.height, e.thumb_png.is_some()));
        r.check(
            "file in library",
            peek.is_some_and(|p| p.2),
            format!("{peek:?}"),
        );
        ui.invoke_back();
    }));
    steps.push(Box::new(|app, ui, r| {
        let n = slint::Model::row_count(&ui.get_cards());
        r.check(
            "library shows the card",
            ui.get_page() == 0 && n == 1,
            format!("page {}, {n} cards", ui.get_page()),
        );
        r.snapshot(ui, "04-library");
        // ZK-55: rename from the card, to the trash, "Undo" brings it back.
        if let Some(c) = slint::Model::row_data(&ui.get_cards(), 0) {
            ui.invoke_card_rename(c.path.clone(), "Перейменований".into());
            let renamed = slint::Model::row_data(&ui.get_cards(), 0).map(|c| c.name.to_string());
            ui.invoke_card_trash(c.path.clone(), false);
            let after_trash = slint::Model::row_count(&ui.get_cards());
            let action = ui.get_toast_action().to_string();
            ui.invoke_toast_action_clicked();
            let back = slint::Model::row_count(&ui.get_cards());
            r.check(
                "library: rename, trash with Undo",
                renamed.as_deref() == Some("Перейменований")
                    && after_trash == 0
                    && !action.is_empty()
                    && back == 1,
                format!(
                    "{renamed:?}, {after_trash} after trash, action {action:?}, {back} after undo"
                ),
            );
        }
        // ZK-131: a second look at the library reads no file (the index), and a file that
        // appears from outside shows up by itself (the watcher).
        {
            use std::sync::atomic::Ordering;
            let before = crate::library::HEAD_READS.load(Ordering::Relaxed);
            app.borrow_mut().refresh_library(ui);
            let reads = crate::library::HEAD_READS.load(Ordering::Relaxed) - before;
            let n0 = slint::Model::row_count(&ui.get_cards());
            let copy = slint::Model::row_data(&ui.get_cards(), 0).map(|c| {
                let src = std::path::PathBuf::from(c.path.as_str());
                let to = src.with_file_name("Znimok-selftest-outside.znimok");
                let _ = std::fs::copy(&src, &to);
                to
            });
            app.borrow_mut().lib_poll(ui, true);
            let n1 = slint::Model::row_count(&ui.get_cards());
            if let Some(p) = &copy {
                let _ = std::fs::remove_file(p);
            }
            app.borrow_mut().lib_poll(ui, true);
            let n2 = slint::Model::row_count(&ui.get_cards());
            r.check(
                "library: the index spares re-reading files; files from outside appear and go",
                reads == 0 && n1 == n0 + 1 && n2 == n0,
                format!("{reads} files read again · cards {n0} → {n1} → {n2}"),
            );
        }
        // Shift+trash: one question, then deleted for good (not in the trash).
        if let Some(c) = slint::Model::row_data(&ui.get_cards(), 0) {
            let src = std::path::PathBuf::from(c.path.as_str());
            let copy = src.with_file_name("Znimok-selftest-forever.znimok");
            let _ = std::fs::copy(&src, &copy);
            ui.invoke_card_trash(copy.to_string_lossy().to_string().into(), true);
            let asked = ui.get_dialog_open();
            ui.invoke_dialog_answer(0);
            let lib = src.parent().map(|p| p.to_path_buf()).unwrap_or_default();
            let in_trash = crate::library::trash_dir(&lib)
                .join("Znimok-selftest-forever.znimok")
                .exists();
            r.check(
                "library: Shift+trash asks, then deletes for good",
                asked
                    && !copy.exists()
                    && !in_trash
                    && slint::Model::row_count(&ui.get_cards()) == 1,
                format!(
                    "asked {asked}, file left {}, in trash {in_trash}",
                    copy.exists()
                ),
            );
        }
        // ZK-176: picking cards with Ctrl / Shift; ZK-175: the picked ones to the trash, the
        // trash page (a click picks, nothing opens), Restore, Destroy all after one question.
        if let Some(c) = slint::Model::row_data(&ui.get_cards(), 0) {
            let src = std::path::PathBuf::from(c.path.as_str());
            let lib = src.parent().map(|p| p.to_path_buf()).unwrap_or_default();
            let (pa, pb) = (
                lib.join("Znimok-selftest-pick-a.znimok"),
                lib.join("Znimok-selftest-pick-b.znimok"),
            );
            let _ = std::fs::copy(&src, &pa);
            let _ = std::fs::copy(&src, &pb);
            app.borrow_mut().lib_poll(ui, true);
            let s_of = |p: &std::path::Path| slint::SharedString::from(p.to_string_lossy().as_ref());
            let marked = |ui: &AppWindow| {
                (0..slint::Model::row_count(&ui.get_cards()))
                    .filter(|i| slint::Model::row_data(&ui.get_cards(), *i).is_some_and(|c| c.selected))
                    .count()
            };
            ui.invoke_card_click(s_of(&pa), 1);
            ui.invoke_card_click(s_of(&pb), 2);
            let range = ui.get_picked_count();
            ui.invoke_lib_action("pick-none".into(), "".into());
            ui.invoke_card_click(s_of(&pa), 1);
            ui.invoke_card_click(s_of(&pb), 1);
            ui.invoke_card_click(s_of(&pb), 1);
            let toggled = ui.get_picked_count();
            ui.invoke_card_click(s_of(&pb), 1);
            let picked = (ui.get_picked_count(), marked(ui));
            ui.invoke_lib_action("picked-trash".into(), "".into());
            let left = slint::Model::row_count(&ui.get_cards());
            let undo = !ui.get_toast_action().is_empty();
            ui.invoke_lib_action("view-trash".into(), "".into());
            let in_trash = (ui.get_trash_view(), slint::Model::row_count(&ui.get_cards()), ui.get_trash_count());
            r.snapshot(ui, "04b-trash");
            let windows = crate::wins::editors().len();
            let first = slint::Model::row_data(&ui.get_cards(), 0).map(|c| c.path).unwrap_or_default();
            ui.invoke_open_card(first.clone());
            let not_opened = crate::wins::editors().len() == windows && ui.get_picked_count() == 1;
            ui.invoke_lib_action("restore".into(), first);
            let after_restore = slint::Model::row_count(&ui.get_cards());
            ui.invoke_lib_action("trash-empty".into(), "".into());
            let asked = ui.get_dialog_open();
            ui.invoke_dialog_answer(0);
            let emptied = (slint::Model::row_count(&ui.get_cards()), ui.get_trash_count());
            ui.invoke_lib_action("view-library".into(), "".into());
            let back = (!ui.get_trash_view(), slint::Model::row_count(&ui.get_cards()));
            let days = app.borrow().prefs().library.trash_days;
            r.check(
                "cards: Ctrl picks one, Shift a range; the picked go to the trash with Undo",
                range >= 2 && toggled == 1 && picked == (2, 2) && left == 1 && undo,
                format!("range {range} · toggled {toggled} · picked {picked:?} · left {left} · undo {undo}"),
            );
            r.check(
                "trash page: two cards, a click does not open, Restore, Destroy all asks; 7 days by default",
                in_trash == (true, 2, 2)
                    && not_opened
                    && after_restore == 1
                    && asked
                    && emptied == (0, 0)
                    && back == (true, 2)
                    && days == 7,
                format!("{in_trash:?} · not opened {not_opened} · {after_restore} after restore · asked {asked} · {emptied:?} · back {back:?} · {days} days"),
            );
            // The one restored goes, so the library has its single card again.
            let _ = std::fs::remove_file(&pa);
            let _ = std::fs::remove_file(&pb);
            app.borrow_mut().lib_poll(ui, true);
        }
        // ZK-177/178/179: pinning puts the card in its own group first and into its file; the
        // library's limit never trashes it; the keys move through the groups.
        if let Some(c) = slint::Model::row_data(&ui.get_cards(), 0) {
            let src = std::path::PathBuf::from(c.path.as_str());
            let lib = src.parent().map(|p| p.to_path_buf()).unwrap_or_default();
            let titles = |ui: &AppWindow| -> Vec<String> {
                let h = ui.get_lib_headers();
                (0..slint::Model::row_count(&h))
                    .filter_map(|i| slint::Model::row_data(&h, i).map(|x| x.title.to_string()))
                    .collect()
            };
            let before = titles(ui);
            ui.invoke_lib_action("pin".into(), c.path.clone());
            let pinned_file = crate::library::read_entry(&src).is_some_and(|e| e.pinned);
            let after = titles(ui);
            let (pa, pb) = (
                lib.join("Znimok-selftest-keys-a.znimok"),
                lib.join("Znimok-selftest-keys-b.znimok"),
            );
            // Unpinned copies (a pinned file's copy is pinned too).
            let _ = std::fs::copy(&src, &pa);
            let _ = std::fs::copy(&src, &pb);
            for p in [&pa, &pb] {
                if let Ok((mut doc, v)) = znimok_format::open_parts(p) {
                    doc.meta.pinned = false;
                    let _ = znimok_format::save_same_kind(p, &doc, v.as_ref(), &Default::default());
                }
            }
            app.borrow_mut().lib_poll(ui, true);
            let cur = |ui: &AppWindow| {
                (0..slint::Model::row_count(&ui.get_cards()))
                    .find(|i| slint::Model::row_data(&ui.get_cards(), *i).is_some_and(|c| c.current))
            };
            ui.invoke_lib_key("home".into(), false);
            let home = cur(ui);
            ui.invoke_lib_key("down".into(), false);
            let down = cur(ui);
            ui.invoke_lib_key("right".into(), false);
            let right = cur(ui);
            ui.invoke_lib_key("left".into(), true);
            let (left, grown) = (cur(ui), ui.get_picked_count());
            ui.invoke_lib_action("pick-none".into(), "".into());
            r.snapshot(ui, "04c-groups");
            // The limit: one unpinned card kept; the pinned one is neither counted nor trashed.
            ui.set_pref_ret_count("1".into());
            ui.invoke_setting("ret-count".into(), 0);
            app.borrow_mut().apply_retention(ui);
            let kept_pin = src.exists();
            let copies_left = [&pa, &pb].iter().filter(|p| p.exists()).count();
            ui.set_pref_ret_count("100".into());
            ui.invoke_setting("ret-count".into(), 0);
            r.check(
                "library groups: pinned first, the pin is in the file",
                before.len() == 1 && after.first().map(String::as_str) == Some("Закріплені") && pinned_file,
                format!("{before:?} → {after:?} · file pinned {pinned_file}"),
            );
            r.check(
                "library keys: Home, Down into the next group, Right, Shift+Left grows the pick",
                home == Some(0) && down == Some(1) && right == Some(2) && left == Some(1) && grown == 2,
                format!("home {home:?} · down {down:?} · right {right:?} · left {left:?} · picked {grown}"),
            );
            r.check(
                "library limit: the pinned card is never trashed",
                kept_pin && copies_left == 1,
                format!("pinned kept {kept_pin} · copies left {copies_left}"),
            );
            ui.invoke_lib_action("pin".into(), c.path.clone());
            let _ = std::fs::remove_file(&pa);
            let _ = std::fs::remove_file(&pb);
            let _ = std::fs::remove_dir_all(crate::library::trash_dir(&lib));
            app.borrow_mut().lib_poll(ui, true);
        }
        // ZK-56: the settings page; a switch goes into settings.json at once; language live.
        ui.invoke_settings_open();
        ui.set_settings_page(2);
    }));
    steps.push(Box::new(|app, ui, r| {
        r.snapshot(ui, "24-settings");
        let page = ui.get_page();
        ui.invoke_setting("metadata".into(), 0);
        let file = std::fs::read_to_string(r.dir.join("settings.json")).unwrap_or_default();
        let saved = file.contains("\"write_metadata\": false");
        ui.invoke_setting("metadata".into(), 1);
        ui.invoke_setting("lang".into(), 2);
        let en = app.borrow().tr.tr("set-title");
        ui.invoke_setting("lang".into(), 1);
        let uk = app.borrow().tr.tr("set-title");
        ui.invoke_setting("lang".into(), 0);
        // ZK-155/156: the gestures page; giving «release» the over-the-screen editor swaps it
        // with Alt, the file keeps a permutation; the hint strip switch is saved.
        {
            use znimok_settings::CaptureAction as A;
            ui.set_settings_page(0);
            r.snapshot(ui, "24b-settings-shots");
            ui.set_settings_page(10);
            r.snapshot(ui, "24c-settings-recording");
            ui.set_settings_page(0);
            ui.invoke_setting("gesture-plain".into(), 1);
            let g = app.borrow().prefs().capture.gestures;
            let shown = (ui.get_pref_gesture_plain(), ui.get_pref_gesture_alt());
            ui.invoke_setting("show-hints".into(), 0);
            let hints_off = !app.borrow().prefs().capture.show_hints;
            ui.invoke_setting("show-hints".into(), 1);
            ui.invoke_setting("gesture-plain".into(), 0);
            let back = app.borrow().prefs().capture.gestures;
            r.check(
                "gestures swap and are saved; hint strip switch saved",
                (g.plain, g.shift, g.alt) == (A::OverScreen, A::Clipboard, A::Editor)
                    && shown == (1, 0)
                    && hints_off
                    && back == znimok_settings::Gestures::default(),
                format!("{g:?}, shown {shown:?}, hints off {hints_off}, back {back:?}"),
            );
            ui.set_settings_page(2);
        }
        // ZK-44: record a combination for the region shot (physical keys), Esc cancels a
        // recording, "Restore defaults" brings the defaults back.
        {
            use slint::winit_030::winit::keyboard::ModifiersState;
            ui.invoke_setting("key-record".into(), 0);
            let waiting = ui.get_key_recording() == 0;
            let mods = ModifiersState::CONTROL | ModifiersState::ALT | ModifiersState::SHIFT;
            app.borrow_mut().hotkey_key(ui, "F13", mods);
            let want = znimok_platform::KeyCombo::parse("Ctrl+Alt+Shift+F13").ok();
            let active = crate::hotkeys::active(crate::hotkeys::Action::Region);
            let saved = app.borrow().prefs().capture.hotkeys.region;
            ui.invoke_setting("key-record".into(), 1);
            app.borrow_mut()
                .hotkey_key(ui, "Escape", ModifiersState::empty());
            let cancelled = ui.get_key_recording() == -1;
            ui.invoke_setting("keys-defaults".into(), 0);
            let back = app.borrow().prefs().capture.hotkeys.region;
            r.check(
                "hotkey recorded (physical keys), registered, saved; Esc cancels; defaults back",
                waiting
                    && active == want
                    && saved == want
                    && cancelled
                    && back == znimok_settings::Hotkeys::default().region,
                format!("active {active:?}, saved {saved:?}, back {back:?}"),
            );
        }
        ui.invoke_setting("close".into(), 0);
        r.check(
            "settings: page, saved at once, language switches live, Esc back",
            page == 2 && saved && en == "Settings" && uk == "Налаштування" && ui.get_page() == 0,
            format!(
                "page {page}, saved {saved}, {en} / {uk}, back to page {}",
                ui.get_page()
            ),
        );
        if let Some(c) = slint::Model::row_data(&ui.get_cards(), 0) {
            ui.invoke_open_card(c.path);
        }
    }));
    // ZK-57: the first-run guide (from the settings here; on a fresh profile it opens by
    // itself). Start at login is not switched in the test: that writes the real Run key.
    steps.push(Box::new(|_, ui, _| {
        ui.invoke_setting("onb-open".into(), 0);
    }));
    // From the guide to the hotkeys and back to the guide, not past it (owner, 29.09).
    steps.push(Box::new(|_, ui, r| {
        ui.invoke_setting("onb-keys".into(), 0);
        let in_keys = ui.get_page() == 2 && ui.get_settings_page() == 1;
        ui.invoke_setting("close".into(), 0);
        r.check(
            "guide → hotkeys → back returns to the guide",
            in_keys && ui.get_page() == 3,
            format!("page {}", ui.get_page()),
        );
    }));
    steps.push(Box::new(|app, ui, r| {
        r.snapshot(ui, "27-onboarding");
        let shown = ui.get_page() == 3;
        // "Don't show next time" unticked: the guide comes back next start.
        ui.invoke_setting("onb-done".into(), 0);
        let again = !app.borrow().prefs().general.onboarding_done;
        ui.invoke_setting("onb-open".into(), 0);
        // Ticked (the default): remembered.
        ui.invoke_setting("onb-done".into(), 1);
        let done = app.borrow().prefs().general.onboarding_done;
        r.check(
            "first-run guide: shown, Done remembers it and returns",
            shown && again && done && ui.get_page() == 1,
            format!("shown {shown}, saved {done}, page {}", ui.get_page()),
        );
    }));
    steps.push(Box::new(|app, ui, r| {
        r.check(
            "reopened with marks",
            ui.get_page() == 1 && count(app) == 7,
            format!("{} marks", count(app)),
        );
        r.snapshot(ui, "05-reopened");
        let dir = r.dir.clone();
        for ext in ["png", "jpg", "webp"] {
            let p = dir.join(format!("export.{ext}"));
            app.borrow_mut().export_to(ui, &p);
            let dims = image::image_dimensions(&p).ok();
            let want = app.borrow().s.as_ref().map(|s| {
                let f = s.ed.doc.frame();
                (f.w as u32, f.h as u32)
            });
            r.check(
                &format!("export {ext}"),
                dims.is_some() && dims == want,
                format!("{dims:?}"),
            );
        }
        // ZK-61: the exported files carry the document's title and the shot's time.
        let (title, created) = app
            .borrow()
            .s
            .as_ref()
            .map(|s| (s.ed.doc.name.clone(), s.ed.doc.meta.created_ms))
            .unwrap_or_default();
        let png = std::fs::read(dir.join("export.png")).unwrap_or_default();
        let jpg = std::fs::read(dir.join("export.jpg")).unwrap_or_default();
        let contains = |hay: &[u8], needle: &[u8]| hay.windows(needle.len()).any(|w| w == needle);
        let title16: Vec<u8> = title.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        let mtime = std::fs::metadata(dir.join("export.png"))
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64);
        r.check(
            "metadata in export: PNG iTXt title, JPEG EXIF title, file time = shot time",
            contains(&png, b"iTXtTitle")
                && contains(&png, title.as_bytes())
                && contains(&jpg, b"Exif\0\0")
                && contains(&jpg, &title16)
                && mtime.is_some_and(|m| (m - created).abs() < 2000),
            format!("title {title:?}, mtime {mtime:?} vs {created}"),
        );
        // ZK-64: the file a drag out carries — a PNG named after the document, full frame.
        let file = app.borrow_mut().drag_file();
        let want = app.borrow().s.as_ref().map(|s| {
            let f = s.ed.doc.frame();
            (f.w as u32, f.h as u32)
        });
        let dims = file
            .as_ref()
            .ok()
            .and_then(|p| image::image_dimensions(p).ok());
        r.check(
            "drag-out file: PNG of the frame",
            dims.is_some()
                && dims == want
                && file
                    .as_ref()
                    .is_ok_and(|p| p.extension().is_some_and(|e| e == "png")),
            format!("{file:?} {dims:?}"),
        );
    }));

    // ZK-187: the export sheet — estimates for every format, JPEG's quality changes the size,
    // 50 % halves the sides, the clipboard gets the chosen size, Esc closes.
    steps.push(Box::new(|_, ui, _| {
        ui.invoke_export();
    }));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|app, ui, r| {
        let sizes = app.borrow().export_sizes();
        r.snapshot(ui, "40-export");
        r.check(
            "export sheet: opens with a size estimate for PNG, JPEG and WebP",
            ui.get_exp_open() && sizes.is_some_and(|s| s.iter().all(|b| b.is_some_and(|n| n > 0))),
            format!("{sizes:?}"),
        );
        let dir = r.dir.clone();
        let write = |q: i32, file: &str| {
            ui.invoke_exp_set("format".into(), 1);
            ui.invoke_exp_set("quality".into(), q);
            ui.invoke_exp_set("scale".into(), 1);
            ui.invoke_exp_set("remember".into(), 0);
            let path = dir.join(file);
            app.borrow_mut().export_write(ui, &path);
            std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0)
        };
        ui.invoke_export();
        let q60 = write(60, "sheet-q60.jpg");
        ui.invoke_export();
        let q90 = write(90, "sheet-q90.jpg");
        r.check(
            "export sheet: JPEG quality 60 gives a smaller file than 90",
            q60 > 0 && q90 > q60,
            format!("{q60} vs {q90} bytes"),
        );
        // ZK-197: WebP through libwebp with its own quality (JPEG's stays), or lossless.
        let webp = |q: i32, lossless: bool, file: &str| {
            ui.invoke_export();
            ui.invoke_exp_set("format".into(), 2);
            ui.invoke_exp_set("lossless".into(), lossless as i32);
            ui.invoke_exp_set("quality".into(), q);
            ui.invoke_exp_set("scale".into(), 1);
            ui.invoke_exp_set("remember".into(), 0);
            let shown = ui.get_exp_quality();
            let path = dir.join(file);
            app.borrow_mut().export_write(ui, &path);
            let n = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            let ok = image::open(&path).is_ok_and(|i| i.width() > 0);
            (n, ok, shown)
        };
        let (w40, ok40, shown) = webp(40, false, "sheet-q40.webp");
        let (w90, ok90, _) = webp(90, false, "sheet-q90.webp");
        let (wll, okll, _) = webp(90, true, "sheet-lossless.webp");
        ui.invoke_export();
        ui.invoke_exp_set("format".into(), 1);
        let jpeg_kept = ui.get_exp_quality() == 90;
        ui.invoke_exp_close();
        r.check(
            "export sheet: WebP quality 40 < 90 < lossless, all readable; JPEG keeps its own quality",
            ok40 && ok90 && okll && w40 < w90 && w90 < wll && shown == 40 && jpeg_kept,
            format!("{w40} < {w90} < {wll} bytes · shown {shown} · JPEG kept {jpeg_kept}"),
        );
        // 50 %: half the sides, PNG.
        ui.invoke_export();
        ui.invoke_exp_set("format".into(), 0);
        ui.invoke_exp_set("scale".into(), 0);
        ui.invoke_exp_set("remember".into(), 0);
        let half = dir.join("sheet-half.png");
        app.borrow_mut().export_write(ui, &half);
        let dims = image::image_dimensions(&half).ok();
        let frame = app.borrow().s.as_ref().map(|s| {
            let f = s.ed.doc.frame();
            (f.w as u32, f.h as u32)
        });
        r.check(
            "export sheet: 50 % halves the sides",
            matches!((dims, frame), (Some((w, h)), Some((fw, fh))) if w == fw.div_ceil(2) && h == fh.div_ceil(2)),
            format!("{dims:?} of {frame:?}"),
        );
        // To the clipboard at 200 %.
        ui.invoke_export();
        ui.invoke_exp_set("scale".into(), 2);
        ui.invoke_exp_set("to".into(), 0);
        ui.invoke_exp_set("remember".into(), 0);
        ui.invoke_exp_go_clicked();
        let clip = arboard::Clipboard::new()
            .and_then(|mut c| c.get_image())
            .ok()
            .map(|i| (i.width as u32, i.height as u32));
        r.check(
            "export sheet: to the clipboard at 200 %, the sheet closes",
            matches!((clip, frame), (Some((w, h)), Some((fw, fh))) if w == fw * 2 && h == fh * 2)
                && !ui.get_exp_open(),
            format!("{clip:?} · open {}", ui.get_exp_open()),
        );
        ui.invoke_export();
        ui.invoke_exp_close();
        r.check("export sheet: closes", !ui.get_exp_open(), String::new());
    }));

    // Capture overlay on a synthetic frozen frame: hover a window, drag a region.
    steps.push(Box::new(|app, _ui, r| {
        let raster = app
            .borrow()
            .s
            .as_ref()
            .map(|s| (*s.ed.doc.banks[0]).clone());
        OVER_SRC.with(|o| *o.borrow_mut() = raster.clone());
        let Some(raster) = raster else {
            r.check("overlay opened", false, "no document".into());
            return;
        };
        let frozen = crate::capture::Frozen {
            displays: Vec::new(),
            bounds: znimok_platform::Rect {
                x: 0,
                y: 0,
                width: raster.width,
                height: raster.height,
            },
            windows: vec![crate::capture::FrozenWindow {
                rect: crate::capture::PxRect {
                    x: 100,
                    y: 100,
                    w: 400,
                    h: 300,
                },
                title: "test window".into(),
                id: 0,
            }],
            raster,
        };
        let ok = crate::overlay::open(frozen, true).is_ok();
        r.check(
            "overlay opened",
            ok && crate::overlay::is_open(),
            String::new(),
        );
    }));
    steps.push(Box::new(|_, _, r| {
        let Some(ov) = crate::overlay::handle() else {
            r.check("overlay hover", false, "closed".into());
            return;
        };
        let sf = ov.window().scale_factor();
        // Logical coordinates of frame pixel (300, 250) — inside the test window.
        let lw = ov.window().size().width as f32 / sf;
        let lh = ov.window().size().height as f32 / sf;
        // Frame pixels per logical pixel, per axis (the synthetic frame need not have the
        // screen's aspect — on the Mac it does not).
        // The frozen frame is the sample given on the command line, whatever its size.
        let (fw, fh) = OVER_SRC
            .with(|o| {
                o.borrow()
                    .as_ref()
                    .map(|r| (r.width as f32, r.height as f32))
            })
            .unwrap_or((1600.0, 1000.0));
        let (kx, ky) = (fw / lw.max(1.0), fh / lh.max(1.0));
        ov.invoke_pointer(1, 300.0 / kx, 250.0 / ky, false, false);
        let (has, win) = (ov.get_has_sel(), ov.get_is_window());
        r.check(
            "overlay hover highlights the window",
            has && win,
            format!("label {}", ov.get_sel_label()),
        );
        // macOS: the overlay must sit above the menu bar, covering the whole screen.
        if let Some((ok, detail)) = crate::overlay::covers_screen() {
            r.check("overlay covers the whole screen (menu bar too)", ok, detail);
        }
        // Guides: over the white window (lit, not veiled) the line must be dark.
        let gh = ov.get_guide_h().to_rgba8();
        let px300 = gh.as_ref().map(|b| b.as_slice()[300]);
        r.check(
            "guides contrast with the background",
            px300.is_some_and(|p| p.r < 40 && p.a > 200)
                && gh
                    .as_ref()
                    .is_some_and(|b| b.width() == fw as u32 && b.height() == 1),
            format!("{px300:?}"),
        );
        r.check(
            "magnifier off by default",
            !ov.get_lens_visible(),
            String::new(),
        );
        // A trackpad-like stream of small deltas turns the lens on one level only.
        for _ in 0..30 {
            ov.invoke_wheel(3.0);
        }
        let on4 = ov.get_lens_visible() && ov.get_lens_coords().ends_with("×4");
        std::thread::sleep(std::time::Duration::from_millis(300));
        ov.invoke_wheel(30.0);
        r.check(
            "wheel: one gesture = one level (×4, then ×8)",
            on4 && ov.get_lens_visible() && ov.get_lens_coords().starts_with("300, 250   ×8"),
            format!(
                "{} · {} · {:.0} px",
                ov.get_lens_coords(),
                ov.get_lens_hex(),
                ov.get_lens_size()
            ),
        );
        let (lw, lh) = (ov.get_lens().size().width, ov.get_lens().size().height);
        r.check(
            "magnifier picture: odd pixel count × zoom",
            lw == lh && lw % 8 == 0 && (lw / 8) % 2 == 1,
            format!("{lw}×{lh}"),
        );
        r.snapshot_window(ov.window(), "07-overlay-hover");
        // Drag frame pixels (600, 500) → (900, 700): a 300 × 200 region.
        ov.invoke_pointer(0, 600.0 / kx, 500.0 / ky, false, false);
        for i in 1..=10 {
            let t = i as f32 / 10.0;
            ov.invoke_pointer(
                1,
                (600.0 + 300.0 * t) / kx,
                (500.0 + 200.0 * t) / ky,
                false,
                false,
            );
        }
        r.check(
            "overlay drag",
            ov.get_has_sel() && !ov.get_is_window(),
            format!("label {}", ov.get_sel_label()),
        );
        r.snapshot_window(ov.window(), "08-overlay-drag");
        ov.invoke_pointer(2, 900.0 / kx, 700.0 / ky, false, false);
    }));
    steps.push(Box::new(|app, ui, r| {
        let size = app.borrow().s.as_ref().map(|s| s.ed.doc.image_size());
        r.check(
            "region opened in the editor",
            !crate::overlay::is_open()
                && ui.get_page() == 1
                && size.is_some_and(|(w, h)| {
                    (w as i32 - 300).abs() <= 2 && (h as i32 - 200).abs() <= 2
                }),
            format!("{size:?}"),
        );
        r.snapshot(ui, "09-region-editor");
    }));

    // ZK-58: Alt on release = edit over the screen. The whole frozen display is the document,
    // the region its crop; the window covers the display without bars; "to the window" keeps
    // the document, marks and undo, and the pixels stay the same.
    steps.push(Box::new(|app, ui, r| {
        let raster = app
            .borrow()
            .s
            .as_ref()
            .map(|s| (*s.ed.doc.banks[0]).clone());
        app.borrow_mut().close_document(ui);
        let Some(raster) = raster else {
            r.check("over the screen: opened", false, "no document".into());
            return;
        };
        let raster = OVER_SRC.with(|o| {
            // The original sample (the region document is only a piece of it).
            o.borrow().clone().unwrap_or(raster)
        });
        let sf = ui.window().scale_factor();
        // Desktop units: points on macOS, pixels on Windows.
        let (bw, bh) = if cfg!(target_os = "macos") {
            (
                (raster.width as f32 / sf).round() as u32,
                (raster.height as f32 / sf).round() as u32,
            )
        } else {
            (raster.width, raster.height)
        };
        let frozen = crate::capture::Frozen {
            displays: Vec::new(),
            bounds: znimok_platform::Rect {
                x: 0,
                y: 0,
                width: bw,
                height: bh,
            },
            windows: Vec::new(),
            raster,
        };
        let _ = crate::overlay::open(frozen, true);
        let Some(ov) = crate::overlay::handle() else {
            r.check("over the screen: opened", false, "overlay closed".into());
            return;
        };
        let osf = ov.window().scale_factor();
        let lw = ov.window().size().width as f32 / osf;
        let lh = ov.window().size().height as f32 / osf;
        // The frozen frame is the sample given on the command line, whatever its size.
        let (fw, fh) = OVER_SRC
            .with(|o| {
                o.borrow()
                    .as_ref()
                    .map(|r| (r.width as f32, r.height as f32))
            })
            .unwrap_or((1600.0, 1000.0));
        let (kx, ky) = (fw / lw.max(1.0), fh / lh.max(1.0));
        ov.invoke_pointer(0, 600.0 / kx, 500.0 / ky, false, false);
        for i in 1..=10 {
            let t = i as f32 / 10.0;
            ov.invoke_pointer(
                1,
                (600.0 + 300.0 * t) / kx,
                (500.0 + 200.0 * t) / ky,
                false,
                true,
            );
        }
        ov.invoke_pointer(2, 900.0 / kx, 700.0 / ky, false, true);
    }));
    steps.push(Box::new(|app, ui, r| {
        let a = app.borrow();
        let over = a.over.is_some();
        let info =
            a.s.as_ref()
                .map(|s| (s.ed.doc.image_size(), s.ed.doc.crop, s.path.exists()));
        drop(a);
        r.check(
            "over the screen: Alt on release opens the frozen display with the region as the frame",
            over && ui.get_over_screen()
                && info.is_some_and(|((w, h), c, _)| {
                    OVER_SRC.with(|o| {
                        o.borrow()
                            .as_ref()
                            .is_some_and(|r| r.width == w && r.height == h)
                    }) && c.is_some_and(|c| {
                        (c.x - 600).abs() <= 2
                            && (c.y - 500).abs() <= 2
                            && (c.w - 300).abs() <= 3
                            && (c.h - 200).abs() <= 3
                    })
                }),
            format!("{info:?}"),
        );
        r.check(
            "over the screen: nothing in the library yet",
            info.is_some_and(|(_, _, on_disk)| !on_disk),
            String::new(),
        );
        let (cw, ww) = (
            ui.get_canvas_width(),
            ui.window().size().width as f32 / ui.window().scale_factor(),
        );
        r.check(
            "over the screen: the canvas is the whole window (no bars, no inspector)",
            (cw - ww).abs() <= 2.0,
            format!("canvas {cw:.0} · window {ww:.0}"),
        );
        // The copy is the frozen pixels under the frame, byte for byte.
        let mut a = app.borrow_mut();
        let flat = a.flatten();
        let want = a.s.as_ref().and_then(|s| {
            let c = s.ed.doc.crop?;
            let b = &s.ed.doc.banks[0];
            let mut v = Vec::with_capacity((c.w * c.h * 4) as usize);
            for y in c.y..c.bottom() {
                let i = ((y as u32 * b.width + c.x as u32) * 4) as usize;
                v.extend_from_slice(&b.rgba[i..i + (c.w * 4) as usize]);
            }
            Some((c.w as u32, c.h as u32, v))
        });
        let same = matches!((&flat, &want), (Some(f), Some(w)) if f == w);
        r.check(
            "over the screen: the picture is the frozen pixels under the frame, byte for byte",
            same,
            format!(
                "{:?} vs {:?}",
                flat.as_ref().map(|f| (f.0, f.1)),
                want.as_ref().map(|w| (w.0, w.1))
            ),
        );
        OVER_FLAT.with(|o| *o.borrow_mut() = flat);
        drop(a);
        ui.set_tool(1);
        ui.invoke_tool_chosen(1);
        // ZK-172: the properties bar stands above the frame when there is room there.
        let room_above = ui.get_ov_y() >= 48.0 + 12.0 + 8.0;
        r.check(
            "over the screen: the properties bar is above the frame when it fits there",
            !room_above || !ui.get_ov_bar_down(),
            format!(
                "frame top {:.0} · bar under {}",
                ui.get_ov_y(),
                ui.get_ov_bar_down()
            ),
        );
        r.snapshot(ui, "28-over-screen");
    }));
    // ZK-157: the frame pulled to the left edge of the screen — the tool rail slides over to
    // its right side (a snapshot after the animation), then the frame goes back (undo).
    steps.push(Box::new(|_app, ui, _r| {
        let (fx, fy) = (ui.get_ov_x(), ui.get_ov_y());
        ui.invoke_pointer(0, fx, fy, 0, false, false);
        for i in 1..=6 {
            ui.invoke_pointer(1, fx - (fx - 2.0) * i as f32 / 6.0, fy, 0, false, false);
        }
        ui.invoke_pointer(2, 2.0, fy, 0, false, false);
    }));
    steps.push(Box::new(|_app, ui, r| {
        r.check(
            "over the screen: the frame at the left edge (the rail moves to its right)",
            ui.get_ov_x() < 60.0,
            format!("frame x {}", ui.get_ov_x()),
        );
        r.snapshot(ui, "28b-over-rail-right");
        ui.invoke_undo();
    }));
    steps.push(Box::new(|app, ui, r| {
        // Draw a rectangle inside the frame, then pull the frame's bottom-right corner.
        let (fx, fy, fw, fh) = (ui.get_ov_x(), ui.get_ov_y(), ui.get_ov_w(), ui.get_ov_h());
        ui.invoke_pointer(0, fx + 30.0, fy + 30.0, 0, false, false);
        for i in 1..=6 {
            ui.invoke_pointer(1, fx + 30.0 + 15.0 * i as f32, fy + 30.0 + 10.0 * i as f32, 0, false, false);
        }
        ui.invoke_pointer(2, fx + 120.0, fy + 90.0, 0, false, false);
        let marks = app.borrow().s.as_ref().map(|s| s.ed.doc.objects.len());
        r.check("over the screen: drawing works", marks == Some(1), format!("{marks:?}"));
        let before = app.borrow().s.as_ref().and_then(|s| s.ed.doc.crop);
        ui.invoke_pointer(0, fx + fw, fy + fh, 0, false, false);
        for i in 1..=5 {
            ui.invoke_pointer(1, fx + fw + 8.0 * i as f32, fy + fh + 6.0 * i as f32, 0, false, false);
        }
        ui.invoke_pointer(2, fx + fw + 40.0, fy + fh + 30.0, 0, false, false);
        let after = app.borrow().s.as_ref().and_then(|s| s.ed.doc.crop);
        r.check(
            "over the screen: the frame's corner drags (the crop grows)",
            matches!((before, after), (Some(b), Some(a)) if a.w > b.w && a.h > b.h && a.x == b.x && a.y == b.y),
            format!("{before:?} → {after:?}"),
        );
        ui.invoke_undo();
        let undone = app.borrow().s.as_ref().and_then(|s| s.ed.doc.crop);
        r.check(
            "over the screen: the frame change is one undo step",
            undone == before,
            format!("{undone:?}"),
        );
        r.snapshot(ui, "29-over-screen-marks");
        app.borrow_mut().over_to_window(ui);
    }));
    steps.push(Box::new(|app, ui, r| {
        let mut a = app.borrow_mut();
        let st =
            a.s.as_ref()
                .map(|s| (s.ed.doc.objects.len(), s.ed.doc.crop, ui.get_can_undo()));
        let flat = a.flatten();
        drop(a);
        let before = OVER_FLAT.with(|o| o.borrow().clone());
        r.check(
            "over the screen → window: same document, marks and undo",
            !ui.get_over_screen()
                && app.borrow().over.is_none()
                && ui.get_page() == 1
                && st.is_some_and(|(n, c, undo)| n == 1 && c.is_some() && undo),
            format!("{st:?}"),
        );
        // The marks were added after `before` was taken: compare the frame's size, and the
        // pixels once the mark is undone.
        ui.invoke_undo();
        let plain = app.borrow_mut().flatten();
        r.check(
            "over the screen → window: the pixels are the same",
            plain.is_some() && plain == before && flat.is_some(),
            format!(
                "{:?} vs {:?}",
                plain.as_ref().map(|f| (f.0, f.1)),
                before.as_ref().map(|f| (f.0, f.1))
            ),
        );
        r.snapshot(ui, "30-over-to-window");
        // Esc over the screen leaves nothing behind; Ctrl+S keeps it in the library.
        let count = |dir: &std::path::Path| {
            std::fs::read_dir(dir)
                .map(|d| {
                    d.filter_map(|e| e.ok())
                        .filter(|e| e.path().extension().is_some_and(|x| x == "znimok"))
                        .count()
                })
                .unwrap_or(0)
        };
        let raster = app
            .borrow()
            .s
            .as_ref()
            .map(|s| (*s.ed.doc.banks[0]).clone());
        let mut a = app.borrow_mut();
        a.save_now(ui);
        let dir = a.lib_dir.clone();
        a.close_document(ui);
        drop(a);
        let Some(raster) = raster else { return };
        let display = znimok_platform::Rect {
            x: 0,
            y: 0,
            width: raster.width,
            height: raster.height,
        };
        let n0 = count(&dir);
        let frame = znimok_core::IRect::new(10, 10, 200, 100);
        // ZK-107: each "over the screen" is an editor window of its own; the library window
        // (`lib`) opens it and stays where it is.
        let Some((lib, lib_ui)) = crate::wins::library() else {
            r.check("over the screen: the library window", false, String::new());
            return;
        };
        let page0 = lib_ui.get_page();
        lib.borrow_mut()
            .over_open(&lib_ui, raster.clone(), frame, "region", display);
        let (o1, o1_ui) = current(&lib, &lib_ui);
        let over1 = o1_ui.get_over_screen() && !Rc::ptr_eq(&o1, &lib);
        o1.borrow_mut().over_finish(&o1_ui, false, false);
        let n1 = count(&dir);
        lib.borrow_mut()
            .over_open(&lib_ui, raster.clone(), frame, "region", display);
        let (o2, o2_ui) = current(&lib, &lib_ui);
        o2.borrow_mut().over_finish(&o2_ui, false, true);
        let n2 = count(&dir);
        let back = !lib_ui.get_over_screen() && lib_ui.get_page() == page0;
        // What follows expects a document in the editor.
        lib.borrow_mut()
            .new_document(&lib_ui, raster, "region", None);
        r.check(
            "over the screen: Esc keeps nothing, Ctrl+S saves to the library",
            over1 && n1 == n0 && n2 == n0 + 1 && back,
            format!("own window {over1} · {n0} → {n1} → {n2} · library window stays: {back}"),
        );
        crate::pill::close();
    }));

    // ZK-126: after Copy the button shows a tick for a second, then the title comes back.
    steps.push(Box::new(|_app, ui, r| {
        ui.set_page(1);
        ui.invoke_copy();
        r.check("copy: tick shown", ui.get_copied(), String::new());
    }));
    steps.push(Box::new(|_app, ui, r| {
        r.check("copy: tick still there", ui.get_copied(), String::new());
        r.snapshot(ui, "copy-tick");
    }));
    // The tick holds 1 s; copying a large picture blocks the thread a while, so ~1.75 s of steps
    // before checking that the title is back (3200×2000 had only ~50 ms to spare).
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_app, ui, r| {
        r.check(
            "copy: title back after a second",
            !ui.get_copied(),
            String::new(),
        );
        r.snapshot(ui, "copy-title");
    }));

    // ZK-119: QR codes — from the editor (Image tab) and with Q in the capture overlay.
    steps.push(Box::new(|app, ui, _| {
        let Some((qw, qh, qr)) = znimok_codes::qr_rgba("https://example.org/znimok", 5) else {
            return;
        };
        // A light "screenshot" with the code in it.
        let (w, h) = (900u32, 600u32);
        let mut rgba = vec![240u8; (w * h * 4) as usize];
        for y in 0..qh {
            for x in 0..qw {
                let (si, di) = (
                    ((y * qw + x) * 4) as usize,
                    (((y + 150) * w + x + 300) * 4) as usize,
                );
                rgba[di..di + 4].copy_from_slice(&qr[si..si + 4]);
            }
        }
        let raster = znimok_core::Raster::new(w, h, rgba);
        QR_SCENE.with(|q| *q.borrow_mut() = Some(raster.clone()));
        app.borrow_mut().new_document(ui, raster, "region", None);
        let (_, ui) = opened(app, ui);
        ui.invoke_read_codes();
    }));
    // (the reading runs on a worker thread; one step later it has answered)
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, ui, r| {
        let codes = crate::codes::last();
        let link = codes.first().map(|c| c.kind.clone());
        r.check(
            "codes: the editor finds the QR link and asks what to do",
            codes.len() == 1
                && link
                    == Some(znimok_codes::Kind::Link(
                        "https://example.org/znimok".into(),
                    ))
                && ui.get_dialog_open()
                && slint::Model::row_count(&ui.get_dialog_buttons()) == 3,
            format!(
                "{codes:?} · dialog {} «{}»",
                ui.get_dialog_open(),
                ui.get_dialog_title()
            ),
        );
        r.snapshot(ui, "34-codes");
        // "Open link…" asks again with the whole address; Cancel opens nothing.
        ui.invoke_dialog_answer(1);
        let asked =
            ui.get_dialog_open() && ui.get_dialog_body().contains("https://example.org/znimok");
        ui.invoke_dialog_answer(1);
        r.check(
            "codes: a link opens only after a second question with the whole address",
            asked && !ui.get_dialog_open(),
            String::new(),
        );
        // Q in the capture overlay reads the frozen screen.
        crate::codes::clear_last();
        if let Some(raster) = QR_SCENE.with(|q| q.borrow().clone()) {
            let frozen = crate::capture::Frozen {
                displays: Vec::new(),
                bounds: znimok_platform::Rect {
                    x: 0,
                    y: 0,
                    width: raster.width,
                    height: raster.height,
                },
                windows: Vec::new(),
                raster,
            };
            if crate::overlay::open(frozen, true).is_ok()
                && let Some(ov) = crate::overlay::handle()
            {
                ov.invoke_key("q".into(), false, false);
            }
        }
    }));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, ui, r| {
        let codes = crate::codes::last();
        r.check(
            "codes: Q in the capture overlay reads the screen",
            !crate::overlay::is_open() && codes.len() == 1,
            format!("{codes:?}"),
        );
        if ui.get_dialog_open() {
            ui.invoke_dialog_answer(2);
        }
    }));

    // ZK-184: the text on the picture, read on the device. A clean page with real text in its
    // pixels (the marks are not read, only the picture): the panel, the lines on the canvas, a
    // click copies a line, a frame reads only a part, Esc closes.
    steps.push(Box::new(|app, ui, _| {
        let raster = text_page();
        app.borrow_mut().new_document(ui, raster, "text", None);
        let (_, ui) = opened(app, ui);
        ui.invoke_tool_chosen(0);
        ui.invoke_text_start();
    }));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|app, ui, r| {
        let lines = app.borrow().text_lines();
        let busy = app.borrow().text_busy();
        let all = ui.get_text_result().to_string();
        r.snapshot(ui, "39-text");
        r.check(
            "text: read on the device, lines on the canvas and in the panel",
            ui.get_text_open()
                && !busy
                && lines.len() == 2
                && all.contains("Znimok reads text 2026")
                && (!uk_reader() || all.contains("Привіт")),
            format!(
                "{} lines, busy {busy}: «{}»",
                lines.len(),
                all.replace('\n', " / ")
            ),
        );
        if let Some((text, rect)) = lines.first().cloned() {
            let at = app.borrow().doc_to_logical(
                rect.x as f64 + rect.w as f64 / 2.0,
                rect.y as f64 + rect.h as f64 / 2.0,
            );
            click(app, ui, at);
            let got = arboard::Clipboard::new()
                .and_then(|mut c| c.get_text())
                .unwrap_or_default();
            r.check(
                "text: a click on a line copies that line",
                got == text,
                format!("«{got}» vs «{text}»"),
            );
        }
        // A frame around the second line only: the next reading has just it.
        if let Some((_, rect)) = lines.get(1).cloned() {
            let (a0, a1) = {
                let a = app.borrow();
                (
                    a.doc_to_logical(rect.x as f64 - 10.0, rect.y as f64 - 10.0),
                    a.doc_to_logical(rect.right() as f64 + 10.0, rect.bottom() as f64 + 10.0),
                )
            };
            drag(app, ui, a0, a1);
        }
    }));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|app, ui, r| {
        let lines = app.borrow().text_lines();
        r.check(
            "text: a frame reads only that part",
            lines.len() == 1 && (!uk_reader() || lines[0].0.contains("Привіт")),
            format!("{lines:?}"),
        );
        key(app, ui, "\u{1b}", false, false);
        r.check(
            "text: Esc closes the panel",
            !ui.get_text_open() && app.borrow().text_lines().is_empty(),
            String::new(),
        );
    }));

    // ZK-185: X in the capture overlay — the text of the screen straight to the clipboard, with
    // a word about it; the page also goes to a PNG for the `znimok text` check.
    steps.push(Box::new(|_, ui, r| {
        if ui.get_dialog_open() {
            ui.invoke_dialog_answer(0);
        }
        let raster = text_page();
        let png = r.dir.join("text-page.png");
        let _ = image::RgbaImage::from_raw(raster.width, raster.height, raster.rgba.clone())
            .map(|i| i.save(&png));
        let frozen = crate::capture::Frozen {
            displays: Vec::new(),
            bounds: znimok_platform::Rect {
                x: 0,
                y: 0,
                width: raster.width,
                height: raster.height,
            },
            windows: Vec::new(),
            raster,
        };
        if crate::overlay::open(frozen, false).is_ok()
            && let Some(ov) = crate::overlay::handle()
        {
            ov.invoke_key("x".into(), false, false);
        }
    }));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, ui, r| {
        let copied = crate::text::last().unwrap_or_default();
        let clip = arboard::Clipboard::new()
            .and_then(|mut c| c.get_text())
            .unwrap_or_default();
        // The word about it is in the library window (the overlay has no editor behind it).
        let mut said = false;
        crate::wins::WeakCtx::LIBRARY.with(|_, lib| {
            said = lib.get_dialog_open();
            if said {
                lib.invoke_dialog_answer(0);
            }
        });
        r.check(
            "text: X in the capture overlay copies the screen's text and says so",
            !crate::overlay::is_open()
                && copied.contains("Znimok reads text 2026")
                && (!uk_reader() || copied.contains("Привіт"))
                && clip == copied
                && said,
            format!("«{}» · dialog {said}", copied.replace('\n', " / ")),
        );
        let _ = ui;
    }));

    // ZK-185: the hotkey / tray open the overlay in its text mode — releasing a frame reads it.
    steps.push(Box::new(|_, _, _| {
        let raster = text_page();
        let frozen = crate::capture::Frozen {
            displays: Vec::new(),
            bounds: znimok_platform::Rect {
                x: 0,
                y: 0,
                width: raster.width,
                height: raster.height,
            },
            windows: Vec::new(),
            raster,
        };
        crate::overlay::set_text_mode(true);
        if crate::overlay::open(frozen, false).is_ok()
            && let Some(ov) = crate::overlay::handle()
        {
            let osf = ov.window().scale_factor();
            let lw = ov.window().size().width as f32 / osf;
            let lh = ov.window().size().height as f32 / osf;
            let (kx, ky) = (1400.0 / lw.max(1.0), 420.0 / lh.max(1.0));
            // A frame round the second line only (frame pixels 40,200 → 1000,320).
            ov.invoke_pointer(0, 40.0 / kx, 200.0 / ky, false, false);
            for i in 1..=8 {
                let t = i as f32 / 8.0;
                ov.invoke_pointer(
                    1,
                    (40.0 + 960.0 * t) / kx,
                    (200.0 + 120.0 * t) / ky,
                    false,
                    false,
                );
            }
            ov.invoke_pointer(2, 1000.0 / kx, 320.0 / ky, false, false);
        }
    }));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, r| {
        let copied = crate::text::last().unwrap_or_default();
        crate::wins::WeakCtx::LIBRARY.with(|_, lib| {
            if lib.get_dialog_open() {
                lib.invoke_dialog_answer(0);
            }
        });
        r.check(
            "text: the text mode (hotkey, tray) reads the frame on release",
            !crate::overlay::is_open()
                && (!uk_reader() || copied.contains("Привіт"))
                && !copied.contains("Znimok"),
            format!("«{}»", copied.replace('\n', " / ")),
        );
    }));

    // ZK-192: a card opens in the window it was clicked in (the library and the editor are one
    // window); Alt+click opens a new window; a document open elsewhere asks, and «Open a copy»
    // gives a new document with a file of its own.
    steps.push(Box::new(|_, _, r| {
        // Only the library window: the editors of earlier steps go.
        for (a, w) in crate::wins::editors() {
            a.borrow_mut().close_document(&w);
        }
        let Some((lib, lw)) = crate::wins::library() else {
            r.check("open in place: the library window", false, String::new());
            return;
        };
        if lib.borrow().doc_path().is_some() {
            lib.borrow_mut().close_document(&lw);
        }
        let dir = lib.borrow().lib_dir.clone();
        let cards: Vec<std::path::PathBuf> = crate::library::scan(&dir, None)
            .into_iter()
            .map(|e| e.path)
            .collect();
        if cards.len() < 2 {
            r.check(
                "open in place: two cards",
                false,
                format!("{} cards", cards.len()),
            );
            return;
        }
        let before = crate::wins::editors().len();
        lib.borrow_mut().card_click(&lw, &cards[0], 0);
        let here = lib.borrow().doc_path() == Some(cards[0].clone())
            && crate::wins::editors().len() == before
            && lw.get_page() == 1;
        // ZK-196: «Бібліотека» in the top bar (the button itself) closes it and brings the grid.
        lw.invoke_back();
        let back = lib.borrow().doc_path().is_none() && lw.get_page() == 0;
        r.check(
            "open in place: «Library» in the top bar goes back to the grid",
            back,
            format!(
                "document {:?} · page {}",
                lib.borrow().doc_path(),
                lw.get_page()
            ),
        );
        if !back {
            lib.borrow_mut().close_document(&lw);
        }
        lib.borrow_mut().card_click(&lw, &cards[1], 3);
        let alt_new =
            crate::wins::editors().len() == before + 1 && lib.borrow().doc_path().is_none();
        r.check(
            "open in place: a card opens in this window, Alt+click in a new one",
            here && alt_new,
            format!("here {here} · Alt new window {alt_new}"),
        );
        // The second card is open in the new window: asked for again from the library, it asks.
        lib.borrow_mut().card_click(&lw, &cards[1], 0);
        let asked = lw.get_dialog_open();
        lw.invoke_dialog_answer(1);
        let copy = lib.borrow().doc_path();
        let copy_name = lib
            .borrow()
            .s
            .as_ref()
            .map(|s| s.ed.doc.name.clone())
            .unwrap_or_default();
        r.check(
            "open in place: a document open elsewhere asks; «Open a copy» is a new document",
            asked && copy.as_ref().is_some_and(|c| *c != cards[1]) && copy_name.contains("(копія)"),
            format!("asked {asked} · {copy:?} «{copy_name}»"),
        );
        lib.borrow_mut().close_document(&lw);
        for (a, w) in crate::wins::editors() {
            a.borrow_mut().close_document(&w);
        }
    }));

    // ZK-141: a scrolling capture over a synthetic page (a fake screen and a fake wheel): the
    // stitched document has the whole page, with the sticky header and footer once.
    steps.push(Box::new(|_, _, _| {
        let (w, page_h, view, head, foot) = (360u32, 1500u32, 420u32, 36u32, 28u32);
        let mut page = vec![250u8; (w * page_h * 4) as usize];
        let mut y = 6;
        let mut k = 1u32;
        while y + 12 < page_h {
            let len = 40 + (k * 97) % (w - 60);
            let shade = ((k * 53) % 140) as u8;
            for yy in y..y + 10 {
                for x in 8..8 + len {
                    if !(x + k).is_multiple_of(6) {
                        let i = ((yy * w + x) * 4) as usize;
                        page[i..i + 3].copy_from_slice(&[shade, shade, 200]);
                    }
                }
            }
            y += 18 + (k * 7) % 11;
            k += 1;
        }
        let band = |h: u32, c: u8| {
            let mut v = vec![0u8; (w * h * 4) as usize];
            for (i, p) in v.chunks_mut(4).enumerate() {
                let x = i as u32 % w;
                let g = if x % 40 < 12 { 255 } else { c };
                p.copy_from_slice(&[g, c, c / 2, 255]);
            }
            v
        };
        let (hb, fb) = (band(head, 60), band(foot, 120));
        let body = view - head - foot;
        let off = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let (o1, o2) = (off.clone(), off);
        let row = w as usize * 4;
        let grab = move || {
            let o = o1.load(std::sync::atomic::Ordering::SeqCst) as usize;
            let mut f = hb.clone();
            f.extend_from_slice(&page[o * row..(o + body as usize) * row]);
            f.extend_from_slice(&fb);
            Some(znimok_core::Raster::new(w, view, f))
        };
        let wheel = move || {
            let o = o2.load(std::sync::atomic::Ordering::SeqCst);
            o2.store(
                (o + 110).min(page_h - body),
                std::sync::atomic::Ordering::SeqCst,
            );
        };
        SCROLL_WANT.with(|s| s.set(head + page_h + foot));
        crate::scroll::start_with(
            grab,
            wheel,
            (40, 40),
            true,
            std::time::Duration::from_millis(15),
        );
    }));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|_, _, _| {}));
    steps.push(Box::new(|app, _, r| {
        let size = app.borrow().s.as_ref().map(|s| s.ed.doc.image_size());
        let want = SCROLL_WANT.with(|s| s.get());
        r.check(
            "scrolling capture: the stitched page is one document, header and footer once",
            !crate::scroll::active() && size == Some((360, want)),
            format!("{size:?}, want 360×{want}"),
        );
    }));

    // ZK-145: a video document opened in the app and saved again stays a video (its stream
    // intact); the library shows it with ▶ and its length.
    steps.push(Box::new(|app, ui, r| {
        let dir = app.borrow().lib_dir.clone();
        let path = dir.join("Znimok-selftest-video.znimok");
        let doc = znimok_core::Document::from_raster(
            String::from("Відео"),
            znimok_core::Raster::solid(160, 100, znimok_core::Rgb::BLUE),
        );
        let info = znimok_format::VideoInfo {
            width: 160,
            height: 100,
            fps_milli: 30_000,
            frames: 90,
            duration_hns: 30_000_000,
            codec: znimok_format::video::CODEC_H264,
        };
        let mp4: Vec<u8> = (0..4096u32).map(|i| (i * 13) as u8).collect();
        let bytes = znimok_format::write_video(
            &doc,
            &znimok_format::Video::new(info),
            &mp4,
            &znimok_format::WriteOptions::default(),
        );
        let written = std::fs::write(&path, bytes).is_ok();
        app.borrow_mut().open_path(ui, &path);
        let (app, ui) = &opened(app, ui);
        let opened_as_video = app.borrow().s.as_ref().is_some_and(|s| s.video.is_some());
        ui.invoke_meta_edited("title".into(), "Відео, перейменоване".into());
        let saved = app.borrow_mut().save_now(ui);
        let after = std::fs::read(&path).unwrap_or_default();
        let still = match znimok_format::read_any(&after) {
            Ok(znimok_format::Loaded::Video(v)) => {
                v.doc.name == "Відео, перейменоване" && v.payload.bytes(&after) == Some(mp4)
            }
            _ => false,
        };
        let card = crate::library::read_entry(&path).map(|e| e.meta_line());
        r.check(
            "video document: opened, renamed and saved — still a video, stream intact; ▶ on the card",
            written
                && opened_as_video
                && saved
                && still
                && card.as_deref().is_some_and(|m| m.starts_with("▶ 0:03")),
            format!("written {written} · video {opened_as_video} · saved {saved} · still {still} · {card:?}"),
        );
        // ZK-181: the «Відео» mode — transport, trimming on the timeline, one undo history with
        // the marks, the edits saved into the file. (The stream here is not decodable: the
        // poster stands for the video, as it does where there is no player yet.)
        let mode = (ui.get_vid_mode(), ui.get_insp_tab(), ui.get_vid_time().to_string(), ui.get_vid_total().to_string());
        ui.invoke_tl_layout(900.0);
        ui.invoke_vid_transport("fwd".into());
        ui.invoke_vid_transport("fwd".into());
        let stepped = ui.get_vid_time().to_string();
        // in at frame 15, out at frame 75 (the frame after End is not kept)
        {
            let mut a = app.borrow_mut();
            a.tl_pointer(ui, 0, 150, 10, false); // the ruler: frame 15 of 90 over 900 px
            a.tl_pointer(ui, 2, 150, 10, false);
        }
        ui.invoke_vid_action("in".into(), 0);
        {
            let mut a = app.borrow_mut();
            a.tl_pointer(ui, 0, 755, 10, false);
            a.tl_pointer(ui, 2, 755, 10, false);
        }
        ui.invoke_vid_action("out".into(), 0);
        let trimmed = app.borrow().s.as_ref().and_then(|s| s.ed.doc.timeline.clone());
        // a range on the strip: press at frame 30, drag to frame 59, release → Del cuts it
        {
            let mut a = app.borrow_mut();
            a.tl_pointer(ui, 0, 300, 50, false);
            a.tl_pointer(ui, 1, 450, 50, false);
            a.tl_pointer(ui, 1, 595, 50, false);
            a.tl_pointer(ui, 2, 595, 50, false);
        }
        let had_range = ui.get_tl_has_range();
        ui.invoke_vid_action("cut".into(), 0);
        let cut = app.borrow().s.as_ref().and_then(|s| s.ed.doc.timeline.clone());
        let left_after_cut = ui.get_vid_total().to_string();
        ui.invoke_undo();
        let undone = app.borrow().s.as_ref().and_then(|s| s.ed.doc.timeline.clone());
        ui.invoke_redo();
        // «Повернути» on the cut piece brings it back; «Скинути» forgets everything
        ui.invoke_vid_action("restore".into(), 400);
        let restored = app.borrow().s.as_ref().and_then(|s| s.ed.doc.timeline.clone());
        {
            // to frame 45 from the ruler, for the split, the trim and the screenshot below
            let mut a = app.borrow_mut();
            a.tl_pointer(ui, 0, 450, 10, false);
            a.tl_pointer(ui, 2, 450, 10, false);
        }
        ui.invoke_vid_action("split".into(), 0);
        ui.invoke_vid_action("reset".into(), 0);
        let reset = app.borrow().s.as_ref().and_then(|s| s.ed.doc.timeline.clone());
        // the trim again, saved into the file's CUTS
        ui.invoke_vid_action("in".into(), 0);
        let saved2 = app.borrow_mut().save_now(ui);
        let in_file = std::fs::read(&path)
            .ok()
            .and_then(|b| znimok_format::read_any(&b).ok())
            .and_then(|l| match l {
                znimok_format::Loaded::Video(v) => Some(v.video.edit.in_frame),
                _ => None,
            });
        // the frame as a screenshot: a new window with a document of the poster's size
        let windows = crate::wins::editors().len();
        ui.invoke_vid_action("frame-shot".into(), 0);
        let (shot_app, shot_ui) = current(app, ui);
        let shot = shot_app.borrow().s.as_ref().map(|s| (s.ed.doc.image_size(), s.video.is_none(), s.ed.doc.name.clone()));
        let one_more = crate::wins::editors().len() == windows + 1 && !Rc::ptr_eq(&shot_app, app);
        shot_app.borrow_mut().close_document(&shot_ui);
        let parts_of = |t: &Option<znimok_core::Timeline>| {
            t.as_ref().map(|t| (t.in_point, t.out_point, t.parts.iter().filter(|p| p.off).map(|p| (p.a, p.b)).collect::<Vec<_>>()))
        };
        r.check(
            "video mode: the «Відео» tab, the transport counts frames, in / out from the ruler",
            mode == (true, 2, "0:00.00".into(), "0:03.00".into())
                && stepped == "0:00.07"
                && parts_of(&trimmed) == Some((15, 76, vec![])),
            format!("{mode:?} · after 2 steps {stepped} · {:?}", parts_of(&trimmed)),
        );
        r.check(
            "video mode: a range on the strip is cut, undone, restored, reset; the trim is saved",
            had_range
                && parts_of(&cut) == Some((15, 76, vec![(30, 60)]))
                && left_after_cut == "0:01.03"
                && parts_of(&undone) == Some((15, 76, vec![]))
                && parts_of(&restored) == Some((15, 76, vec![]))
                && parts_of(&reset) == Some((0, 90, vec![]))
                && saved2
                && in_file == Some(45),
            format!(
                "range {had_range} · cut {:?} · left {left_after_cut} · undone {:?} · restored {:?} · reset {:?} · saved {saved2} · in the file {in_file:?}",
                parts_of(&cut), parts_of(&undone), parts_of(&restored), parts_of(&reset)
            ),
        );
        r.check(
            "video mode: «Кадр як знімок» opens the frame as a screenshot in a window of its own",
            one_more && matches!(&shot, Some(((160, 100), true, name)) if name.contains("кадр 45")),
            format!("one more window {one_more} · {shot:?}"),
        );
        // ZK-94: marks in time. A new mark shows three seconds from the frame (45 → the end, 90);
        // on frame 10 it is neither drawn nor picked; its bar on the marks track drags its time
        // (one undo step); the time is saved; «Кадр як знімок» takes the marks live on the frame.
        let (cx, cy) = centre(ui);
        app.borrow_mut().set_tool(ui, crate::app::tool::RECT);
        drag(app, ui, (cx - 40.0, cy - 25.0), (cx + 40.0, cy + 25.0));
        app.borrow_mut().set_tool(ui, crate::app::tool::SELECT);
        let span_of = |app: &Shared| {
            app.borrow().s.as_ref().and_then(|s| {
                let o = s.ed.doc.objects.last()?;
                s.ed.doc.timeline.as_ref()?.marks.get(&o.id).copied()
            })
        };
        let made = span_of(app);
        let bars = ui.get_tl_marks().row_count();
        {
            let mut a = app.borrow_mut();
            a.tl_pointer(ui, 0, 100, 10, false);
            a.tl_pointer(ui, 2, 100, 10, false);
        }
        let live_at_10 = app.borrow().s.as_ref().map(|s| {
            s.ed.doc.objects.iter().filter(|o| s.ed.doc.live(o)).count()
        });
        // Press on the bar (frame 60 = 600 px, the first lane), drag 100 px (10 frames) left.
        {
            let mut a = app.borrow_mut();
            a.tl_pointer(ui, 0, 600, 111, false);
            a.tl_pointer(ui, 1, 550, 111, false);
            a.tl_pointer(ui, 1, 500, 111, false);
            a.tl_pointer(ui, 2, 500, 111, false);
        }
        let moved = span_of(app);
        let selected = app.borrow().s.as_ref().map(|s| s.ed.selection().len());
        ui.invoke_undo();
        let undone_span = span_of(app);
        let saved3 = app.borrow_mut().save_now(ui);
        let spans_in_file = std::fs::read(&path)
            .ok()
            .and_then(|b| znimok_format::read_any(&b).ok())
            .and_then(|l| match l {
                znimok_format::Loaded::Video(v) => {
                    Some(v.video.mark_spans.values().copied().collect::<Vec<_>>())
                }
                _ => None,
            });
        {
            let mut a = app.borrow_mut();
            a.tl_pointer(ui, 0, 500, 10, false);
            a.tl_pointer(ui, 2, 500, 10, false);
        }
        ui.invoke_vid_action("frame-shot".into(), 0);
        let (shot2_app, shot2_ui) = current(app, ui);
        let shot_marks = shot2_app.borrow().s.as_ref().map(|s| s.ed.doc.objects.len());
        if !Rc::ptr_eq(&shot2_app, app) {
            shot2_app.borrow_mut().close_document(&shot2_ui);
        }
        r.check(
            "marks in time: a new mark shows three seconds from the frame, with a bar on the marks track",
            made == Some((45, 90)) && bars == 1,
            format!("{made:?} · {bars} bars"),
        );
        r.check(
            "marks in time: outside its time a mark is neither drawn nor picked",
            live_at_10 == Some(0),
            format!("live on frame 10: {live_at_10:?}"),
        );
        r.check(
            "marks in time: its bar drags its time, one undo step; the time is saved",
            moved == Some((35, 80))
                && selected == Some(1)
                && undone_span == Some((45, 90))
                && saved3
                && spans_in_file.as_deref() == Some(&[(45u32, 90u32)][..]),
            format!("moved {moved:?} · selected {selected:?} · undone {undone_span:?} · saved {saved3} · in the file {spans_in_file:?}"),
        );
        r.check(
            "marks in time: «Кадр як знімок» takes the marks live on that frame",
            shot_marks == Some(1),
            format!("{shot_marks:?}"),
        );
        app.borrow_mut().close_document(ui);
        let _ = std::fs::remove_file(&path);
    }));

    // ZK-181, a live check on Windows: ZNIMOK_SELFTEST_VIDEO=<file.mp4> wraps the recording into
    // a video document, opens it, plays a moment and expects decoded frames to reach the canvas.
    #[cfg(windows)]
    if let Some(mp4) = std::env::var_os("ZNIMOK_SELFTEST_VIDEO").map(PathBuf::from) {
        steps.push(Box::new(move |app, ui, r| {
            use znimok_video::traits::VideoDecoder;
            let dir = app.borrow().lib_dir.clone();
            let path = dir.join("Znimok-selftest-real-video.znimok");
            let info = match znimok_video_win::MfDecoder::open(&mp4) {
                Ok(d) => d.info().clone(),
                Err(e) => {
                    r.check("real video: the file decodes", false, e.to_string());
                    return;
                }
            };
            let fps = if info.fps > 0.0 { info.fps } else { 30.0 };
            let frames = ((info.duration_hns as f64 / 10_000_000.0) * fps)
                .round()
                .max(1.0) as u32;
            let poster = znimok_core::Raster::solid(
                info.width,
                info.height,
                znimok_core::Rgb::new(40, 40, 40),
            );
            let doc = znimok_core::Document::from_raster(String::from("Справжній запис"), poster);
            let vinfo = znimok_format::VideoInfo {
                width: info.width,
                height: info.height,
                fps_milli: (fps * 1000.0).round() as u32,
                frames,
                duration_hns: info.duration_hns,
                codec: znimok_format::video::CODEC_H264,
            };
            let bytes = std::fs::read(&mp4).unwrap_or_default();
            let out = znimok_format::write_video(
                &doc,
                &znimok_format::Video::new(vinfo),
                &bytes,
                &znimok_format::WriteOptions::default(),
            );
            let written = std::fs::write(&path, out).is_ok();
            app.borrow_mut().open_path(ui, &path);
            let (_, ui) = &opened(app, ui);
            let mode = ui.get_vid_mode();
            r.check(
                "real video: wrapped into a document and opened in the «Відео» mode",
                written && mode,
                format!(
                    "written {written} · {}×{} {fps} fps {frames} frames · mode {mode}",
                    info.width, info.height
                ),
            );
            REAL_VIDEO.with(|v| *v.borrow_mut() = Some(path));
        }));
        for _ in 0..3 {
            steps.push(Box::new(|_, _, _| {}));
        }
        steps.push(Box::new(|app, ui, _| {
            let (_, ui) = &current(app, ui);
            ui.invoke_vid_transport("play".into());
        }));
        for _ in 0..4 {
            steps.push(Box::new(|_, _, _| {}));
        }
        // Playing: the frames come from the GPU (the player's texture under a see-through canvas).
        steps.push(Box::new(|app, ui, r| {
            let (app, ui) = &current(app, ui);
            let (frame, playing, live, path, picture) = {
                let a = app.borrow();
                let v = a.s.as_ref().and_then(|s| s.vid.as_ref());
                (
                    v.map_or(0, |v| v.frame),
                    v.is_some_and(|v| v.playing),
                    v.is_some_and(|v| v.shown.is_some()),
                    v.and_then(|v| v.path),
                    a.picture_drawn(),
                )
            };
            r.check(
                "real video: frames are decoded on the GPU and shown while playing (ZK-92)",
                live && frame > 5 && ui.get_vid_live() && !picture && path.is_some(),
                format!(
                    "frame {frame} · playing {playing} · on screen {live} · path {path:?} · canvas draws the picture {picture}"
                ),
            );
            r.snapshot(ui, "40-video-playing");
            ui.invoke_vid_transport("play".into());
        }));
        // Paused: a CPU copy of the frame arrives a moment later.
        for _ in 0..6 {
            steps.push(Box::new(|_, _, _| {}));
        }
        steps.push(Box::new(|app, ui, r| {
            let (app, ui) = &current(app, ui);
            let (frame, copy, lit, thumbs) = {
                let a = app.borrow();
                let v = a.s.as_ref().and_then(|s| s.vid.as_ref());
                let copy = v.and_then(|v| v.raster.clone());
                let lit = copy
                    .as_ref()
                    .map(|(_, r)| {
                        r.rgba
                            .chunks(4)
                            .filter(|p| p[0] as u32 + p[1] as u32 + p[2] as u32 > 60)
                            .count()
                    })
                    .unwrap_or(0);
                (
                    v.map_or(0, |v| v.frame),
                    copy.map(|(f, _)| f),
                    lit,
                    v.map_or(0, |v| v.thumbs.len()),
                )
            };
            r.check(
                "real video: paused, the frame shown comes back to the CPU; the film strip has thumbnails",
                copy == Some(frame) && thumbs > 0,
                format!("frame {frame} · copy of {copy:?} · {lit} lit pixels · {thumbs} thumbnails"),
            );
            // A mark on the paused frame, its bar on the marks track (ZK-94).
            let (cx, cy) = centre(ui);
            app.borrow_mut().set_tool(ui, crate::app::tool::RECT);
            drag(app, ui, (cx - 120.0, cy - 60.0), (cx + 60.0, cy + 40.0));
            app.borrow_mut().set_tool(ui, crate::app::tool::SELECT);
            r.snapshot(ui, "42-video-marks");
            // Backwards from here (J).
            ui.invoke_vid_transport("reverse".into());
            REAL_FROM.with(|f| f.set(frame));
        }));
        for _ in 0..4 {
            steps.push(Box::new(|_, _, _| {}));
        }
        steps.push(Box::new(|app, ui, r| {
            let (app, ui) = &current(app, ui);
            let from = REAL_FROM.with(|f| f.get());
            let (frame, playing, backward) = {
                let a = app.borrow();
                let v = a.s.as_ref().and_then(|s| s.vid.as_ref());
                (
                    v.map_or(0, |v| v.frame),
                    v.is_some_and(|v| v.playing),
                    v.is_some_and(|v| v.backward),
                )
            };
            r.check(
                "real video: J plays backwards",
                (frame < from || from == 0) && backward,
                format!("from {from} to {frame} · playing {playing} · backward {backward}"),
            );
            ui.invoke_vid_transport("pause".into());
        }));
        for _ in 0..6 {
            steps.push(Box::new(|_, _, _| {}));
        }
        steps.push(Box::new(|app, ui, r| {
            let (app, ui) = &current(app, ui);
            // Trim on a real stream, then a frame as a screenshot with the decoded pixels.
            ui.invoke_vid_action("in".into(), 0);
            ui.invoke_vid_action("frame-shot".into(), 0);
            let lit = {
                let a = app.borrow();
                a.s.as_ref()
                    .and_then(|s| s.vid.as_ref())
                    .and_then(|v| v.raster.as_ref().filter(|(f, _)| *f == v.frame))
                    .map(|(_, r)| {
                        r.rgba
                            .chunks(4)
                            .filter(|p| p[0] as u32 + p[1] as u32 + p[2] as u32 > 60)
                            .count()
                    })
            };
            let (shot_app, shot_ui) = current(app, ui);
            let shot_lit = shot_app.borrow().s.as_ref().map(|s| {
                s.ed.doc
                    .source()
                    .rgba
                    .chunks(4)
                    .filter(|p| p[0] as u32 + p[1] as u32 + p[2] as u32 > 60)
                    .count()
            });
            let lit = lit.unwrap_or(usize::MAX);
            r.check(
                "real video: the frame as a screenshot carries the decoded pixels",
                shot_lit.is_some_and(|n| n > 0) && shot_lit == Some(lit),
                format!("{shot_lit:?} vs {lit}"),
            );
            shot_app.borrow_mut().close_document(&shot_ui);
            app.borrow_mut().close_document(ui);
            if let Some(p) = REAL_VIDEO.with(|v| v.borrow_mut().take()) {
                let _ = std::fs::remove_file(p);
            }
        }));
    }

    // ZK-180/91: recording in the app — a window of Znimok itself (WGC sees windows even where the
    // screen is out of reach, as under RDP): the edges and the bar show around it, the time runs,
    // pause holds it; Stop wraps the MP4 into a video document in the library that opens in the
    // «Відео» mode.
    #[cfg(windows)]
    steps.push(Box::new(|_, ui, r| {
        // Another program's window: VS Code where the self-test runs from it; else this window
        // itself — WGC sends an in-process window no frames, and since ZK-193 the recording
        // takes its picture with PrintWindow and repeats it.
        let hwnd = {
            use znimok_platform::WindowList;
            let cap = znimok_win::WinCapture::new();
            cap.windows()
                .unwrap_or_default()
                .into_iter()
                .find(|w| w.title.contains("Visual Studio Code"))
                .map(|w| w.id.0)
        }
        .or_else(|| {
            use slint::winit_030::WinitWindowAccessor;
            use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
            ui.window()
                .with_winit_window(|w| match w.window_handle().ok()?.as_raw() {
                    RawWindowHandle::Win32(h) => Some(h.hwnd.get() as u64),
                    _ => None,
                })
                .flatten()
        });
        let Some(hwnd) = hwnd else {
            r.check(
                "recording: a window to record",
                true,
                "no VS Code window — skipped".into(),
            );
            return;
        };
        let frame =
            znimok_win::raw::dwm_bounds(znimok_win::raw::hwnd(znimok_platform::WindowId(hwnd)));
        let display = znimok_win::raw::monitors().first().map(|m| m.info.bounds);
        // With the system sound (ZK-89): a track of its own in the document.
        ui.invoke_setting("rec-sound".into(), 1);
        match (frame, display) {
            (Some(frame), Some(display)) => crate::rec::start(crate::rec::Choice {
                display,
                frame,
                window: Some(hwnd),
                source: "window",
            }),
            _ => r.check(
                "recording: the library window's place",
                false,
                format!("{frame:?}"),
            ),
        }
        let ind = crate::rec::indicators();
        r.check(
            "recording: started on a window; four edges around it and the bar",
            crate::rec::is_recording() && ind.is_some_and(|(_, edges, bar)| edges == 4 && bar),
            format!("{ind:?}"),
        );
    }));
    // The window changes while it is recorded (WGC sends a frame when the window is presented).
    #[cfg(windows)]
    for i in 0..6 {
        steps.push(Box::new(move |_, ui, _| {
            ui.set_toast(format!("запис {i}").into())
        }));
    }
    #[cfg(windows)]
    steps.push(Box::new(|_, _, r| {
        let t = crate::rec::bar_time();
        crate::rec::toggle_pause();
        let paused_at = crate::rec::bar_time();
        r.check(
            "recording: the time runs on the bar; pause holds it",
            t.as_deref().is_some_and(|t| t != "0:00") && paused_at == t,
            format!("{t:?} → paused {paused_at:?}"),
        );
    }));
    #[cfg(windows)]
    for i in 0..3 {
        steps.push(Box::new(move |_, ui, _| {
            ui.set_toast(format!("пауза {i}").into())
        }));
    }
    #[cfg(windows)]
    steps.push(Box::new(|_, _, r| {
        let held = crate::rec::bar_time();
        crate::rec::toggle_pause();
        crate::rec::stop();
        r.check(
            "recording: the time stood still while paused; stop hides the indicators",
            held.is_some() && !crate::rec::is_recording() && crate::rec::indicators().is_none(),
            format!("{held:?}"),
        );
    }));
    // The file is finished and wrapped on a worker thread.
    #[cfg(windows)]
    for _ in 0..8 {
        steps.push(Box::new(|_, _, _| {}));
    }
    #[cfg(windows)]
    steps.push(Box::new(|app, ui, r| {
        let path = crate::rec::LAST_SAVED.with(|l| l.borrow_mut().take());
        let doc = path
            .as_ref()
            .and_then(|p| znimok_format::open(p).ok())
            .and_then(|l| match l {
                znimok_format::Loaded::Video(v) => Some((
                    v.video.info.width,
                    v.video.info.height,
                    v.video.info.frames,
                    v.doc.meta.source.clone(),
                    v.video
                        .audio
                        .iter()
                        .map(|t| (t.source, t.label.clone()))
                        .collect::<Vec<_>>(),
                )),
                _ => None,
            });
        let card = path
            .as_deref()
            .and_then(crate::library::read_entry)
            .map(|e| e.meta_line());
        // WGC sends a window's frame only when it is drawn anew; a window that stands still
        // gets its first frame from PrintWindow (ZK-193), so the recording is never empty.
        let frames = crate::rec::LAST_FRAMES.load(std::sync::atomic::Ordering::SeqCst);
        if frames == 0 {
            let said = crate::wins::library().map(|(_, u)| u.get_toast().to_string());
            r.check(
                "recording: a still window is recorded too (ZK-193)",
                false,
                format!("0 frames · {said:?}"),
            );
            return;
        }
        r.check(
            "recording: stop leaves a video document in the library (▶ on the card)",
            doc.as_ref()
                .is_some_and(|(w, h, n, s, _)| *w > 0 && *h > 0 && *n >= 20 && s == "window")
                && card.as_deref().is_some_and(|m| m.starts_with('▶'))
                && crate::pill::is_open(),
            format!(
                "{doc:?} · {card:?} · card after capture {} · {frames} frames",
                crate::pill::is_open()
            ),
        );
        // ZK-89: the system sound is a track of the document, named after its device (where
        // this machine has an output device at all).
        let outputs = znimok_video_win::audio_devices(znimok_video::traits::AudioKind::System);
        let tracks = doc.as_ref().map(|d| d.4.clone()).unwrap_or_default();
        r.check(
            "recording: the system sound is a track of its own, named after the device",
            outputs.is_empty()
                || (tracks.len() == 1
                    && tracks[0].0 == znimok_format::video::AudioSource::System
                    && !tracks[0].1.is_empty()),
            format!("{tracks:?} · outputs {}", outputs.len()),
        );
        ui.invoke_setting("rec-sound".into(), 0);
        // ZK-100: the library shows all, only screenshots or only videos; the choice is kept.
        if let Some((lib, lw)) = crate::wins::library() {
            use slint::Model;
            let videos = lib.borrow().entries.iter().filter(|e| e.video_ms.is_some()).count();
            let shots = lib.borrow().entries.len() - videos;
            let count = |what: &str| {
                lw.invoke_lib_action(what.into(), "".into());
                lw.get_cards().row_count()
            };
            let (v, s, a) = (count("kind-videos"), count("kind-shots"), count("kind-all"));
            r.check(
                "library: All / Shots / Videos show what they say",
                videos >= 1 && v == videos && s == shots && a == videos + shots && lw.get_lib_kind() == 0,
                format!("videos {v}/{videos} · shots {s}/{shots} · all {a}"),
            );
        }
        crate::pill::close();
        if let Some(p) = path {
            app.borrow_mut().open_path(ui, &p);
            let (va, vui) = opened(app, ui);
            let playing_ok = va
                .borrow()
                .s
                .as_ref()
                .and_then(|s| s.vid.as_ref())
                .is_some_and(|v| v.player.is_some() || !v.player_started);
            r.check(
                "recording: the new video opens in the «Відео» mode with a player (or one waiting for the window's GPU)",
                vui.get_vid_mode() && playing_ok,
                format!("mode {} · player {playing_ok}", vui.get_vid_mode()),
            );
            va.borrow_mut().close_document(&vui);
            let _ = std::fs::remove_file(&p);
        }
    }));

    // ZK-180: the recording overlay — a region dragged on the frozen first display becomes the
    // recorded part (desktop units = the display's origin + frame pixels). Under RDP the screen
    // may be out of reach: then the start fails with a message, never silently.
    #[cfg(windows)]
    steps.push(Box::new(|_, _, r| {
        let Some(m) = znimok_win::raw::monitors().into_iter().next() else {
            r.check(
                "recording overlay: a display",
                true,
                "none — skipped".into(),
            );
            return;
        };
        let b = m.info.bounds;
        let frozen = crate::capture::Frozen {
            displays: Vec::new(),
            bounds: b,
            windows: Vec::new(),
            raster: znimok_core::Raster::solid(
                b.width,
                b.height,
                znimok_core::Rgb::new(90, 90, 100),
            ),
        };
        crate::rec::set_video_mode(true);
        if crate::overlay::open(frozen, false).is_ok()
            && let Some(ov) = crate::overlay::handle()
        {
            let osf = ov.window().scale_factor();
            let lw = ov.window().size().width as f32 / osf;
            let lh = ov.window().size().height as f32 / osf;
            let (kx, ky) = (b.width as f32 / lw.max(1.0), b.height as f32 / lh.max(1.0));
            // Frame pixels 200,150 → 840,510: a 640 × 360 region.
            ov.invoke_pointer(0, 200.0 / kx, 150.0 / ky, false, false);
            for i in 1..=8 {
                let t = i as f32 / 8.0;
                ov.invoke_pointer(
                    1,
                    (200.0 + 640.0 * t) / kx,
                    (150.0 + 360.0 * t) / ky,
                    false,
                    false,
                );
            }
            ov.invoke_pointer(2, 840.0 / kx, 510.0 / ky, false, false);
        }
        OVERLAY_DISPLAY.with(|d| d.set(Some((b.x, b.y))));
    }));
    #[cfg(windows)]
    steps.push(Box::new(|_, _, _| {}));
    #[cfg(windows)]
    steps.push(Box::new(|_, _, r| {
        let Some((x0, y0)) = OVERLAY_DISPLAY.with(|d| d.take()) else {
            return;
        };
        let ind = crate::rec::indicators();
        let said = crate::wins::library().map(|(_, u)| u.get_toast().to_string());
        let ok = match ind {
            // Started: the recorded part is where the region was dragged (±1 px of rounding).
            Some((f, edges, bar)) => {
                (f.x - (x0 + 200)).abs() <= 1
                    && (f.y - (y0 + 150)).abs() <= 1
                    && (f.width as i32 - 640).abs() <= 2
                    && (f.height as i32 - 360).abs() <= 2
                    && edges == 4
                    && bar
            }
            // The screen out of reach: a message says why.
            None => said.as_deref().is_some_and(|t| !t.is_empty()),
        };
        r.check(
            "recording overlay: the dragged region is recorded (or the start says why it cannot)",
            ok && !crate::overlay::is_open(),
            format!("{ind:?} · {said:?}"),
        );
        crate::rec::with_bar(|w| r.snapshot_window(w, "41-rec-bar"));
        crate::rec::stop();
    }));
    #[cfg(windows)]
    for _ in 0..6 {
        steps.push(Box::new(|_, _, _| {}));
    }
    #[cfg(windows)]
    steps.push(Box::new(|_, _, _| {
        // A black RDP screen still records: that document goes, and the card with it.
        if let Some(p) = crate::rec::LAST_SAVED.with(|l| l.borrow_mut().take()) {
            let _ = std::fs::remove_file(p);
        }
        crate::pill::close();
    }));

    // ZK-107: one window per document — a second document opens in a window of its own, marks
    // do not mix, a PNG from one dropped on the other becomes a mark there, opening a document
    // that is open again raises its window instead of a copy, closing takes the window away.
    steps.push(Box::new(|app, ui, r| {
        let n1 = count(app);
        // Windows opened by the steps before stay open (each document keeps its own).
        let before = crate::wins::editors().len();
        let path1 = app.borrow().s.as_ref().map(|s| s.path.clone());
        let name1 = app.borrow().doc_name();
        let png = r.dir.join("zk107-from-window-1.png");
        app.borrow_mut().export_to(ui, &png);
        let (w, h) = (320u32, 200u32);
        let raster = znimok_core::Raster::new(w, h, vec![200; (w * h * 4) as usize]);
        app.borrow_mut().new_document(ui, raster, "region", None);
        let (app2, ui2) = opened(app, ui);
        let two = crate::wins::editors().len() == before + 1 && !Rc::ptr_eq(app, &app2);
        let n2 = count(&app2);
        let dropped = png.exists() && app2.borrow_mut().drop_image_mark(&ui2, &png);
        let title1 = ui.get_window_title().to_string();
        let title2 = ui2.get_window_title().to_string();
        let titled = title1 == format!("{name1} — Znimok")
            && title2 == format!("{} — Znimok", app2.borrow().doc_name())
            && title1 != title2;
        r.check(
            "two documents, two windows: marks stay apart, a PNG dropped from one is a mark in the other",
            two && dropped
                && count(&app2) == n2 + 1
                && count(app) == n1
                && ui.get_page() == 1
                && ui2.get_page() == 1
                && titled,
            format!(
                "windows {} · dropped {dropped} · {n2}→{} / {n1}→{} · «{title1}» / «{title2}»",
                crate::wins::editors().len(),
                count(&app2),
                count(app)
            ),
        );
        // The first document again: its window, not a third one.
        if let Some(p) = path1
            && let Some((lib, lib_ui)) = crate::wins::library()
        {
            lib.borrow_mut().open_path(&lib_ui, &p);
        }
        r.check(
            "opening an open document raises its window, no copy",
            crate::wins::editors().len() == before + 1,
            format!("{} windows", crate::wins::editors().len()),
        );
        app2.borrow_mut().close_document(&ui2);
        let _ = std::fs::remove_file(&png);
        WINDOWS_BEFORE.with(|w| w.set(before));
    }));
    steps.push(Box::new(|app, _, r| {
        let n = crate::wins::editors().len();
        let before = WINDOWS_BEFORE.with(|w| w.get());
        let names: Vec<Option<String>> = crate::wins::editors()
            .iter()
            .map(|(a, _)| {
                a.try_borrow()
                    .ok()
                    .and_then(|a| a.s.as_ref().map(|s| s.ed.doc.name.clone()))
            })
            .collect();
        r.check(
            "a closed document's window is gone; the first is current again",
            n == before && app.borrow().s.is_some(),
            format!("{n} windows, {before} before · {names:?}"),
        );
    }));

    // ZK-132: the Agents and Updates pages; the MCP switch is the one the server checks.
    steps.push(Box::new(|_, ui, _| {
        ui.invoke_settings_open();
        ui.set_settings_page(8);
    }));
    steps.push(Box::new(|_, ui, r| {
        r.snapshot(ui, "32-agents");
        ui.invoke_setting("mcp".into(), 1);
        let file = std::fs::read_to_string(r.dir.join("settings.json")).unwrap_or_default();
        let on = file.contains("\"mcp_enabled\": true") && ui.get_pref_mcp();
        ui.invoke_setting("mcp".into(), 0);
        let file = std::fs::read_to_string(r.dir.join("settings.json")).unwrap_or_default();
        let off = !file.contains("\"mcp_enabled\": true") && !ui.get_pref_mcp();
        r.check(
            "agents: the MCP switch goes into settings.json",
            on && off,
            String::new(),
        );
        ui.set_settings_page(9);
    }));
    steps.push(Box::new(|_, ui, r| {
        r.snapshot(ui, "33-updates");
        // «Перевірити зараз»: the answer is checked in the steps below.
        ui.invoke_setting("upd-check".into(), 0);
        r.check(
            "updates page: version and last check shown",
            !ui.get_upd_version().is_empty() && !ui.get_upd_last().is_empty(),
            format!("{} · {}", ui.get_upd_version(), ui.get_upd_last()),
        );
    }));
    // The answer comes from a worker thread: without the release key at once, with it from
    // GitHub (up to date, a release, or a network error offline) — wait up to ~20 s for it.
    let answered = Rc::new(std::cell::Cell::new(false));
    const WAIT_STEPS: usize = 60;
    for n in 0..WAIT_STEPS {
        let answered = answered.clone();
        steps.push(Box::new(move |app, ui, r| {
            if answered.get() {
                return;
            }
            let a = app.borrow();
            let checking = a.tr.tr("upd-checking");
            let status = ui.get_upd_status().to_string();
            let done = !ui.get_upd_busy() && !status.is_empty() && status != checking;
            if !done && n + 1 < WAIT_STEPS {
                return;
            }
            answered.set(true);
            // A release on offer is the only case with an install button (Windows).
            let offer = ui.get_upd_can_install() || ui.get_upd_can_open();
            let not_set_up = status == a.tr.tr("upd-not-configured");
            r.check(
                "updates: «check now» answers (not set up / up to date / a release / offline); install only with a release",
                done && (!ui.get_upd_can_install() || (offer && !not_set_up)),
                status,
            );
            ui.set_settings_page(6);
        }));
    }
    steps.push(Box::new(|_, ui, r| {
        r.snapshot(ui, "35-about");
        // ZK-198: the version and the build (commit, platform) are on the page.
        let (version, build) = (ui.get_pref_version(), ui.get_pref_build());
        r.check(
            "about: the version and the build are shown",
            version.contains(env!("CARGO_PKG_VERSION")) && build.contains(std::env::consts::ARCH),
            format!("{version} · {build}"),
        );
        ui.invoke_setting("close".into(), 0);
    }));

    // ZK-139: two displays side by side, the right one twice as dense (like Retina next to a
    // plain screen): a window on each, one frame under them, a region dragged across the seam.
    steps.push(Box::new(|_, _, r| {
        let solid = |w: u32, h: u32, c: [u8; 3]| {
            let mut v = vec![0u8; (w * h * 4) as usize];
            for p in v.chunks_mut(4) {
                p.copy_from_slice(&[c[0], c[1], c[2], 255]);
            }
            znimok_core::Raster::new(w, h, v)
        };
        let a = (
            znimok_platform::Rect::new(0, 0, 800, 500),
            solid(800, 500, [200, 40, 40]),
        );
        let b = (
            znimok_platform::Rect::new(800, 0, 800, 500),
            solid(1600, 1000, [40, 40, 200]),
        );
        let (raster, union, displays) = crate::capture::compose(&[a, b]);
        r.check(
            "two displays: one frame in the denser grid",
            (raster.width, raster.height) == (3200, 1000)
                && union == znimok_platform::Rect::new(0, 0, 1600, 500)
                && displays.len() == 2
                && displays[1].rect.x == 1600,
            format!(
                "{}×{} · {union:?} · {displays:?}",
                raster.width, raster.height
            ),
        );
        let frozen = crate::capture::Frozen {
            raster,
            bounds: union,
            windows: Vec::new(),
            displays,
        };
        let _ = crate::overlay::open(frozen, true);
        let windows = crate::overlay::window_count();
        if let Some(ov) = crate::overlay::handle() {
            // Logical pixels of the left window: frame pixels ÷ its own scale.
            let sf = ov.window().scale_factor();
            let lw = ov.window().size().width as f32 / sf;
            let k = 1600.0 / lw.max(1.0);
            ov.invoke_pointer(0, 1200.0 / k, 400.0 / k, false, false);
            for i in 1..=8 {
                let t = i as f32 / 8.0;
                ov.invoke_pointer(
                    1,
                    (1200.0 + 800.0 * t) / k,
                    (400.0 + 200.0 * t) / k,
                    false,
                    false,
                );
            }
            ov.invoke_pointer(2, 2000.0 / k, 600.0 / k, false, false);
        }
        MULTI_WINDOWS.with(|m| m.set(windows));
    }));
    steps.push(Box::new(|app, _, r| {
        let a = app.borrow();
        let doc =
            a.s.as_ref()
                .map(|s| (s.ed.doc.image_size(), s.ed.doc.banks[0].clone()));
        drop(a);
        let ok = doc.as_ref().is_some_and(|((w, h), bank)| {
            let (w, h) = (*w as i32, *h as i32);
            // Left half red (from the plain display, enlarged), right half blue.
            let px = |x: u32| {
                let i = ((h as u32 / 2 * bank.width + x) * 4) as usize;
                [bank.rgba[i], bank.rgba[i + 1], bank.rgba[i + 2]]
            };
            (w - 800).abs() <= 3
                && (h - 200).abs() <= 3
                && px(10) == [200, 40, 40]
                && px(bank.width - 10) == [40, 40, 200]
        });
        r.check(
            "two displays: a window on each, a region across the seam has both",
            MULTI_WINDOWS.with(|m| m.get()) == 2 && ok,
            format!(
                "{} windows · {:?}",
                MULTI_WINDOWS.with(|m| m.get()),
                doc.map(|d| d.0)
            ),
        );
    }));

    // ZK-128: a second click takes the shot after a countdown; Esc cancels the countdown.
    steps.push(Box::new(|_, _, _| {
        let open = || -> Option<(crate::Overlay, f32, f32)> {
            let raster = OVER_SRC.with(|o| o.borrow().clone())?;
            let frozen = crate::capture::Frozen {
                displays: Vec::new(),
                bounds: znimok_platform::Rect {
                    x: 0,
                    y: 0,
                    width: raster.width,
                    height: raster.height,
                },
                windows: vec![crate::capture::FrozenWindow {
                    rect: crate::capture::PxRect {
                        x: 100,
                        y: 100,
                        w: 400,
                        h: 300,
                    },
                    title: "test window".into(),
                    id: 0,
                }],
                raster,
            };
            let (fw, fh) = (frozen.raster.width as f32, frozen.raster.height as f32);
            crate::overlay::open(frozen, false).ok()?;
            let ov = crate::overlay::handle()?;
            let sf = ov.window().scale_factor();
            let lw = ov.window().size().width as f32 / sf;
            let lh = ov.window().size().height as f32 / sf;
            Some((ov, fw / lw.max(1.0), fh / lh.max(1.0)))
        };
        // Double click on the test window.
        let double = open().map(|(ov, kx, ky)| {
            let (x, y) = (300.0 / kx, 250.0 / ky);
            ov.invoke_pointer(1, x, y, false, false);
            for kind in [0, 2, 0, 2] {
                ov.invoke_pointer(kind, x, y, false, false);
            }
            !crate::overlay::is_open() && crate::overlay::countdown_open()
        });
        DOUBLE_OK.with(|d| d.set(double == Some(true)));
    }));
    // The countdown window has drawn by now: its picture, then Esc.
    steps.push(Box::new(|_, _, r| {
        if let Some(c) = crate::overlay::countdown_window() {
            r.snapshot_window(c.window(), "31-countdown");
        }
        crate::overlay::escape();
        let double = Some(DOUBLE_OK.with(|d| d.get()) && !crate::overlay::countdown_open());
        let open = || -> Option<(crate::Overlay, f32, f32)> {
            let raster = OVER_SRC.with(|o| o.borrow().clone())?;
            let frozen = crate::capture::Frozen {
                displays: Vec::new(),
                bounds: znimok_platform::Rect {
                    x: 0,
                    y: 0,
                    width: raster.width,
                    height: raster.height,
                },
                windows: Vec::new(),
                raster,
            };
            let (fw, fh) = (frozen.raster.width as f32, frozen.raster.height as f32);
            crate::overlay::open(frozen, false).ok()?;
            let ov = crate::overlay::handle()?;
            let sf = ov.window().scale_factor();
            let lw = ov.window().size().width as f32 / sf;
            let lh = ov.window().size().height as f32 / sf;
            Some((ov, fw / lw.max(1.0), fh / lh.max(1.0)))
        };
        // A click, then at once a drag: the region, after the countdown.
        let drag = open().map(|(ov, kx, ky)| {
            ov.invoke_pointer(0, 600.0 / kx, 500.0 / ky, false, false);
            ov.invoke_pointer(2, 600.0 / kx, 500.0 / ky, false, false);
            ov.invoke_pointer(0, 600.0 / kx, 500.0 / ky, false, false);
            for i in 1..=6 {
                let t = i as f32 / 6.0;
                ov.invoke_pointer(
                    1,
                    (600.0 + 300.0 * t) / kx,
                    (500.0 + 200.0 * t) / ky,
                    false,
                    false,
                );
            }
            ov.invoke_pointer(2, 900.0 / kx, 700.0 / ky, false, false);
            let started = !crate::overlay::is_open() && crate::overlay::countdown_open();
            crate::overlay::escape();
            started && !crate::overlay::countdown_open()
        });
        r.check(
            "overlay: double click / click-then-drag start the countdown, Esc cancels it",
            double == Some(true) && drag == Some(true),
            format!("double {double:?} · click+drag {drag:?}"),
        );
    }));

    // ZK-117 / ZK-46: the main screens once more in the light theme, and text contrast in both
    // (WCAG: main text at least 7:1 on panels, secondary at least 4.5:1).
    steps.push(Box::new(|_, ui, r| {
        let theme = ui.global::<crate::Theme>();
        let contrast = |a: slint::Color, b: slint::Color| -> f64 {
            let lum = |c: slint::Color| {
                let f = |v: u8| {
                    let v = v as f64 / 255.0;
                    if v <= 0.03928 {
                        v / 12.92
                    } else {
                        ((v + 0.055) / 1.055).powf(2.4)
                    }
                };
                0.2126 * f(c.red()) + 0.7152 * f(c.green()) + 0.0722 * f(c.blue())
            };
            let (x, y) = (lum(a), lum(b));
            (x.max(y) + 0.05) / (x.min(y) + 0.05)
        };
        let mut worst = Vec::new();
        for mode in [2, 1] {
            theme.set_mode(mode);
            let (p, t1, t2) = (theme.get_panel(), theme.get_text(), theme.get_text2());
            worst.push((mode, contrast(t1, p), contrast(t2, p)));
        }
        r.check(
            "text contrast, dark and light: ≥ 7:1 main, ≥ 4.5:1 secondary",
            worst.iter().all(|(_, a, b)| *a >= 7.0 && *b >= 4.5),
            format!("{worst:.2?}"),
        );
        theme.set_mode(0);
        ui.invoke_setting("theme".into(), 1);
        ui.set_insp_tab(0);
        r.check("light theme on", !theme.get_dark(), String::new());
    }));
    steps.push(Box::new(|_, ui, r| {
        r.snapshot(ui, "L1-editor-light");
        ui.set_insp_tab(2);
    }));
    steps.push(Box::new(|_, ui, r| {
        r.snapshot(ui, "L2-image-light");
        ui.set_insp_tab(1);
    }));
    steps.push(Box::new(|_, ui, r| {
        r.snapshot(ui, "L3-layers-light");
        ui.set_insp_tab(0);
        ui.invoke_settings_open();
        ui.set_settings_page(6);
    }));
    // ZK-198: the About page in the light theme (Slint's badge follows it too).
    steps.push(Box::new(|_, ui, r| {
        r.snapshot(ui, "L8-about-light");
        ui.set_settings_page(4);
    }));
    steps.push(Box::new(|_, ui, r| {
        r.snapshot(ui, "L4-settings-light");
        // ZK-140: «For developers» is hidden; ten real clicks on «Changes apply immediately»
        // (fast pairs, as double clicks) reveal it and do not maximize the window.
        let hidden = !ui.get_pref_dev_page();
        ten_clicks_on_applied(ui);
        r.check(
            "developer page: hidden, ten clicks on the header note reveal it, no maximize",
            hidden && ui.get_pref_dev_page() && !ui.get_win_maximized(),
            format!(
                "hidden {hidden} · shown {} · maximized {}",
                ui.get_pref_dev_page(),
                ui.get_win_maximized()
            ),
        );
        ui.set_settings_page(7);
    }));
    steps.push(Box::new(|_, ui, r| {
        r.snapshot(ui, "L7-developer-light");
        ten_clicks_on_applied(ui);
        r.check(
            "developer page: ten more clicks hide it again (and leave the page)",
            !ui.get_pref_dev_page() && ui.get_settings_page() == 6,
            format!(
                "shown {} · page {}",
                ui.get_pref_dev_page(),
                ui.get_settings_page()
            ),
        );
        ui.invoke_setting("onb-open".into(), 0);
    }));
    steps.push(Box::new(|app, ui, r| {
        r.snapshot(ui, "L5-onboarding-light");
        ui.invoke_setting("onb-done".into(), 1);
        app.borrow_mut().close_document(ui);
    }));
    steps.push(Box::new(|_, ui, r| {
        r.snapshot(ui, "L6-library-light");
        // Back to "as the system" for whatever runs after.
        ui.invoke_setting("theme".into(), 0);
    }));

    // Needs a live desktop: opt in with ZNIMOK_SELFTEST_CAPTURE=1.
    if std::env::var_os("ZNIMOK_SELFTEST_CAPTURE").is_some() {
        steps.push(Box::new(|app, ui, r| {
            // Same worker-thread path as the button (WinRT wants the MTA).
            let res = std::thread::spawn(crate::capture::display_under_cursor).join();
            match res {
                Ok(Ok(raster)) => {
                    let (w, h) = (raster.width, raster.height);
                    let lit = raster
                        .rgba
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .filter(|p| p[0] as u32 + p[1] as u32 + p[2] as u32 > 30)
                        .count();
                    r.check(
                        "capture display",
                        w > 0 && h > 0,
                        format!("{w}×{h}, {lit} non-black pixels"),
                    );
                    app.borrow_mut().new_document(ui, raster, "screen", None);
                }
                Ok(Err(crate::capture::Fail::Other(e))) => r.check("capture display", false, e),
                Ok(Err(crate::capture::Fail::Permission)) => r.check(
                    "capture display",
                    false,
                    "Screen Recording permission".into(),
                ),
                Err(_) => r.check("capture display", false, "panicked".into()),
            }
        }));
        steps.push(Box::new(|_, ui, r| r.snapshot(ui, "06-capture")));
    }

    let steps = Rc::new(RefCell::new(steps.into_iter()));
    let report = Rc::new(RefCell::new(Report {
        dir,
        ..Default::default()
    }));
    let timer = Rc::new(slint::Timer::default());
    let weak = ui.as_weak();
    let t2 = timer.clone();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(350),
        move || {
            let Some(ui) = weak.upgrade() else { return };
            let next = steps.borrow_mut().next();
            match next {
                Some(step) => {
                    // ZK-107: a step drives the newest window with a document — the editor the
                    // previous step opened — or the library when none is open.
                    let (a, w) = current(&app, &ui);
                    step(&a, &w, &mut report.borrow_mut());
                    w.window().request_redraw();
                }
                None => {
                    t2.stop();
                    let r = report.borrow();
                    let summary = format!("{} checks, {} failed", r.lines.len(), r.failed);
                    println!("{summary}");
                    let _ = std::fs::write(
                        r.dir.join("report.txt"),
                        format!("{}\n{summary}\n", r.lines.join("\n")),
                    );
                    let code = if r.failed == 0 { 0 } else { 1 };
                    // ZNIMOK_SELFTEST_REAL_QUIT: end the way the tray's «Quit» does — leave the
                    // event loop and go through the end of main (the exit crash of ZK-146).
                    if std::env::var_os("ZNIMOK_SELFTEST_REAL_QUIT").is_some() {
                        EXIT_CODE.store(code, std::sync::atomic::Ordering::SeqCst);
                        // As from the tray: the window is hidden when «Quit» is chosen.
                        let _ = ui.hide();
                        let _ = slint::quit_event_loop();
                    } else {
                        std::process::exit(code);
                    }
                }
            }
        },
    );
    // The timer lives as long as the closure it owns a handle to.
    std::mem::forget(timer);
}

thread_local! {
    /// The sample frozen for the over-the-screen test (ZK-58), and its copy as first shown.
    static OVER_SRC: std::cell::RefCell<Option<znimok_core::Raster>> = const { std::cell::RefCell::new(None) };
    static OVER_FLAT: std::cell::RefCell<Option<(u32, u32, Vec<u8>)>> = const { std::cell::RefCell::new(None) };
}

thread_local! {
    static DOUBLE_OK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

thread_local! {
    static QR_SCENE: std::cell::RefCell<Option<znimok_core::Raster>> = const { std::cell::RefCell::new(None) };
}

thread_local! {
    static SCROLL_WANT: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

thread_local! {
    /// The display the recording overlay test froze (ZK-180): its origin.
    static OVERLAY_DISPLAY: std::cell::Cell<Option<(i32, i32)>> = const { std::cell::Cell::new(None) };
    /// The document made from ZNIMOK_SELFTEST_VIDEO, removed at the end (ZK-181).
    static REAL_VIDEO: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
    /// The frame the real video's reverse run started from (ZK-92).
    static REAL_FROM: std::cell::Cell<i64> = const { std::cell::Cell::new(0) };
    static MULTI_WINDOWS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// ZK-107: editor windows before the two-window step opened its second one.
    static WINDOWS_BEFORE: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
