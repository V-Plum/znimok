//! Application state: the library, the open document and the canvas.
//! Every change to a document goes through `znimok_core::Editor::apply` — the same commands the
//! CLI and the agents use (ZK-24), so undo, dirty tracking and validation are the core's.

use std::path::{Path, PathBuf};
use std::time::Instant;

use chrono::TimeZone;
use slint::wgpu_30::wgpu;
use slint::{ComponentHandle, SharedString, VecModel};
use znimok_core::command::{AlignEdge, Arrange, Axis};
use znimok_core::hit;
use znimok_core::*;
use znimok_i18n::{FluentArgs, Localizer};
use znimok_render::vello_cpu::Pixmap;
use znimok_render::vello_cpu::kurbo::Point;
use znimok_render::{Renderer, View};

use crate::library::{self, Entry};
use crate::{AppWindow, CardData, LayerRow, io};

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
    /// Rubber-band selection with the Select tool from empty space (ZK-52).
    Marquee {
        start: (i32, i32),
        add: bool,
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
    /// Defaults for new marks (and what the inspector changes on a selection), ZK-54.
    alpha: u8,
    fill: Option<usize>,
    dash: Dash,
    corners: Corners,
    head_start: Head,
    head_end: Head,
    text_size_i: usize,
    bold: bool,
    italic: bool,
    /// One undo step per drag of the opacity slider.
    alpha_merge: Option<MergeKey>,
    drag: Option<Drag>,
    next_merge: u64,
    editing: Option<Editing>,
    pub autosave: bool,
    pub saving: bool,
    toast_at: Option<Instant>,
    /// Zoom / fit animation in progress (ease-in-out, about a fixed screen point).
    anim: Option<ViewAnim>,
    anim_timer: slint::Timer,
    /// Rubber band being dragged, document pixels (drawn over the render).
    marquee: Option<IRect>,
    /// Last pointer position on the canvas, output pixels (anchor for pinch and double tap).
    last_out: Point,
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
            alpha: 100,
            fill: None,
            dash: Dash::Solid,
            corners: Corners::Sharp,
            head_start: Head::None,
            head_end: Head::Triangle,
            text_size_i: 1,
            bold: false,
            italic: false,
            alpha_merge: None,
            drag: None,
            next_merge: 1,
            editing: None,
            autosave: true,
            saving: false,
            toast_at: None,
            anim: None,
            anim_timer: slint::Timer::default(),
            marquee: None,
            last_out: Point::ZERO,
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
    fn build_document(
        &self,
        raster: Raster,
        source: &str,
        name: Option<String>,
    ) -> (Document, PathBuf) {
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
        (doc, path)
    }

    /// A new library document from pixels (screenshot, clipboard, image file), opened in the editor.
    pub fn new_document(
        &mut self,
        ui: &AppWindow,
        raster: Raster,
        source: &str,
        name: Option<String>,
    ) {
        let (doc, path) = self.build_document(raster, source, name);
        // Not on disk yet: `fresh` makes the first autosave write it.
        self.open_session(ui, Editor::new(doc), path, true);
    }

    /// Straight to the library without opening the editor (Shift in the capture overlay).
    pub fn store_quietly(
        &mut self,
        ui: &AppWindow,
        raster: Raster,
        source: &str,
    ) -> Result<(), String> {
        let (doc, path) = self.build_document(raster, source, None);
        let opts = self.options_for(&doc);
        let _guard = SAVE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _ = std::fs::create_dir_all(&self.lib_dir);
        let r = znimok_format::save(&path, &doc, &opts)
            .map_err(|e| format!("{} ({e})", self.tr.tr("err-library-save")));
        if self.s.is_none() {
            self.refresh_library(ui);
        }
        r
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
        self.show_meta(ui);
        self.sync(ui);
        ui.window().request_redraw();
    }

    /// Meta tab fields — set when a document opens, not on every sync (it would move the cursor
    /// while typing).
    fn show_meta(&self, ui: &AppWindow) {
        let Some(s) = self.s.as_ref() else { return };
        let d = &s.ed.doc;
        ui.set_meta_title(d.name.as_str().into());
        ui.set_meta_description(d.meta.description.as_str().into());
        ui.set_meta_author(d.meta.author.as_str().into());
        ui.set_meta_rights(d.meta.copyright.as_str().into());
        ui.set_meta_tags(d.meta.tags.join(", ").into());
    }

    /// Meta tab edits: the title is the document name; the rest is `Meta`. Not undo steps.
    pub fn meta_edited(&mut self, ui: &AppWindow, field: &str, value: &str) {
        let Some(s) = self.s.as_ref() else { return };
        let cmd = if field == "title" {
            Command::SetName {
                name: value.trim().to_string(),
            }
        } else {
            let mut meta = s.ed.doc.meta.clone();
            match field {
                "description" => meta.description = value.to_string(),
                "author" => meta.author = value.trim().to_string(),
                "rights" => meta.copyright = value.trim().to_string(),
                "tags" => {
                    meta.tags = value
                        .split(',')
                        .map(|t| t.trim().to_string())
                        .filter(|t| !t.is_empty())
                        .collect()
                }
                _ => return,
            }
            Command::SetMeta { meta }
        };
        self.apply(ui, cmd);
        self.sync(ui);
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
        let doc = self.s.as_ref()?.ed.doc.clone();
        Some(self.options_for(&doc))
    }

    /// Thumbnail and write options for saving `doc`.
    fn options_for(&mut self, doc: &Document) -> znimok_format::WriteOptions {
        let f = doc.frame();
        let k = (320.0 / f.w as f64).min(240.0 / f.h as f64).min(1.0);
        let mut view = View::one_to_one(doc);
        view.scale = k;
        view.width = ((f.w as f64 * k).round() as i64).clamp(1, 320) as u16;
        view.height = ((f.h as f64 * k).round() as i64).clamp(1, 240) as u16;
        let mut pix = Pixmap::new(1, 1);
        self.renderer.render(doc, view, &mut pix);
        znimok_format::WriteOptions {
            app_version: format!("Znimok {}", env!("CARGO_PKG_VERSION")),
            thumbnail: Some(Raster::new(
                pix.width() as u32,
                pix.height() as u32,
                znimok_render::pixmap_to_rgba(&pix),
            )),
            ..Default::default()
        }
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
            alpha: self.alpha,
            color2: if matches!(t, tool::RECT | tool::ELLIPSE) {
                self.fill.map(|i| PALETTE[i])
            } else {
                None
            },
            dash: if matches!(t, tool::RECT | tool::ELLIPSE | tool::ARROW | tool::PEN) {
                self.dash
            } else {
                Dash::Solid
            },
            corners: if t == tool::RECT {
                self.corners
            } else {
                Corners::Sharp
            },
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

    /// Text sizes S / M / L are 18 / 24 / 36 px on a 1080 px tall picture, scaled up for taller.
    fn text_size_base(&self) -> f64 {
        let h = self.s.as_ref().map(|s| s.ed.doc.frame().h).unwrap_or(1080);
        (h as f64 / 1080.0).max(1.0)
    }

    fn text_size(&self) -> i32 {
        let base = [18, 24, 36][self.text_size_i.min(2)];
        ((base as f64 * self.text_size_base()).round() as i32).max(10)
    }

    fn new_object(&self, t: usize, a: (i32, i32), b: (i32, i32)) -> Object {
        let rect = IRect::new(a.0, a.1, b.0 - a.0, b.1 - a.1);
        let data = match t {
            tool::RECT => Data::Rect,
            tool::ELLIPSE => Data::Ellipse,
            tool::ARROW => Data::Line {
                head_front: self.head_end,
                head_back: self.head_start,
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

    /// `ctrl` held turns any tool into Select for the duration (owner, 28.09): Ctrl+click
    /// toggles a mark in the selection, Ctrl+drag on empty space adds with a rubber band.
    #[allow(clippy::too_many_arguments)]
    pub fn pointer(
        &mut self,
        ui: &AppWindow,
        kind: i32,
        x: f32,
        y: f32,
        button: i32,
        shift: bool,
        ctrl: bool,
    ) {
        if self.s.is_none() {
            return;
        }
        let (out, p) = self.to_doc(x, y);
        self.last_out = out;
        if kind == 0 {
            self.stop_anim();
        }
        match kind {
            0 => self.pointer_down(ui, out, p, button, shift, ctrl),
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
        ctrl: bool,
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
        let tool = if ctrl { tool::SELECT } else { self.tool };
        let extend = shift || ctrl;
        match tool {
            tool::SELECT => match hit_id {
                Some(id) if extend => {
                    // Toggle: a selected mark leaves the selection, another one joins it.
                    if sel.contains(&id) {
                        let rest: Vec<ObjectId> = sel.into_iter().filter(|s| *s != id).collect();
                        if rest.is_empty() {
                            self.apply(ui, Command::ClearSelection);
                        } else {
                            self.apply(
                                ui,
                                Command::Select {
                                    ids: rest,
                                    add: false,
                                },
                            );
                        }
                    } else {
                        self.apply(
                            ui,
                            Command::Select {
                                ids: vec![id],
                                add: true,
                            },
                        );
                    }
                }
                Some(id) => {
                    if !sel.contains(&id) {
                        self.apply(
                            ui,
                            Command::Select {
                                ids: vec![id],
                                add: false,
                            },
                        );
                    }
                    let merge = self.merge_key();
                    self.drag = Some(Drag::Move { last: p, merge });
                }
                None => {
                    if !extend {
                        self.apply(ui, Command::ClearSelection);
                    }
                    self.drag = Some(Drag::Marquee {
                        start: p,
                        add: extend,
                    });
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
            Drag::Marquee { start, add } => {
                let r = IRect::new(start.0, start.1, p.0 - start.0, p.1 - start.1).normalized();
                self.marquee = Some(r);
                let ids: Vec<ObjectId> = {
                    let doc = &self.s.as_ref().unwrap().ed.doc;
                    hit::pick_in_rect(doc, r)
                        .into_iter()
                        .map(|i| doc.objects[i].id)
                        .collect()
                };
                let current = self.selection();
                if add {
                    let missing: Vec<ObjectId> =
                        ids.into_iter().filter(|id| !current.contains(id)).collect();
                    if !missing.is_empty() {
                        self.apply(
                            ui,
                            Command::Select {
                                ids: missing,
                                add: true,
                            },
                        );
                    }
                } else if ids != current {
                    if ids.is_empty() {
                        self.apply(ui, Command::ClearSelection);
                    } else {
                        self.apply(ui, Command::Select { ids, add: false });
                    }
                }
                self.dirty = true;
            }
            Drag::Pan { start_out, orig } => {
                let sc = self.view.scale;
                self.view.origin = Point::new(
                    orig.x - (out.x - start_out.x) / sc,
                    orig.y - (out.y - start_out.y) / sc,
                );
                self.constrain();
                self.dirty = true;
            }
        }
    }

    fn pointer_up(&mut self, _ui: &AppWindow) {
        self.drag = None;
        self.marquee = None;
        self.dirty = true;
    }

    // ------------------------------------------------------------------ arrange (ZK-52)

    /// Copies of the selected marks, offset a little, become the new selection — one undo step.
    pub fn duplicate(&mut self, ui: &AppWindow) {
        self.finish_text(ui);
        let ids = self.selection();
        if ids.is_empty() {
            return;
        }
        let copies: Vec<Object> = {
            let doc = &self.s.as_ref().unwrap().ed.doc;
            ids.iter()
                .filter_map(|id| doc.get(*id))
                .map(|o| {
                    let mut c = o.clone();
                    c.translate(16, 16);
                    c
                })
                .collect()
        };
        let merge = self.merge_key();
        let mut new_ids = Vec::new();
        for object in copies {
            if let Some(a) = self.apply(
                ui,
                Command::AddObject {
                    object,
                    select: false,
                    merge: Some(merge.clone()),
                },
            ) && let Some(id) = a.created
            {
                new_ids.push(id);
            }
        }
        if !new_ids.is_empty() {
            self.apply(
                ui,
                Command::Select {
                    ids: new_ids,
                    add: false,
                },
            );
        }
        self.sync(ui);
        ui.window().request_redraw();
    }

    /// 0 front, 1 back, 2 forward, 3 backward.
    pub fn arrange(&mut self, ui: &AppWindow, to: i32) {
        let ids = self.selection();
        if ids.is_empty() {
            return;
        }
        let to = match to {
            0 => Arrange::Front,
            1 => Arrange::Back,
            2 => Arrange::Forward,
            _ => Arrange::Backward,
        };
        self.apply(ui, Command::Arrange { ids, to });
        self.sync(ui);
        ui.window().request_redraw();
    }

    /// 0 left, 1 h-centre, 2 right, 3 top, 4 v-centre, 5 bottom.
    pub fn align(&mut self, ui: &AppWindow, edge: i32) {
        let ids = self.selection();
        if ids.is_empty() {
            return;
        }
        let edge = match edge {
            0 => AlignEdge::Left,
            1 => AlignEdge::HCenter,
            2 => AlignEdge::Right,
            3 => AlignEdge::Top,
            4 => AlignEdge::VCenter,
            _ => AlignEdge::Bottom,
        };
        self.apply(ui, Command::Align { ids, edge });
        self.sync(ui);
        ui.window().request_redraw();
    }

    /// 0 across, 1 down.
    pub fn distribute(&mut self, ui: &AppWindow, axis: i32) {
        let ids = self.selection();
        if ids.len() < 3 {
            return;
        }
        let axis = if axis == 0 {
            Axis::Horizontal
        } else {
            Axis::Vertical
        };
        self.apply(ui, Command::Distribute { ids, axis });
        self.sync(ui);
        ui.window().request_redraw();
    }

    pub fn group(&mut self, ui: &AppWindow, group: bool) {
        let ids = self.selection();
        if ids.is_empty() {
            return;
        }
        let cmd = if group {
            Command::Group { ids }
        } else {
            Command::Ungroup { ids }
        };
        self.apply(ui, cmd);
        self.sync(ui);
        ui.window().request_redraw();
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
                            bold: self.bold,
                            italic: self.italic,
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

    /// Wheel and trackpad. With Ctrl (⌘ on macOS) or Alt it zooms about the pointer: a mouse
    /// notch is one animated step, a trackpad's small deltas (and Windows' pinch, which arrives
    /// as Ctrl+wheel) zoom smoothly in proportion. Without a modifier it scrolls both ways —
    /// two-finger trackpad scrolling, Shift+wheel for sideways with a mouse (as in LH).
    #[allow(clippy::too_many_arguments)]
    pub fn wheel(
        &mut self,
        ui: &AppWindow,
        x: f32,
        y: f32,
        dx: f32,
        dy: f32,
        zoom: bool,
        shift: bool,
    ) {
        if self.s.is_none() {
            return;
        }
        let out = Point::new(x as f64 * self.dpr, y as f64 * self.dpr);
        self.last_out = out;
        if zoom {
            if dy.abs() >= 40.0 {
                // A mouse notch: continue from where a running animation is heading.
                let from = self.anim.as_ref().map_or(self.view.scale, |a| a.s1);
                let target = from * if dy > 0.0 { 1.25 } else { 1.0 / 1.25 };
                self.animate_zoom(target, Some(out));
            } else {
                self.stop_anim();
                let factor = 1.0025_f64.powf(dy as f64);
                self.set_zoom(self.view.scale * factor, Some(out));
            }
        } else {
            self.stop_anim();
            let (dx, dy) = if shift && dx == 0.0 {
                (dy, 0.0)
            } else {
                (dx, dy)
            };
            self.view.origin.x -= dx as f64 * self.dpr / self.view.scale;
            self.view.origin.y -= dy as f64 * self.dpr / self.view.scale;
            self.constrain();
            self.dirty = true;
        }
        self.sync(ui);
        ui.window().request_redraw();
    }

    /// macOS trackpad pinch: `delta` is the change of magnification (+0.02 = 2 % bigger).
    pub fn pinch(&mut self, ui: &AppWindow, delta: f64) {
        if self.s.is_none() {
            return;
        }
        self.stop_anim();
        let out = self.last_out;
        self.set_zoom(self.view.scale * (1.0 + delta).max(0.2), Some(out));
        self.sync(ui);
        ui.window().request_redraw();
    }

    /// Trackpad double tap ("smart zoom"): fit ↔ 100 % about the pointer.
    pub fn smart_zoom(&mut self, ui: &AppWindow) {
        if self.s.is_none() {
            return;
        }
        let fit = self.fit_scale();
        if (self.view.scale - fit).abs() < 0.01 {
            self.animate_zoom(1.0, Some(self.last_out));
        } else {
            self.zoom_fit(ui);
        }
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
        const PAIRS: [(char, char); 20] = [
            ('d', 'в'),
            ('g', 'п'),
            ('[', 'х'),
            (']', 'ї'),
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
                Some('d') => self.duplicate(ui),
                Some('g') if shift => self.group(ui, false),
                Some('g') => self.group(ui, true),
                Some(']') if shift => self.arrange(ui, 0),
                Some(']') => self.arrange(ui, 2),
                Some('[') if shift => self.arrange(ui, 1),
                Some('[') => self.arrange(ui, 3),
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
        self.set_prop(ui, "color", i as i32);
    }

    /// Selected ids whose kind `ok` accepts.
    fn selected_where(&self, ok: impl Fn(&Object) -> bool) -> Vec<ObjectId> {
        let Some(s) = self.s.as_ref() else {
            return Vec::new();
        };
        s.ed.selection()
            .iter()
            .copied()
            .filter(|id| s.ed.doc.get(*id).is_some_and(&ok))
            .collect()
    }

    fn patch_selected(
        &mut self,
        ui: &AppWindow,
        ok: impl Fn(&Object) -> bool,
        patch: ObjectPatch,
        merge: Option<MergeKey>,
    ) {
        let ids = self.selected_where(ok);
        if !ids.is_empty() {
            self.apply(ui, Command::UpdateObjects { ids, patch, merge });
        }
    }

    /// One inspector control (ZK-54): changes the default for new marks and, when marks are
    /// selected, those of them it applies to — one undo step per click.
    pub fn set_prop(&mut self, ui: &AppWindow, name: &str, v: i32) {
        let outline = |o: &Object| {
            matches!(
                o.kind(),
                Kind::Rect | Kind::Ellipse | Kind::Line | Kind::Pen
            )
        };
        let style = |sp: StylePatch| ObjectPatch {
            style: Some(sp),
            ..Default::default()
        };
        match name {
            "color" => {
                self.color = (v.max(0) as usize).min(PALETTE.len() - 1);
                let c = PALETTE[self.color];
                self.patch_selected(
                    ui,
                    |o| !matches!(o.kind(), Kind::Hide | Kind::Image),
                    style(StylePatch {
                        color: Some(c),
                        ..Default::default()
                    }),
                    None,
                );
            }
            "thick" => {
                self.thick = (v.max(0) as usize).min(THICK.len() - 1);
                let t = THICK[self.thick];
                self.patch_selected(
                    ui,
                    outline,
                    style(StylePatch {
                        thick: Some(t),
                        ..Default::default()
                    }),
                    None,
                );
            }
            "fill" => {
                self.fill = (v >= 0).then(|| (v as usize).min(PALETTE.len() - 1));
                let c2 = self.fill.map(|i| PALETTE[i]);
                self.patch_selected(
                    ui,
                    |o| matches!(o.kind(), Kind::Rect | Kind::Ellipse),
                    style(StylePatch {
                        color2: Some(c2),
                        ..Default::default()
                    }),
                    None,
                );
            }
            "dash" => {
                self.dash = match v {
                    1 => Dash::Dashed,
                    2 => Dash::DashDot,
                    _ => Dash::Solid,
                };
                let d = self.dash;
                self.patch_selected(
                    ui,
                    outline,
                    style(StylePatch {
                        dash: Some(d),
                        ..Default::default()
                    }),
                    None,
                );
            }
            "corners" => {
                self.corners = match v {
                    1 => Corners::Soft,
                    2 => Corners::Round,
                    _ => Corners::Sharp,
                };
                let c = self.corners;
                self.patch_selected(
                    ui,
                    |o| o.kind() == Kind::Rect,
                    style(StylePatch {
                        corners: Some(c),
                        corner_px: Some(0),
                        ..Default::default()
                    }),
                    None,
                );
            }
            "head-start" | "head-end" => {
                let h = match v {
                    1 => Head::Triangle,
                    2 => Head::Chevron,
                    3 => Head::Dot,
                    _ => Head::None,
                };
                let start = name == "head-start";
                if start {
                    self.head_start = h;
                } else {
                    self.head_end = h;
                }
                // Heads are line data: each selected line gets its own patch.
                let lines: Vec<(ObjectId, Data)> = {
                    let Some(s) = self.s.as_ref() else { return };
                    s.ed.selection()
                        .iter()
                        .filter_map(|id| s.ed.doc.get(*id))
                        .filter_map(|o| match o.data {
                            Data::Line {
                                head_front,
                                head_back,
                                head_size,
                            } => Some((
                                o.id,
                                Data::Line {
                                    head_front: if start { head_front } else { h },
                                    head_back: if start { h } else { head_back },
                                    head_size,
                                },
                            )),
                            _ => None,
                        })
                        .collect()
                };
                let merge = (lines.len() > 1).then(|| self.merge_key());
                for (id, data) in lines {
                    self.apply(
                        ui,
                        Command::UpdateObjects {
                            ids: vec![id],
                            patch: ObjectPatch {
                                data: Some(data),
                                ..Default::default()
                            },
                            merge: merge.clone(),
                        },
                    );
                }
            }
            "text-size" | "bold" | "italic" => {
                match name {
                    "text-size" => self.text_size_i = (v.max(0) as usize).min(2),
                    "bold" => self.bold = v != 0,
                    _ => self.italic = v != 0,
                }
                let patch = match name {
                    "text-size" => ObjectPatch {
                        size: Some(self.text_size()),
                        ..Default::default()
                    },
                    "bold" => ObjectPatch {
                        bold: Some(self.bold),
                        ..Default::default()
                    },
                    _ => ObjectPatch {
                        italic: Some(self.italic),
                        ..Default::default()
                    },
                };
                let ids = self.selected_where(|o| o.kind() == Kind::Text);
                if !ids.is_empty() {
                    let merge = self.merge_key();
                    self.apply(
                        ui,
                        Command::UpdateObjects {
                            ids: ids.clone(),
                            patch,
                            merge: Some(merge.clone()),
                        },
                    );
                    for id in ids {
                        self.fit_text(ui, id, Some(merge.clone()));
                    }
                }
            }
            _ => {}
        }
        self.sync(ui);
        ui.window().request_redraw();
    }

    /// Opacity slider: one undo step per drag (`last` = the knob was released).
    pub fn set_alpha(&mut self, ui: &AppWindow, v: f32, last: bool) {
        self.alpha = ((v * 100.0).round() as i32).clamp(10, 100) as u8;
        let merge = match &self.alpha_merge {
            Some(m) => m.clone(),
            None => {
                let m = self.merge_key();
                self.alpha_merge = Some(m.clone());
                m
            }
        };
        let a = self.alpha;
        self.patch_selected(
            ui,
            |o| o.kind() != Kind::Hide,
            ObjectPatch {
                style: Some(StylePatch {
                    alpha: Some(a),
                    ..Default::default()
                }),
                ..Default::default()
            },
            Some(merge),
        );
        if last {
            self.alpha_merge = None;
        }
        self.sync(ui);
        ui.window().request_redraw();
    }

    /// X / Y move the mark (pen points too), W / H resize it.
    pub fn set_geom(&mut self, ui: &AppWindow, field: &str, text: &str) {
        let Ok(v) = text.trim().parse::<i32>() else {
            self.sync(ui);
            return;
        };
        let Some((id, r)) = self
            .s
            .as_ref()
            .and_then(|s| s.ed.selection().first().copied())
            .and_then(|id| self.s.as_ref()?.ed.doc.get(id).map(|o| (id, o.rect)))
        else {
            return;
        };
        let cmd = match field {
            "x" => Command::MoveObjects {
                ids: vec![id],
                dx: v - r.x,
                dy: 0,
                merge: None,
            },
            "y" => Command::MoveObjects {
                ids: vec![id],
                dx: 0,
                dy: v - r.y,
                merge: None,
            },
            "w" | "h" => {
                let mut n = r;
                if field == "w" {
                    n.w = v.max(1);
                } else {
                    n.h = v.max(1);
                }
                Command::UpdateObjects {
                    ids: vec![id],
                    patch: ObjectPatch {
                        rect: Some(n),
                        ..Default::default()
                    },
                    merge: None,
                }
            }
            _ => return,
        };
        self.apply(ui, cmd);
        self.sync(ui);
        ui.window().request_redraw();
    }

    /// Layers list: click selects (Shift/Ctrl toggles), the eye hides and shows.
    pub fn layer_click(&mut self, ui: &AppWindow, id: i32, add: bool) {
        let id = id as ObjectId;
        let sel = self.selection();
        let cmd = if add && sel.contains(&id) {
            let rest: Vec<ObjectId> = sel.into_iter().filter(|s| *s != id).collect();
            if rest.is_empty() {
                Command::ClearSelection
            } else {
                Command::Select {
                    ids: rest,
                    add: false,
                }
            }
        } else {
            Command::Select { ids: vec![id], add }
        };
        self.apply(ui, cmd);
        self.sync(ui);
        ui.window().request_redraw();
    }

    pub fn layer_eye(&mut self, ui: &AppWindow, id: i32) {
        let id = id as ObjectId;
        let Some(hidden) = self
            .s
            .as_ref()
            .and_then(|s| s.ed.doc.get(id))
            .map(|o| o.hidden)
        else {
            return;
        };
        self.apply(
            ui,
            Command::UpdateObjects {
                ids: vec![id],
                patch: ObjectPatch {
                    hidden: Some(!hidden),
                    ..Default::default()
                },
                merge: None,
            },
        );
        self.sync(ui);
        ui.window().request_redraw();
    }

    /// Zoom slider: 0…1 ↔ 6.25 %…1600 % on a log scale, about the canvas centre.
    pub fn zoom_to(&mut self, ui: &AppWindow, pos: f32) {
        if self.s.is_none() {
            return;
        }
        self.stop_anim();
        let scale = 2f64.powf(pos.clamp(0.0, 1.0) as f64 * 8.0 - 4.0);
        let c = Point::new(self.view.width as f64 / 2.0, self.view.height as f64 / 2.0);
        self.set_zoom(scale, Some(c));
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
        self.constrain();
        self.dirty = true;
    }

    /// Keeps the picture in view (owner, 28.09): smaller than the canvas → centred on that
    /// axis; bigger → it may not be pulled away from an edge further than a small margin.
    fn constrain(&mut self) {
        let Some(s) = self.s.as_ref() else { return };
        let f = s.ed.doc.frame();
        let sc = self.view.scale.max(1e-6);
        let (vw, vh) = (self.view.width as f64 / sc, self.view.height as f64 / sc);
        let m = 24.0 * self.dpr / sc;
        let axis = |o: f64, start: f64, len: f64, view: f64| -> f64 {
            if len + 2.0 * m <= view {
                start + len / 2.0 - view / 2.0
            } else {
                o.clamp(start - m, start + len - view + m)
            }
        };
        self.view.origin.x = axis(self.view.origin.x, f.x as f64, f.w as f64, vw);
        self.view.origin.y = axis(self.view.origin.y, f.y as f64, f.h as f64, vh);
    }

    /// Fit with a margin; never enlarge beyond 100 % (a small shot stays sharp, LH behaviour).
    fn fit_scale(&self) -> f64 {
        let Some(s) = self.s.as_ref() else { return 1.0 };
        let f = s.ed.doc.frame();
        let margin = 24.0 * self.dpr;
        let w = (self.view.width as f64 - 2.0 * margin).max(16.0);
        let h = (self.view.height as f64 - 2.0 * margin).max(16.0);
        (w / f.w as f64).min(h / f.h as f64).min(1.0)
    }

    fn fit(&mut self) {
        let k = self.fit_scale();
        self.set_zoom(k, None);
    }

    pub fn zoom_fit(&mut self, ui: &AppWindow) {
        let k = self.fit_scale();
        self.animate_zoom(k, None);
        self.sync(ui);
    }

    pub fn zoom_100(&mut self, ui: &AppWindow) {
        // 100 % = one screenshot pixel per physical screen pixel.
        self.animate_zoom(1.0, None);
        self.sync(ui);
    }

    /// Animates the view to `scale` about `around` (output pixels; `None` = the picture's
    /// centre): 220 ms, ease-in-out, the zoom anchored at the point both ends share, so the
    /// picture grows or shrinks "about" a fixed spot like on iOS.
    fn animate_zoom(&mut self, scale: f64, around: Option<Point>) {
        let (s0, o0) = (self.view.scale, self.view.origin);
        self.set_zoom(scale, around);
        let (s1, o1) = (self.view.scale, self.view.origin);
        self.view.scale = s0;
        self.view.origin = o0;
        if (s1 - s0).abs() < 1e-6 && (o1 - o0).hypot() < 1e-3 {
            return;
        }
        self.anim = Some(ViewAnim {
            s0,
            s1,
            o0,
            o1,
            start: Instant::now(),
        });
        if !self.anim_timer.running() {
            self.anim_timer.start(
                slint::TimerMode::Repeated,
                std::time::Duration::from_millis(16),
                || crate::with_ctx(|a, ui| a.tick_anim(ui)),
            );
        }
    }

    fn stop_anim(&mut self) {
        if let Some(a) = self.anim.take() {
            // Jump to where it was heading, so the next gesture starts from a settled view.
            self.view.scale = a.s1;
            self.view.origin = a.o1;
            self.dirty = true;
        }
        self.anim_timer.stop();
    }

    /// For the self-test: scale, and where the picture's centre sits on the canvas relative to
    /// the canvas centre (output pixels).
    pub fn view_probe(&self) -> (f64, f64, f64, f64) {
        let Some(s) = self.s.as_ref() else {
            return (0.0, 0.0, 0.0, 0.0);
        };
        let (cx, cy) = s.ed.doc.frame().center();
        let p = self.view.to_out(Point::new(cx, cy));
        (
            self.view.scale,
            p.x - self.view.width as f64 / 2.0,
            p.y - self.view.height as f64 / 2.0,
            self.view.origin.x,
        )
    }

    pub fn tick_anim(&mut self, ui: &AppWindow) {
        let Some(a) = self.anim.as_ref() else {
            self.anim_timer.stop();
            return;
        };
        let t = (a.start.elapsed().as_secs_f64() / 0.22).min(1.0);
        let (scale, origin) = a.at(t);
        self.view.scale = scale;
        self.view.origin = origin;
        if t >= 1.0 {
            self.anim = None;
            self.anim_timer.stop();
        }
        self.dirty = true;
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
        ui.set_selection_count(sel.len() as i32);
        // Inspector: the primary selected mark, or the current tool's defaults.
        let primary = sel.first().and_then(|id| doc.get(*id));
        let kind = match primary {
            Some(o) => kind_index(o.kind()),
            None => tool_kind(self.tool),
        };
        ui.set_prop_kind(kind);
        ui.set_prop_for_selection(primary.is_some());
        let tool_name = |k: i32| -> String {
            let id = match k {
                0 => "tool-rect-name",
                1 => "tool-ellipse-name",
                2 => "tool-line-name",
                3 => "tool-pen-name",
                4 => "tool-text-name",
                5 => "tool-hide-name",
                6 => "tool-highlighter-name",
                7 => "tool-counter-name",
                8 => "tool-stamp-name",
                _ => "tool-image-name",
            };
            self.tr.tr(id)
        };
        match primary {
            Some(o) => {
                let title = o.name.clone().unwrap_or_else(|| tool_name(kind));
                ui.set_prop_title(title.into());
                let st = &o.style;
                ui.set_color_index(
                    PALETTE
                        .iter()
                        .position(|c| *c == st.color)
                        .map_or(-1, |i| i as i32),
                );
                ui.set_thick_index(
                    THICK
                        .iter()
                        .position(|t| *t == st.thick)
                        .map_or(-1, |i| i as i32),
                );
                ui.set_fill_index(
                    st.color2
                        .and_then(|c| PALETTE.iter().position(|p| *p == c))
                        .map_or(-1, |i| i as i32),
                );
                ui.set_dash_index(dash_index(st.dash));
                ui.set_corners_index(corners_index(st.corners));
                ui.set_alpha(st.alpha as f32 / 100.0);
                if let Data::Line {
                    head_front,
                    head_back,
                    ..
                } = o.data
                {
                    ui.set_head_start(head_index(head_back));
                    ui.set_head_end(head_index(head_front));
                }
                if let Data::Text {
                    size, bold, italic, ..
                } = o.data
                {
                    let base = self.text_size_base();
                    let i = [18, 24, 36]
                        .iter()
                        .position(|b| ((*b as f64 * base).round() as i32).max(10) == size)
                        .map_or(-1, |i| i as i32);
                    ui.set_text_size_index(i);
                    ui.set_text_bold(bold);
                    ui.set_text_italic(italic);
                }
                ui.set_geom_x(o.rect.x.to_string().into());
                ui.set_geom_y(o.rect.y.to_string().into());
                ui.set_geom_w(o.rect.w.to_string().into());
                ui.set_geom_h(o.rect.h.to_string().into());
            }
            None => {
                ui.set_prop_title(
                    if kind >= 0 {
                        tool_name(kind)
                    } else {
                        String::new()
                    }
                    .into(),
                );
                ui.set_color_index(self.color as i32);
                ui.set_thick_index(self.thick as i32);
                ui.set_fill_index(self.fill.map_or(-1, |i| i as i32));
                ui.set_dash_index(dash_index(self.dash));
                ui.set_corners_index(corners_index(self.corners));
                ui.set_alpha(self.alpha as f32 / 100.0);
                ui.set_head_start(head_index(self.head_start));
                ui.set_head_end(head_index(self.head_end));
                ui.set_text_size_index(self.text_size_i as i32);
                ui.set_text_bold(self.bold);
                ui.set_text_italic(self.italic);
            }
        }
        // Layers: front first; unnamed marks are "<kind> <n>", numbered per kind bottom-up.
        let mut counts = [0usize; 10];
        let mut rows: Vec<LayerRow> = doc
            .objects
            .iter()
            .map(|o| {
                let k = kind_index(o.kind());
                counts[k as usize] += 1;
                LayerRow {
                    id: o.id as i32,
                    name: o
                        .name
                        .clone()
                        .unwrap_or_else(|| format!("{} {}", tool_name(k), counts[k as usize]))
                        .into(),
                    kind: k,
                    hidden: o.hidden,
                    selected: sel.contains(&o.id),
                }
            })
            .collect();
        rows.reverse();
        ui.set_layers(std::rc::Rc::new(VecModel::from(rows)).into());
        // Image tab
        ui.set_info_size(format!("{iw} × {ih}").into());
        let source = match doc.meta.source.as_str() {
            "region" => self.tr.tr("shot-source-region"),
            "clipboard" => self.tr.tr("shot-source-clipboard"),
            "window" => self.tr.tr("capture-window"),
            "screen" => self.tr.tr("capture-whole-screen"),
            "file" => self
                .tr
                .tr_args("shot-source-file", &args(&[("name", doc.name.clone())])),
            other => other.to_string(),
        };
        ui.set_info_source(source.into());
        let taken = chrono::Local
            .timestamp_millis_opt(doc.meta.created_ms)
            .single()
            .map(|t| t.format("%d.%m.%Y %H:%M:%S").to_string())
            .unwrap_or_default();
        ui.set_info_taken(taken.into());
        ui.set_info_path(s.path.display().to_string().into());
        // Zoom slider: log2 scale −4…+4 → 0…1
        ui.set_zoom_pos((((self.view.scale.log2() + 4.0) / 8.0).clamp(0.0, 1.0)) as f32);
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
            // Keep the doc point at the centre of the canvas where it was, then re-centre /
            // clamp for the new size (owner, 28.09: a picture smaller than the window stays
            // centred while the window is resized).
            let c = self
                .view
                .to_doc(self.view.width as f64 / 2.0, self.view.height as f64 / 2.0);
            self.view.width = w.min(65535) as u16;
            self.view.height = h.min(65535) as u16;
            let sc = self.view.scale.max(1e-6);
            self.view.origin = Point::new(c.x - w as f64 / 2.0 / sc, c.y - h as f64 / 2.0 / sc);
            if !self.fit_pending {
                self.constrain();
            }
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
        if let Some(m) = self.marquee {
            draw_marquee(&mut self.pixmap, &self.view, m);
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

/// One zoom / fit animation. Both ends are exact; in between the scale moves geometrically and
/// the origin keeps the point both ends share fixed on screen.
struct ViewAnim {
    s0: f64,
    s1: f64,
    o0: Point,
    o1: Point,
    start: Instant,
}

impl ViewAnim {
    fn at(&self, t: f64) -> (f64, Point) {
        // ease-in-out (cubic)
        let e = if t < 0.5 {
            4.0 * t * t * t
        } else {
            1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
        };
        let scale = (self.s0.ln() + (self.s1.ln() - self.s0.ln()) * e).exp();
        let inv = 1.0 / self.s1 - 1.0 / self.s0;
        let origin = if inv.abs() < 1e-9 {
            Point::new(
                self.o0.x + (self.o1.x - self.o0.x) * e,
                self.o0.y + (self.o1.y - self.o0.y) * e,
            )
        } else {
            // Fixed screen point p and doc point d: o(s) = d - p / s at both ends.
            let px = (self.o0.x - self.o1.x) / inv;
            let py = (self.o0.y - self.o1.y) / inv;
            let (dx, dy) = (self.o0.x + px / self.s0, self.o0.y + py / self.s0);
            Point::new(dx - px / scale, dy - py / scale)
        };
        (scale, origin)
    }
}

pub enum KeyAction {
    None,
    Copy,
    Export,
    Open,
}

/// The rubber band: a dashed white rectangle in screen pixels.
fn draw_marquee(pix: &mut Pixmap, view: &View, m: IRect) {
    let (w, h) = (pix.width() as i64, pix.height() as i64);
    let data = pix.data_mut();
    let mut put = |x: i64, y: i64| {
        if x >= 0 && y >= 0 && x < w && y < h {
            data[(y * w + x) as usize] = znimok_render::vello_cpu::color::PremulRgba8 {
                r: 0xFF,
                g: 0xFF,
                b: 0xFF,
                a: 0xFF,
            };
        }
    };
    let p0 = view.to_out(Point::new(m.x as f64, m.y as f64));
    let p1 = view.to_out(Point::new(m.right() as f64, m.bottom() as f64));
    let (x0, y0, x1, y1) = (
        p0.x.round() as i64,
        p0.y.round() as i64,
        p1.x.round() as i64,
        p1.y.round() as i64,
    );
    for x in x0.max(0)..=x1.min(w - 1) {
        if (x - x0) % 6 < 3 {
            put(x, y0);
            put(x, y1);
        }
    }
    for y in y0.max(0)..=y1.min(h - 1) {
        if (y - y0) % 6 < 3 {
            put(x0, y);
            put(x1, y);
        }
    }
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

/// Kind order of the inspector and the layers list (see `prop-kind` in app.slint).
fn kind_index(k: Kind) -> i32 {
    match k {
        Kind::Rect => 0,
        Kind::Ellipse => 1,
        Kind::Line => 2,
        Kind::Pen => 3,
        Kind::Text => 4,
        Kind::Hide => 5,
        Kind::Mark => 6,
        Kind::Counter => 7,
        Kind::Stamp => 8,
        _ => 9,
    }
}

fn tool_kind(t: usize) -> i32 {
    match t {
        tool::RECT => 0,
        tool::ELLIPSE => 1,
        tool::ARROW => 2,
        tool::PEN => 3,
        tool::TEXT => 4,
        tool::HIDE => 5,
        tool::MARKER => 6,
        tool::COUNTER => 7,
        tool::STAMP => 8,
        _ => -1,
    }
}

fn dash_index(d: Dash) -> i32 {
    match d {
        Dash::Solid => 0,
        Dash::Dashed => 1,
        Dash::DashDot => 2,
    }
}

fn corners_index(c: Corners) -> i32 {
    match c {
        Corners::Sharp => 0,
        Corners::Soft => 1,
        Corners::Round => 2,
    }
}

fn head_index(h: Head) -> i32 {
    match h {
        Head::None => 0,
        Head::Triangle => 1,
        Head::Chevron => 2,
        Head::Dot => 3,
    }
}
