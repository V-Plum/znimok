//! Application state: the library, the open document and the canvas.
//! Every change to a document goes through `znimok_core::Editor::apply` — the same commands the
//! CLI and the agents use (ZK-24), so undo, dirty tracking and validation are the core's.

use std::path::{Path, PathBuf};
use std::time::Instant;

use slint::wgpu_30::wgpu;
use slint::{ComponentHandle, SharedString, VecModel};
use znimok_core::hit;
use znimok_core::*;
use znimok_i18n::{FluentArgs, Localizer};
use znimok_render::vello_cpu::Pixmap;
use znimok_render::vello_cpu::kurbo::Point;
use znimok_render::{Renderer, View};

use crate::library::{self, Entry};
use crate::{AppWindow, CardData, io};

pub const PALETTE: [Rgb; 8] = [
    Rgb::new(0xFF, 0x5A, 0x5F),
    Rgb::new(0xFF, 0x8A, 0x3D),
    Rgb::new(0xFF, 0xD2, 0x3F),
    Rgb::new(0x34, 0xC4, 0x8A),
    Rgb::new(0x3D, 0x7B, 0xF5),
    Rgb::new(0x9B, 0x5C, 0xF5),
    Rgb::new(0xFF, 0xFF, 0xFF),
    Rgb::new(0x14, 0x16, 0x1A),
];
pub const THICK: [i32; 3] = [2, 4, 7];

/// Held by every writer of a document: the background autosave and `save_now`. Both write
/// `<file>.part` and rename it, so two writers at once would corrupt the part file; leaving a
/// document also waits here so the library is scanned after the file is complete.
pub static SAVE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Tool order of the rail in `app.slint`.
pub mod tool {
    pub const SELECT: usize = 0;
    pub const RECT: usize = 1;
    pub const ELLIPSE: usize = 2;
    pub const ARROW: usize = 3;
    pub const PEN: usize = 4;
    pub const TEXT: usize = 5;
    pub const HIDE: usize = 6;
    pub const MARKER: usize = 7;
    pub const COUNTER: usize = 8;
    pub const STAMP: usize = 9;
    pub const NAMES: [&str; 10] = [
        "tool-select",
        "tool-rect",
        "tool-ellipse",
        "tool-arrow",
        "tool-pen",
        "tool-text",
        "tool-hide",
        "tool-highlighter",
        "tool-counter",
        "tool-stamp",
    ];
}

#[derive(Clone, Debug)]
enum Drag {
    /// Button down with a drawing tool; the mark appears once the pointer moved a few pixels,
    /// so a stray click leaves no empty mark and no undo step.
    Pending {
        start: (i32, i32),
        start_out: Point,
    },
    Create {
        id: ObjectId,
        start: (i32, i32),
        merge: MergeKey,
    },
    Pen {
        id: ObjectId,
        points: Vec<(i32, i32)>,
        merge: MergeKey,
    },
    Move {
        last: (i32, i32),
        merge: MergeKey,
    },
    Resize {
        id: ObjectId,
        handle: usize,
        orig: IRect,
        grab: (f64, f64),
        merge: MergeKey,
    },
    Pan {
        start_out: Point,
        orig: Point,
    },
}

enum Editing {
    New { at: (i32, i32) },
    Existing { id: ObjectId },
}

pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub texture: Option<(wgpu::Texture, u32, u32)>,
}

pub struct Session {
    pub ed: Editor,
    pub path: PathBuf,
    changed_at: Instant,
    /// A background save failed: keep trying even though the editor thinks it is saved.
    save_failed: bool,
}

pub struct App {
    pub tr: Localizer,
    pub lib_dir: PathBuf,
    entries: Vec<Entry>,
    filter: String,
    pub s: Option<Session>,
    renderer: Renderer,
    view: View,
    pixmap: Pixmap,
    pub gpu: Option<Gpu>,
    pub dpr: f64,
    dirty: bool,
    fit_pending: bool,
    tool: usize,
    color: usize,
    thick: usize,
    drag: Option<Drag>,
    next_merge: u64,
    editing: Option<Editing>,
    pub autosave: bool,
    pub saving: bool,
    toast_at: Option<Instant>,
}

