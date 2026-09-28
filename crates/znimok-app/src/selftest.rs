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
        match ui.window().take_snapshot() {
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
    a.pointer(ui, 0, from.0, from.1, 0, false);
    for i in 1..=12 {
        let t = i as f32 / 12.0;
        a.pointer(
            ui,
            1,
            from.0 + (to.0 - from.0) * t,
            from.1 + (to.1 - from.1) * t,
            0,
            false,
        );
    }
    a.pointer(ui, 2, to.0, to.1, 0, false);
}

fn click(app: &Shared, ui: &AppWindow, at: (f32, f32)) {
    let mut a = app.borrow_mut();
    a.pointer(ui, 0, at.0, at.1, 0, false);
    a.pointer(ui, 2, at.0, at.1, 0, false);
}

fn key(app: &Shared, ui: &AppWindow, text: &str, ctrl: bool, shift: bool) {
    let _ = app.borrow_mut().key(ui, text, ctrl, shift);
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
            a.pointer(ui, 0, cx - 250.0, cy + 120.0, 0, false);
            for i in 1..=40 {
                let t = i as f32 / 40.0;
                a.pointer(
                    ui,
                    1,
                    cx - 250.0 + 200.0 * t,
                    cy + 120.0 + 40.0 * (t * std::f32::consts::TAU).sin(),
                    0,
                    false,
                );
            }
            a.pointer(ui, 2, cx - 50.0, cy + 120.0, 0, false);
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
        ui.set_edit_text("Привіт, Znimok".into());
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
        r.check("select all + colour", blue == 7, format!("{blue} blue"));
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
        if let Some(c) = slint::Model::row_data(&ui.get_cards(), 0) {
            ui.invoke_open_card(c.path);
        }
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
                Ok(Err(e)) => r.check("capture display", false, e),
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
