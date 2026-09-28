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

    // Capture overlay on a synthetic frozen frame: hover a window, drag a region.
    steps.push(Box::new(|app, _ui, r| {
        let raster = app
            .borrow()
            .s
            .as_ref()
            .map(|s| (*s.ed.doc.banks[0]).clone());
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
        let kk = 1600.0 / lw.max(1.0);
        ov.invoke_pointer(1, 300.0 / kk, 250.0 / kk, false);
        let (has, win) = (ov.get_has_sel(), ov.get_is_window());
        r.check(
            "overlay hover highlights the window",
            has && win,
            format!("label {}", ov.get_sel_label()),
        );
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
        ov.invoke_wheel(1.0);
        let on4 = ov.get_lens_visible();
        ov.invoke_wheel(1.0);
        r.check(
            "wheel turns the magnifier on (×4, ×8)",
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
        ov.invoke_pointer(0, 600.0 / kk, 500.0 / kk, false);
        for i in 1..=10 {
            let t = i as f32 / 10.0;
            ov.invoke_pointer(1, (600.0 + 300.0 * t) / kk, (500.0 + 200.0 * t) / kk, false);
        }
        r.check(
            "overlay drag",
            ov.get_has_sel() && !ov.get_is_window(),
            format!("label {}", ov.get_sel_label()),
        );
        r.snapshot_window(ov.window(), "08-overlay-drag");
        ov.invoke_pointer(2, 900.0 / kk, 700.0 / kk, false);
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