fn args(pairs: &[(&'static str, String)]) -> FluentArgs<'static> {
    let mut a = FluentArgs::new();
    for (k, v) in pairs {
        a.set(*k, v.clone());
    }
    a
}

fn bounds_of_points(points: &[(i32, i32)]) -> IRect {
    Object::new(
        IRect::default(),
        Data::Pen {
            points: points.to_vec(),
        },
    )
    .bounds()
}

impl App {
    pub fn new(tr: Localizer, lib_dir: PathBuf) -> Self {
        Self {
            tr,
            lib_dir,
            entries: Vec::new(),
            filter: String::new(),
            s: None,
            renderer: Renderer::new(),
            view: View {
                scale: 1.0,
                origin: Point::ZERO,
                width: 1,
                height: 1,
            },
            pixmap: Pixmap::new(1, 1),
            gpu: None,
            dpr: 1.0,
            dirty: true,
            fit_pending: true,
            tool: tool::RECT,
            color: 0,
            thick: 1,
            drag: None,
            next_merge: 1,
            editing: None,
            autosave: true,
            saving: false,
            toast_at: None,
        }
    }

    // ------------------------------------------------------------------ library

    pub fn refresh_library(&mut self, ui: &AppWindow) {
        self.entries = library::scan(&self.lib_dir);
        self.show_cards(ui);
    }

    pub fn set_filter(&mut self, ui: &AppWindow, f: &str) {
        self.filter = f.trim().to_string();
        self.show_cards(ui);
    }

    fn show_cards(&self, ui: &AppWindow) {
        let cards: Vec<CardData> = self
            .entries
            .iter()
            .filter(|e| e.matches(&self.filter))
            .map(|e| CardData {
                name: e.name.as_str().into(),
                meta: e.meta_line().into(),
                thumb: e
                    .thumb_png
                    .as_deref()
                    .and_then(library::thumb_image)
                    .unwrap_or_default(),
                path: e.path.display().to_string().into(),
            })
            .collect();
        ui.set_cards(std::rc::Rc::new(VecModel::from(cards)).into());
        ui.set_library_dir(self.lib_dir.display().to_string().into());
    }

    pub fn toast(&mut self, ui: &AppWindow, text: impl Into<SharedString>) {
        ui.set_toast(text.into());
        self.toast_at = Some(Instant::now());
    }

    pub fn tick_toast(&mut self, ui: &AppWindow) {
        if self
            .toast_at
            .is_some_and(|t| t.elapsed().as_secs_f32() > 3.5)
        {
            self.toast_at = None;
            ui.set_toast(SharedString::new());
        }
    }

    // ------------------------------------------------------------------ documents

    /// A new library document from pixels (screenshot, clipboard, image file).
    pub fn new_document(
        &mut self,
        ui: &AppWindow,
        raster: Raster,
        source: &str,
        name: Option<String>,
    ) {
        let now = chrono::Local::now();
        let name = name.unwrap_or_else(|| {
            self.tr.tr_args(
                "doc-untitled",
                &args(&[
                    ("date", now.format("%Y-%m-%d").to_string()),
                    ("time", now.format("%H.%M.%S").to_string()),
                ]),
            )
        });
        let mut doc = Document::from_raster(name, raster);
        doc.meta.created_ms = now.timestamp_millis();
        doc.meta.source = source.into();
        let path = library::new_path(&self.lib_dir, &doc.id.simple().to_string());
        // Not on disk yet: `fresh` makes the first autosave write it.
        self.open_session(ui, Editor::new(doc), path, true);
    }

    fn open_session(&mut self, ui: &AppWindow, ed: Editor, path: PathBuf, fresh: bool) {
        self.s = Some(Session {
            ed,
            path,
            changed_at: Instant::now(),
            save_failed: fresh,
        });
        self.drag = None;
        self.editing = None;
        ui.set_editing(false);
        self.fit_pending = true;
        self.dirty = true;
        ui.set_page(1);
        ui.invoke_focus_canvas();
        self.sync(ui);
        ui.window().request_redraw();
    }

    /// Opens a `.znimok` in place, or makes a new library document from an image file.
    pub fn open_path(&mut self, ui: &AppWindow, path: &Path) {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let data = match std::fs::read(path) {
            Ok(d) => d,
            Err(e) => {
                self.toast(ui, format!("{name}: {e}"));
                return;
            }
        };
        if znimok_format::is_znimok(&data) {
            match znimok_format::read(&data) {
                Ok(doc) => self.open_session(ui, Editor::new(doc), path.to_path_buf(), false),
                Err(e) => self.toast(ui, format!("{name}: {e}")),
            }
            return;
        }
        match io::load_image(path) {
            Ok(r) => {
                let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned());
                self.new_document(ui, r, "file", stem);
            }
            Err(_) => {
                let msg = self
                    .tr
                    .tr_args("open-error-not-image", &args(&[("name", name)]));
                self.toast(ui, msg);
            }
        }
    }

    pub fn open_clipboard(&mut self, ui: &AppWindow) {
        match io::paste_image() {
            Some(r) => self.new_document(ui, r, "clipboard", None),
            None => {
                let msg = self.tr.tr("open-error-no-image");
                self.toast(ui, msg);
            }
        }
    }

    pub fn is_unsaved(&self) -> bool {
        self.s
            .as_ref()
            .is_some_and(|s| s.ed.is_dirty() || s.save_failed)
    }

    pub fn doc_name(&self) -> String {
        self.s
            .as_ref()
            .map(|s| s.ed.doc.name.clone())
            .unwrap_or_default()
    }

    /// Thumbnail and write options for a save.
    fn write_options(&mut self) -> Option<znimok_format::WriteOptions> {
        let s = self.s.as_ref()?;
        let doc = &s.ed.doc;
        let f = doc.frame();
        let k = (320.0 / f.w as f64).min(240.0 / f.h as f64).min(1.0);
        let mut view = View::one_to_one(doc);
        view.scale = k;
        view.width = ((f.w as f64 * k).round() as i64).clamp(1, 320) as u16;
        view.height = ((f.h as f64 * k).round() as i64).clamp(1, 240) as u16;
        let mut pix = Pixmap::new(1, 1);
        self.renderer.render(doc, view, &mut pix);
        Some(znimok_format::WriteOptions {
            app_version: format!("Znimok {}", env!("CARGO_PKG_VERSION")),
            thumbnail: Some(Raster::new(
                pix.width() as u32,
                pix.height() as u32,
                znimok_render::pixmap_to_rgba(&pix),
            )),
            ..Default::default()
        })
    }

    /// Saves now, on this thread (leaving the document, closing the window, Ctrl+S).
    pub fn save_now(&mut self, ui: &AppWindow) -> bool {
        if self.s.is_none() {
            return true;
        }
        self.finish_text(ui);
        let Some(opts) = self.write_options() else {
            return true;
        };
        let _guard = SAVE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let s = self.s.as_mut().unwrap();
        if let Some(dir) = s.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match znimok_format::save(&s.path, &s.ed.doc, &opts) {
            Ok(()) => {
                s.ed.mark_saved();
                s.save_failed = false;
                self.sync(ui);
                true
            }
            Err(e) => {
                let msg = format!("{} ({e})", self.tr.tr("err-library-save"));
                self.toast(ui, msg);
                false
            }
        }
    }

    /// Autosave: a copy of the document is written on a worker thread; the result comes back
    /// through [`App::save_finished`]. PNG encoding of a 4K source takes a few hundred ms.
    pub fn autosave_job(
        &mut self,
        ui: &AppWindow,
    ) -> Option<(PathBuf, Document, znimok_format::WriteOptions)> {
        if !self.autosave || self.saving || self.drag.is_some() || self.editing.is_some() {
            return None;
        }
        let s = self.s.as_ref()?;
        if !(s.ed.is_dirty() || s.save_failed) || s.changed_at.elapsed().as_millis() < 700 {
            return None;
        }
        let opts = self.write_options()?;
        let s = self.s.as_mut()?;
        let job = (s.path.clone(), s.ed.doc.clone(), opts);
        s.ed.mark_saved();
        s.save_failed = false;
        self.saving = true;
        ui.set_save_state(2);
        Some(job)
    }

    pub fn save_finished(&mut self, ui: &AppWindow, path: &Path, result: Result<(), String>) {
        self.saving = false;
        if let Err(e) = result
            && let Some(s) = self.s.as_mut()
            && s.path == path
        {
            s.save_failed = true;
            let msg = format!("{} ({e})", self.tr.tr("err-library-save"));
            self.toast(ui, msg);
        }
        self.sync(ui);
    }

    pub fn close_document(&mut self, ui: &AppWindow) {
        drop(SAVE_LOCK.lock());
        self.s = None;
        self.drag = None;
        self.editing = None;
        ui.set_editing(false);
        ui.set_page(0);
        ui.invoke_focus_library();
        self.refresh_library(ui);
    }

    // ------------------------------------------------------------------ core commands

    fn apply(&mut self, ui: &AppWindow, cmd: Command) -> Option<Applied> {
        let s = self.s.as_mut()?;
        match s.ed.apply(cmd) {
            Ok(a) => {
                if !a.is_empty() {
                    s.changed_at = Instant::now();
                }
                self.dirty = true;
                Some(a)
            }
            Err(e) => {
                self.toast(ui, e.to_string());
                None
            }
        }
    }

    fn merge_key(&mut self) -> MergeKey {
        self.next_merge += 1;
        MergeKey::Drag {
            id: self.next_merge,
        }
    }

    pub fn undo(&mut self, ui: &AppWindow) {
        self.finish_text(ui);
        self.apply(ui, Command::Undo);
        self.sync(ui);
    }

    pub fn redo(&mut self, ui: &AppWindow) {
        self.finish_text(ui);
        self.apply(ui, Command::Redo);
        self.sync(ui);
    }

    fn selection(&self) -> Vec<ObjectId> {
        self.s
            .as_ref()
            .map(|s| s.ed.selection().to_vec())
            .unwrap_or_default()
    }

    fn style_for(&self, t: usize) -> Style {
        let base = Style {
            color: PALETTE[self.color],
            thick: THICK[self.thick],
            ..Style::default()
        };
        let side = self
            .s
            .as_ref()
            .map(|s| {
                let f = s.ed.doc.frame();
                f.w.min(f.h)
            })
            .unwrap_or(1000);
        // Counters and stamps are sized to the picture (LH: ~3.6 % of the short side).
        let badge = ((side as f64 * 0.036).round() as i32).clamp(28, 96);
        match t {
            tool::MARKER => Style {
                // The red default reads badly as a highlighter: yellow unless a colour was picked.
                color: if self.color == 0 {
                    Rgb::YELLOW
                } else {
                    PALETTE[self.color]
                },
                thick: 8 * THICK[self.thick] + 8,
                alpha: 100,
                ..base
            },
            tool::COUNTER => Style {
                thick: badge,
                ..base
            },
            tool::STAMP => Style {
                color: if self.color == 0 {
                    Rgb::GREEN
                } else {
                    PALETTE[self.color]
                },
                thick: badge + 8,
                ..base
            },
            _ => base,
        }
    }

    fn text_size(&self) -> i32 {
        let h = self.s.as_ref().map(|s| s.ed.doc.frame().h).unwrap_or(1080);
        let base = [18, 24, 36][self.thick];
        ((base as f64 * (h as f64 / 1080.0).max(1.0)).round() as i32).max(10)
    }

    fn new_object(&self, t: usize, a: (i32, i32), b: (i32, i32)) -> Object {
        let rect = IRect::new(a.0, a.1, b.0 - a.0, b.1 - a.1);
        let data = match t {
            tool::RECT => Data::Rect,
            tool::ELLIPSE => Data::Ellipse,
            tool::ARROW => Data::Line {
                head_front: Head::Triangle,
                head_back: Head::None,
                head_size: 1,
            },
            tool::HIDE => Data::Hide {
                mode: HideMode::Pixelate,
                strength: 50,
            },
            _ => Data::Mark,
        };
        Object::new(rect, data).with_style(self.style_for(t))
    }

    // ------------------------------------------------------------------ canvas input

    fn to_doc(&self, x: f32, y: f32) -> (Point, (i32, i32)) {
        let out = Point::new(x as f64 * self.dpr, y as f64 * self.dpr);
        let p = self.view.to_doc(out.x, out.y);
        (out, (p.x.round() as i32, p.y.round() as i32))
    }

    pub fn pointer(&mut self, ui: &AppWindow, kind: i32, x: f32, y: f32, button: i32, shift: bool) {
        if self.s.is_none() {
            return;
        }
        let (out, p) = self.to_doc(x, y);
        match kind {
            0 => self.pointer_down(ui, out, p, button, shift),
            1 => self.pointer_move(ui, out, p, shift),
            _ => self.pointer_up(ui),
        }
        self.sync(ui);
        ui.window().request_redraw();
    }

    fn pointer_down(
        &mut self,
        ui: &AppWindow,
        out: Point,
        p: (i32, i32),
        button: i32,
        shift: bool,
    ) {
        if button == 2 {
            self.drag = Some(Drag::Pan {
                start_out: out,
                orig: self.view.origin,
            });
            return;
        }
        if button != 0 {
            return;
        }
        self.finish_text(ui);
        let px = self.view.scale / self.dpr;
        let sel = self.selection();

        // Handles of a single selection come before the tool (LH §2.7).
        if sel.len() == 1 {
            let doc = &self.s.as_ref().unwrap().ed.doc;
            if let Some(o) = doc.get(sel[0]) {
                let pd = self.view.to_doc(out.x, out.y);
                if let Some(h) = hit::hit_handle(o, (pd.x, pd.y), px) {
                    let grab = hit::handles(o)[h];
                    let (id, orig) = (o.id, o.rect);
                    let merge = self.merge_key();
                    self.drag = Some(Drag::Resize {
                        id,
                        handle: h,
                        orig,
                        grab,
                        merge,
                    });
                    return;
                }
            }
        }

        let hit_id = {
            let doc = &self.s.as_ref().unwrap().ed.doc;
            hit::pick(doc, (p.0 as f64, p.1 as f64), px).map(|i| doc.objects[i].id)
        };
        match self.tool {
            tool::SELECT => match hit_id {
                Some(id) => {
                    if !sel.contains(&id) {
                        self.apply(
                            ui,
                            Command::Select {
                                ids: vec![id],
                                add: shift,
                            },
                        );
                    }
                    let merge = self.merge_key();
                    self.drag = Some(Drag::Move { last: p, merge });
                }
                None => {
                    self.apply(ui, Command::ClearSelection);
                }
            },
            tool::TEXT => {
                let existing = hit_id.filter(|id| {
                    self.s
                        .as_ref()
                        .unwrap()
                        .ed
                        .doc
                        .get(*id)
                        .is_some_and(|o| o.kind() == Kind::Text)
                });
                self.start_text(ui, p, existing);
            }
            tool::COUNTER | tool::STAMP => {
                let st = self.style_for(self.tool);
                let d = st.thick;
                let data = if self.tool == tool::COUNTER {
                    Data::Counter {
                        seq: 0,
                        group: 1,
                        start: 1,
                        shape: CounterShape::Circle,
                    }
                } else {
                    Data::Stamp { id: 0 }
                };
                let obj =
                    Object::new(IRect::new(p.0 - d / 2, p.1 - d / 2, d, d), data).with_style(st);
                self.apply(
                    ui,
                    Command::AddObject {
                        object: obj,
                        select: true,
                        merge: None,
                    },
                );
            }
            _ => {
                self.drag = Some(Drag::Pending {
                    start: p,
                    start_out: out,
                });
            }
        }
    }

    fn pointer_move(&mut self, ui: &AppWindow, out: Point, p: (i32, i32), shift: bool) {
        let Some(drag) = self.drag.clone() else {
            return;
        };
        match drag {
            Drag::Pending { start, start_out } => {
                if (out - start_out).hypot() < 3.0 * self.dpr {
                    return;
                }
                let merge = self.merge_key();
                if self.tool == tool::PEN {
                    let points = vec![start, p];
                    let mut obj = Object::new(
                        bounds_of_points(&points),
                        Data::Pen {
                            points: points.clone(),
                        },
                    )
                    .with_style(self.style_for(tool::PEN));
                    obj.rect = obj.bounds();
                    if let Some(a) = self.apply(
                        ui,
                        Command::AddObject {
                            object: obj,
                            select: true,
                            merge: Some(merge.clone()),
                        },
                    ) && let Some(id) = a.created
                    {
                        self.drag = Some(Drag::Pen { id, points, merge });
                    }
                } else {
                    let obj = self.new_object(self.tool, start, p);
                    if let Some(a) = self.apply(
                        ui,
                        Command::AddObject {
                            object: obj,
                            select: true,
                            merge: Some(merge.clone()),
                        },
                    ) && let Some(id) = a.created
                    {
                        self.drag = Some(Drag::Create { id, start, merge });
                    }
                }
            }
            Drag::Create { id, start, merge } => {
                let (mut w, mut h) = (p.0 - start.0, p.1 - start.1);
                if shift && self.tool != tool::ARROW {
                    let m = w.abs().max(h.abs());
                    w = m * if w < 0 { -1 } else { 1 };
                    h = m * if h < 0 { -1 } else { 1 };
                }
                let rect = IRect::new(start.0, start.1, w, h);
                self.apply(
                    ui,
                    Command::UpdateObjects {
                        ids: vec![id],
                        patch: ObjectPatch {
                            rect: Some(rect),
                            ..Default::default()
                        },
                        merge: Some(merge),
                    },
                );
            }
            Drag::Pen {
                id,
                mut points,
                merge,
            } => {
                let last = *points.last().unwrap();
                if last == p {
                    return;
                }
                points.push(p);
                let patch = ObjectPatch {
                    data: Some(Data::Pen {
                        points: points.clone(),
                    }),
                    rect: Some(bounds_of_points(&points)),
                    ..Default::default()
                };
                self.apply(
                    ui,
                    Command::UpdateObjects {
                        ids: vec![id],
                        patch,
                        merge: Some(merge.clone()),
                    },
                );
                self.drag = Some(Drag::Pen { id, points, merge });
            }
            Drag::Move { last, merge } => {
                let (dx, dy) = (p.0 - last.0, p.1 - last.1);
                if (dx, dy) == (0, 0) {
                    return;
                }
                let ids = self.selection();
                self.apply(
                    ui,
                    Command::MoveObjects {
                        ids,
                        dx,
                        dy,
                        merge: Some(merge.clone()),
                    },
                );
                self.drag = Some(Drag::Move { last: p, merge });
            }
            Drag::Resize {
                id,
                handle,
                orig,
                grab,
                merge,
            } => {
                let pd = self.view.to_doc(out.x, out.y);
                let (dx, dy) = (
                    (pd.x - grab.0).round() as i32,
                    (pd.y - grab.1).round() as i32,
                );
                self.apply(
                    ui,
                    Command::ResizeObject {
                        id,
                        handle,
                        orig,
                        dx,
                        dy,
                        merge: Some(merge.clone()),
                    },
                );
                self.fit_text(ui, id, Some(merge));
            }
            Drag::Pan { start_out, orig } => {
                let sc = self.view.scale;
                self.view.origin = Point::new(
                    orig.x - (out.x - start_out.x) / sc,
                    orig.y - (out.y - start_out.y) / sc,
                );
                self.dirty = true;
            }
        }
    }

    fn pointer_up(&mut self, _ui: &AppWindow) {
        self.drag = None;
        self.dirty = true;
    }

    /// Text marks size themselves from their text.
    fn fit_text(&mut self, ui: &AppWindow, id: ObjectId, merge: Option<MergeKey>) {
        let Some(o) = self.s.as_ref().and_then(|s| s.ed.doc.get(id)) else {
            return;
        };
        let Data::Text {
            text,
            size,
            bold,
            italic,
            box_w,
            ..
        } = &o.data
        else {
            return;
        };
        let (w, h) = self
            .renderer
            .measure_text(text, *size, *bold, *italic, *box_w);
        let mut r = o.rect;
        r.w = if *box_w > 0 {
            *box_w
        } else {
            w.ceil() as i32 + 2
        };
        r.h = h.ceil() as i32;
        if r != o.rect {
            self.apply(
                ui,
                Command::UpdateObjects {
                    ids: vec![id],
                    patch: ObjectPatch {
                        rect: Some(r),
                        ..Default::default()
                    },
                    merge,
                },
            );
        }
    }

    fn start_text(&mut self, ui: &AppWindow, p: (i32, i32), existing: Option<ObjectId>) {
        let (at, text) =
            match existing.and_then(|id| self.s.as_ref()?.ed.doc.get(id).map(|o| (id, o))) {
                Some((id, o)) => {
                    let t = match &o.data {
                        Data::Text { text, .. } => text.clone(),
                        _ => String::new(),
                    };
                    let at = (o.rect.x, o.rect.y);
                    self.editing = Some(Editing::Existing { id });
                    (at, t)
                }
                None => {
                    self.editing = Some(Editing::New { at: p });
                    (p, String::new())
                }
            };
        let o = self.view.to_out(Point::new(at.0 as f64, at.1 as f64));
        ui.set_edit_x((o.x / self.dpr) as f32);
        ui.set_edit_y((o.y / self.dpr - 6.0) as f32);
        ui.set_edit_text(text.into());
        ui.set_editing(true);
    }

    pub fn commit_text(&mut self, ui: &AppWindow, text: &str) {
        let Some(editing) = self.editing.take() else {
            return;
        };
        ui.set_editing(false);
        let text = text.trim_end().to_string();
        match editing {
            Editing::New { at } => {
                if !text.trim().is_empty() {
                    let size = self.text_size();
                    let obj = Object::new(
                        IRect::new(at.0, at.1, 1, 1),
                        Data::Text {
                            text,
                            size,
                            bold: false,
                            italic: false,
                            align: Align::Left,
                            box_w: 0,
                        },
                    )
                    .with_style(self.style_for(tool::TEXT));
                    let merge = self.merge_key();
                    if let Some(a) = self.apply(
                        ui,
                        Command::AddObject {
                            object: obj,
                            select: true,
                            merge: Some(merge.clone()),
                        },
                    ) && let Some(id) = a.created
                    {
                        self.fit_text(ui, id, Some(merge));
                    }
                }
            }
            Editing::Existing { id } => {
                if text.trim().is_empty() {
                    self.apply(ui, Command::DeleteObjects { ids: vec![id] });
                } else {
                    let merge = self.merge_key();
                    self.apply(
                        ui,
                        Command::UpdateObjects {
                            ids: vec![id],
                            patch: ObjectPatch {
                                text: Some(text),
                                ..Default::default()
                            },
                            merge: Some(merge.clone()),
                        },
                    );
                    self.fit_text(ui, id, Some(merge));
                }
            }
        }
        ui.invoke_focus_canvas();
        self.sync(ui);
        ui.window().request_redraw();
    }

    pub fn cancel_text(&mut self, ui: &AppWindow) {
        self.editing = None;
        ui.set_editing(false);
        ui.invoke_focus_canvas();
    }

    /// Commits a text being typed (clicking elsewhere, saving, undo).
    fn finish_text(&mut self, ui: &AppWindow) {
        if self.editing.is_some() {
            let t = ui.get_edit_text().to_string();
            self.commit_text(ui, &t);
        }
    }

    pub fn wheel(&mut self, ui: &AppWindow, x: f32, y: f32, dy: f32, zoom: bool) {
        if self.s.is_none() {
            return;
        }
        let out = Point::new(x as f64 * self.dpr, y as f64 * self.dpr);
        if zoom {
            let factor = if dy > 0.0 { 1.15 } else { 1.0 / 1.15 };
            let z = self.view.scale * factor;
            self.set_zoom(z, Some(out));
        } else {
            self.view.origin.y -= dy as f64 * self.dpr / self.view.scale;
            self.dirty = true;
        }
        self.sync(ui);
        ui.window().request_redraw();
    }

    // ------------------------------------------------------------------ keys and tools

    pub fn set_tool(&mut self, ui: &AppWindow, t: usize) {
        self.finish_text(ui);
        self.tool = t.min(tool::STAMP);
        ui.set_tool(self.tool as i32);
        self.sync(ui);
    }

    /// Keys of the canvas. Letters are matched on both the Latin and the Ukrainian layout, so a
    /// tool key works whatever layout is on (the physical-key API comes with ZK-35's hotkeys).
    pub fn key(&mut self, ui: &AppWindow, text: &str, ctrl: bool, shift: bool) -> KeyAction {
        const PAIRS: [(char, char); 16] = [
            ('v', 'м'),
            ('r', 'к'),
            ('e', 'у'),
            ('l', 'д'),
            ('p', 'з'),
            ('t', 'е'),
            ('b', 'и'),
            ('h', 'р'),
            ('n', 'т'),
            ('s', 'і'),
            ('z', 'я'),
            ('y', 'н'),
            ('c', 'с'),
            ('o', 'щ'),
            ('a', 'ф'),
            ('i', 'ш'),
        ];
        let mut chars = text.chars();
        let ch = match (chars.next(), chars.next()) {
            (Some(c), None) => Some(c.to_lowercase().next().unwrap_or(c)),
            _ => None,
        };
        let latin = ch.map(|c| {
            PAIRS
                .iter()
                .find(|(_, u)| *u == c)
                .map(|(l, _)| *l)
                .unwrap_or(c)
        });
        if ctrl {
            match latin {
                Some('z') if shift => self.redo(ui),
                Some('z') => self.undo(ui),
                Some('y') => self.redo(ui),
                Some('c') => return KeyAction::Copy,
                Some('v') => self.paste_as_mark(ui),
                Some('s') if shift => return KeyAction::Export,
                Some('s') => {
                    self.save_now(ui);
                }
                Some('o') => return KeyAction::Open,
                Some('a') => {
                    self.apply(ui, Command::SelectAll);
                }
                Some('0') => self.zoom_fit(ui),
                Some('1') => self.zoom_100(ui),
                _ => return KeyAction::None,
            }
            self.sync(ui);
            ui.window().request_redraw();
            return KeyAction::None;
        }
        let tools = ['v', 'r', 'e', 'l', 'p', 't', 'b', 'h', 'n', 's'];
        if let Some(i) = latin.and_then(|c| tools.iter().position(|t| *t == c)) {
            self.set_tool(ui, i);
            return KeyAction::None;
        }
        let step = if shift { 10 } else { 1 };
        let nudge = match text {
            "\u{F702}" => Some((-step, 0)),
            "\u{F703}" => Some((step, 0)),
            "\u{F700}" => Some((0, -step)),
            "\u{F701}" => Some((0, step)),
            _ => None,
        };
        if let Some((dx, dy)) = nudge {
            let ids = self.selection();
            if !ids.is_empty() {
                self.apply(
                    ui,
                    Command::MoveObjects {
                        ids,
                        dx,
                        dy,
                        merge: Some(MergeKey::Nudge),
                    },
                );
            }
        } else {
            match text {
                "\u{7f}" | "\u{8}" => {
                    let ids = self.selection();
                    if !ids.is_empty() {
                        self.apply(ui, Command::DeleteObjects { ids });
                    }
                }
                "\u{1b}" => {
                    if self.drag.take().is_none() {
                        if !self.selection().is_empty() {
                            self.apply(ui, Command::ClearSelection);
                        } else if self.tool != tool::SELECT {
                            self.set_tool(ui, tool::SELECT);
                        }
                    }
                }
                _ => return KeyAction::None,
            }
        }
        self.sync(ui);
        ui.window().request_redraw();
        KeyAction::None
    }

    /// Ctrl+V in the editor: the clipboard picture becomes an image mark in the middle.
    fn paste_as_mark(&mut self, ui: &AppWindow) {
        let Some(r) = io::paste_image() else {
            let msg = self.tr.tr("open-error-no-image");
            self.toast(ui, msg);
            return;
        };
        let Some(s) = self.s.as_mut() else { return };
        let f = s.ed.doc.frame();
        let k = (f.w as f64 * 0.8 / r.width as f64)
            .min(f.h as f64 * 0.8 / r.height as f64)
            .min(1.0);
        let (w, h) = (
            (r.width as f64 * k).round() as i32,
            (r.height as f64 * k).round() as i32,
        );
        let (cx, cy) = f.center();
        let bank = s.ed.doc.add_bank(r);
        let obj = Object::new(
            IRect::new(cx as i32 - w / 2, cy as i32 - h / 2, w.max(1), h.max(1)),
            Data::Image { bank },
        );
        self.apply(
            ui,
            Command::AddObject {
                object: obj,
                select: true,
                merge: None,
            },
        );
        self.set_tool(ui, tool::SELECT);
    }

    pub fn set_color(&mut self, ui: &AppWindow, i: usize) {
        self.color = i.min(PALETTE.len() - 1);
        let ids = self.selection();
        if !ids.is_empty() {
            let patch = ObjectPatch {
                style: Some(StylePatch {
                    color: Some(PALETTE[self.color]),
                    ..Default::default()
                }),
                ..Default::default()
            };
            self.apply(
                ui,
                Command::UpdateObjects {
                    ids,
                    patch,
                    merge: None,
                },
            );
        }
        self.sync(ui);
        ui.window().request_redraw();
    }

    pub fn set_thick(&mut self, ui: &AppWindow, i: usize) {
        self.thick = i.min(THICK.len() - 1);
        // Thickness means the outline for these kinds; for badges and the marker it is a size.
        let ids: Vec<ObjectId> = {
            let Some(s) = self.s.as_ref() else { return };
            s.ed.selection()
                .iter()
                .copied()
                .filter(|id| {
                    s.ed.doc.get(*id).is_some_and(|o| {
                        matches!(
                            o.kind(),
                            Kind::Rect | Kind::Ellipse | Kind::Line | Kind::Pen
                        )
                    })
                })
                .collect()
        };
        if !ids.is_empty() {
            let patch = ObjectPatch {
                style: Some(StylePatch {
                    thick: Some(THICK[self.thick]),
                    ..Default::default()
                }),
                ..Default::default()
            };
            self.apply(
                ui,
                Command::UpdateObjects {
                    ids,
                    patch,
                    merge: None,
                },
            );
        }
        self.sync(ui);
        ui.window().request_redraw();
    }

    // ------------------------------------------------------------------ view

    fn canvas_px(&self, ui: &AppWindow) -> (u32, u32) {
        let w = (ui.get_canvas_width() as f64 * self.dpr).round().max(1.0) as u32;
        let h = (ui.get_canvas_height() as f64 * self.dpr).round().max(1.0) as u32;
        (w.min(16384), h.min(16384))
    }

    fn set_zoom(&mut self, scale: f64, around_out: Option<Point>) {
        let Some(s) = self.s.as_ref() else { return };
        let scale = scale.clamp(0.05, 16.0);
        let f = s.ed.doc.frame();
        match around_out {
            Some(p) => {
                let d = self.view.to_doc(p.x, p.y);
                self.view.scale = scale;
                self.view.origin = Point::new(d.x - p.x / scale, d.y - p.y / scale);
            }
            None => {
                let (cx, cy) = f.center();
                self.view.scale = scale;
                self.view.origin = Point::new(
                    cx - self.view.width as f64 / 2.0 / scale,
                    cy - self.view.height as f64 / 2.0 / scale,
                );
            }
        }
        self.dirty = true;
    }

    /// Fit with a margin; never enlarge beyond 100 % (a small shot stays sharp, LH behaviour).
    fn fit(&mut self) {
        let Some(s) = self.s.as_ref() else { return };
        let f = s.ed.doc.frame();
        let margin = 24.0 * self.dpr;
        let w = (self.view.width as f64 - 2.0 * margin).max(16.0);
        let h = (self.view.height as f64 - 2.0 * margin).max(16.0);
        let k = (w / f.w as f64).min(h / f.h as f64).min(1.0);
        self.set_zoom(k, None);
    }

    pub fn zoom_fit(&mut self, ui: &AppWindow) {
        self.fit();
        self.sync(ui);
        ui.window().request_redraw();
    }

    pub fn zoom_100(&mut self, ui: &AppWindow) {
        // 100 % = one screenshot pixel per physical screen pixel.
        self.set_zoom(1.0, None);
        self.sync(ui);
        ui.window().request_redraw();
    }

    // ------------------------------------------------------------------ output

    /// The document 1:1 as straight RGBA (copy, export).
    pub fn flatten(&mut self) -> Option<(u32, u32, Vec<u8>)> {
        let s = self.s.as_ref()?;
        let mut pix = Pixmap::new(1, 1);
        self.renderer
            .render(&s.ed.doc, View::one_to_one(&s.ed.doc), &mut pix);
        Some((
            pix.width() as u32,
            pix.height() as u32,
            znimok_render::pixmap_to_rgba(&pix),
        ))
    }

    pub fn copy(&mut self, ui: &AppWindow) {
        self.finish_text(ui);
        let Some((w, h, rgba)) = self.flatten() else {
            return;
        };
        let msg = match io::copy_image(w, h, rgba) {
            Ok(()) => self.tr.tr("clipboard-copied"),
            Err(e) => format!("{} ({e})", self.tr.tr("clipboard-error")),
        };
        self.toast(ui, msg);
    }

    pub fn export_to(&mut self, ui: &AppWindow, path: &Path) {
        let Some((w, h, rgba)) = self.flatten() else {
            return;
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let msg = match io::write_image(path, w, h, rgba) {
            Ok(()) => self
                .tr
                .tr_args("export-done-toast", &args(&[("name", name)])),
            Err(e) => format!("{} ({e})", self.tr.tr("export-error")),
        };
        self.toast(ui, msg);
    }

    // ------------------------------------------------------------------ UI state

    pub fn sync(&mut self, ui: &AppWindow) {
        let Some(s) = self.s.as_ref() else { return };
        let doc = &s.ed.doc;
        ui.set_doc_name(doc.name.as_str().into());
        if let QueryResult::State { state } = s.ed.query(&Query::GetState) {
            ui.set_can_undo(state.can_undo);
            ui.set_can_redo(state.can_redo);
        }
        let unsaved = s.ed.is_dirty() || s.save_failed;
        ui.set_save_state(if self.saving {
            2
        } else if unsaved {
            1
        } else {
            0
        });
        let (iw, ih) = doc.image_size();
        ui.set_image_size(format!("{iw} × {ih}").into());
        ui.set_mark_count(doc.objects.len() as i32);
        ui.set_zoom_text(format!("{:.0} %", self.view.scale * 100.0).into());
        let sel = s.ed.selection();
        ui.set_has_selection(!sel.is_empty());
        if let Some(o) = sel.first().and_then(|id| doc.get(*id)) {
            if let Some(i) = PALETTE.iter().position(|c| *c == o.style.color) {
                self.color = i;
            }
            if let Some(i) = THICK.iter().position(|t| *t == o.style.thick) {
                self.thick = i;
            }
        }
        ui.set_color_index(self.color as i32);
        ui.set_thick_index(self.thick as i32);
        ui.set_hint(self.tr.tr(tool::NAMES[self.tool]).into());
        ui.set_autosave(self.autosave);
    }

    // ------------------------------------------------------------------ rendering

    /// Called before Slint draws: renders the document into the canvas texture when needed.
    pub fn before_rendering(&mut self, ui: &AppWindow) {
        if self.s.is_none() {
            return;
        }
        self.dpr = ui.window().scale_factor() as f64;
        let (w, h) = self.canvas_px(ui);
        let size_changed = self
            .gpu
            .as_ref()
            .and_then(|g| g.texture.as_ref())
            .map(|(_, tw, th)| (*tw, *th) != (w, h))
            .unwrap_or(true);
        if size_changed {
            self.view.width = w.min(65535) as u16;
            self.view.height = h.min(65535) as u16;
            self.dirty = true;
        }
        if self.fit_pending && w > 1 && h > 1 {
            self.fit_pending = false;
            self.fit();
            self.sync(ui);
        }
        if !self.dirty {
            return;
        }
        let s = self.s.as_ref().unwrap();
        self.renderer.render(&s.ed.doc, self.view, &mut self.pixmap);
        for id in s.ed.selection() {
            if let Some(o) = s.ed.doc.get(*id) {
                draw_selection(&mut self.pixmap, &self.view, o, self.dpr);
            }
        }
        let Some(gpu) = self.gpu.as_mut() else { return };
        if size_changed || gpu.texture.is_none() {
            let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("znimok canvas"),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            match slint::Image::try_from(tex.clone()) {
                Ok(img) => ui.set_canvas(img),
                Err(e) => eprintln!("canvas texture import failed: {e:?}"),
            }
            gpu.texture = Some((tex, w, h));
        }
        let (tex, _, _) = gpu.texture.as_ref().unwrap();
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            self.pixmap.data_as_u8_slice(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w * 4),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        self.dirty = false;
    }

    pub fn gpu_lost(&mut self) {
        self.gpu = None;
        self.dirty = true;
    }
}

pub enum KeyAction {
    None,
    Copy,
    Export,
    Open,
}

/// Selection outline and handles drawn straight into the pixmap, in screen pixels.
fn draw_selection(pix: &mut Pixmap, view: &View, o: &Object, dpr: f64) {
    let (w, h) = (pix.width() as i64, pix.height() as i64);
    let data = pix.data_mut();
    let mut put = |x: i64, y: i64, c: [u8; 4]| {
        if x >= 0 && y >= 0 && x < w && y < h {
            data[(y * w + x) as usize] = znimok_render::vello_cpu::color::PremulRgba8 {
                r: c[0],
                g: c[1],
                b: c[2],
                a: c[3],
            };
        }
    };
    let blue = [0x3D, 0x7B, 0xF5, 0xFF];
    let white = [0xFF, 0xFF, 0xFF, 0xFF];
    if !o.kind().is_segment() {
        let b = o.bounds();
        let p0 = view.to_out(Point::new(b.x as f64, b.y as f64));
        let p1 = view.to_out(Point::new(b.right() as f64, b.bottom() as f64));
        let (x0, y0, x1, y1) = (
            p0.x.round() as i64,
            p0.y.round() as i64,
            p1.x.round() as i64,
            p1.y.round() as i64,
        );
        for x in x0.max(-1)..=x1.min(w) {
            if (x - x0) % 6 < 3 {
                put(x, y0, blue);
                put(x, y1, blue);
            }
        }
        for y in y0.max(-1)..=y1.min(h) {
            if (y - y0) % 6 < 3 {
                put(x0, y, blue);
                put(x1, y, blue);
            }
        }
    }
    let hs = (4.0 * dpr).round() as i64;
    for (hx, hy) in hit::handles(o) {
        let p = view.to_out(Point::new(hx, hy));
        let (cx, cy) = (p.x.round() as i64, p.y.round() as i64);
        for y in -hs..=hs {
            for x in -hs..=hs {
                let edge = x.abs() == hs || y.abs() == hs;
                put(cx + x, cy + y, if edge { blue } else { white });
            }
        }
    }
}
