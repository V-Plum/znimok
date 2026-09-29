//! Scripted self-test: `ZNIMOK_SELFTEST=<dir> znimok-app <image>` drives the real window
//! through the same handlers the UI calls (pointer, keys, text, save, library, export), takes
//! window snapshots into `<dir>` and writes `report.txt`. Exit code 0 = every check passed.
//! Needs no desktop session and no synthetic input, so it runs over an idle RDP connection and
//! in CI.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use slint::ComponentHandle;

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
            ui.invoke_set_prop("fill".into(), 4);
            r.snapshot(ui, "14-props-rect");
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
        ui.invoke_tool_chosen(0);
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
        crate::pill::close();
    }));
    // ZK-60: Esc takes off one layer at a time, Enter repeats the last share, [ ] thickness,
    // Ctrl+= zooms in.
    steps.push(Box::new(|app, ui, r| {
        use crate::app::KeyAction;
        let k = |t: &str, ctrl: bool| app.borrow_mut().key(ui, t, ctrl, false);
        k("a", true); // select all
        app.borrow_mut().set_tool(ui, crate::app::tool::RECT);
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
            enter == KeyAction::Copy && enter2 == KeyAction::Export,
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
    steps.push(Box::new(|_, ui, r| {
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
            ui.invoke_card_trash(c.path.clone());
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
        let (kx, ky) = (1600.0 / lw.max(1.0), 1000.0 / lh.max(1.0));
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
                    .is_some_and(|b| b.width() == 1600 && b.height() == 1),
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
        let (kx, ky) = (1600.0 / lw.max(1.0), 1000.0 / lh.max(1.0));
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
                    w == 1600
                        && h == 1000
                        && c.is_some_and(|c| {
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
        r.snapshot(ui, "28-over-screen");
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
        let Some(raster) = raster else { return };
        let display = znimok_platform::Rect {
            x: 0,
            y: 0,
            width: raster.width,
            height: raster.height,
        };
        let n0 = count(&dir);
        let frame = znimok_core::IRect::new(10, 10, 200, 100);
        a.over_open(ui, raster.clone(), frame, "region", display, true);
        a.over_finish(ui, false, false);
        let n1 = count(&dir);
        a.over_open(ui, raster.clone(), frame, "region", display, true);
        a.over_finish(ui, false, true);
        let n2 = count(&dir);
        let back = !ui.get_over_screen() && ui.get_page() == 0;
        // What follows expects a document in the editor.
        a.new_document(ui, raster, "region", None);
        drop(a);
        r.check(
            "over the screen: Esc keeps nothing, Ctrl+S saves to the library",
            n1 == n0 && n2 == n0 + 1 && back,
            format!("{n0} → {n1} → {n2} · back to the library {back}"),
        );
        crate::pill::close();
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
        ui.set_settings_page(4);
    }));
    steps.push(Box::new(|_, ui, r| {
        r.snapshot(ui, "L4-settings-light");
        ui.set_settings_page(7);
    }));
    steps.push(Box::new(|_, ui, r| {
        r.snapshot(ui, "L7-developer-light");
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
                    step(&app, &ui, &mut report.borrow_mut());
                    ui.window().request_redraw();
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
                    std::process::exit(code);
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
