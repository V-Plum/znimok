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
use znimok_render::{Renderer, Repaint, Tracker, View};

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

/// The marker's own colours (ZK-171, LH `kEdMarkPalette`): highlighter inks — pure channels,
/// bright under multiply.
pub const MARKER_PALETTE: [Rgb; 4] = [
    Rgb::new(0xFF, 0xFF, 0x00),
    Rgb::new(0x00, 0xFF, 0x00),
    Rgb::new(0xFF, 0x00, 0xFF),
    Rgb::new(0x00, 0xFF, 0xFF),
];

/// Marker thicknesses on a picture up to 1080 px on its short side (ZK-171; LH 16 / 26 / 40),
/// scaled up for larger pictures so a line of text is covered at any DPI.
pub const MARKER_SIZES: [i32; 4] = [12, 18, 26, 38];

/// How a colour control shows a colour: its palette chip (index), -1 for none, -2 for a colour
/// off the palette (the picker's chip then shows it, ZK-160) — and the colour itself.
fn colour_shown(c: Option<Rgb>) -> (i32, slint::Color) {
    colour_shown_in(&PALETTE, c)
}

/// [`colour_shown`] against another row of chips (the marker's, ZK-171).
fn colour_shown_in(palette: &[Rgb], c: Option<Rgb>) -> (i32, slint::Color) {
    match c {
        None => (-1, slint::Color::from_argb_u8(0, 0, 0, 0)),
        Some(c) => (
            palette
                .iter()
                .position(|p| *p == c)
                .map_or(-2, |i| i as i32),
            slint::Color::from_rgb_u8(c.r, c.g, c.b),
        ),
    }
}

/// `#RRGGBB` of a colour.
pub fn hex_of(c: Rgb) -> String {
    format!("#{:02X}{:02X}{:02X}", c.r, c.g, c.b)
}

/// A colour typed as hex: `#RGB`, `RGB`, `#RRGGBB` or `RRGGBB`, any case, spaces around.
pub fn parse_hex(t: &str) -> Option<Rgb> {
    let h = t.trim().trim_start_matches('#');
    if !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let v = |s: &str| u8::from_str_radix(s, 16).ok();
    match h.len() {
        3 => {
            let d: Vec<u8> = h
                .chars()
                .filter_map(|c| c.to_digit(16).map(|d| d as u8 * 17))
                .collect();
            Some(Rgb::new(d[0], d[1], d[2]))
        }
        6 => Some(Rgb::new(v(&h[0..2])?, v(&h[2..4])?, v(&h[4..6])?)),
        _ => None,
    }
}

/// A rectangle or ellipse without an outline: one solid colour, its main one (LH's plate).
fn is_plate(o: &Object) -> bool {
    matches!(o.kind(), Kind::Rect | Kind::Ellipse) && o.style.no_main
}

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
    /// Crop (ZK-53): not a mark tool — it edits the document's frame.
    pub const CROP: usize = 10;
    pub const NAMES: [&str; 11] = [
        "tool-select",
        "tool-rect",
        "tool-ellipse",
        "tool-line",
        "tool-pen",
        "tool-text",
        "tool-hide",
        "tool-highlighter",
        "tool-counter",
        "tool-stamp",
        "tool-crop",
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
    /// Turning the selection by its rotation handle (ZK-164): the angle of the pointer about
    /// `center` against `ang0` at the press. One mark turns about its own centre; several turn
    /// about the middle of their box — each moves round it and adds the angle to its own.
    Rotate {
        center: (f64, f64),
        ang0: f64,
        /// Each mark as it was: id, centre, angle, whether it turns (Hide only moves).
        orig: Vec<(ObjectId, (f64, f64), u16, bool)>,
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
    /// Shift / Ctrl pressed on a mark: toggle on release, or move once dragged.
    Toggle {
        id: ObjectId,
        was_selected: bool,
        start: (i32, i32),
        start_out: Point,
    },
    /// A frame dragged over the picture while the text panel is open: read only that part; a
    /// click without a drag copies the line under it (ZK-184).
    TextRegion {
        start: (i32, i32),
        start_out: Point,
    },
    /// Selecting in the text being typed with the mouse (ZK-49).
    TextSelect {
        anchor: usize,
    },
    /// Rubber-band selection with the Select tool from empty space (ZK-52).
    Marquee {
        start: (i32, i32),
        add: bool,
    },
    /// A new crop frame dragged out on the picture (ZK-53); `prev` comes back on a stray click.
    CropNew {
        start: (i32, i32),
        prev: IRect,
    },
    /// A crop handle (0 top-left, clockwise to 7 left) or the inside (8, move).
    CropEdit {
        handle: usize,
        orig: IRect,
        start: (i32, i32),
    },
}

/// A row of the layers list as shown (front first), for dragging (ZK-54).
#[derive(Clone, Copy, Debug, PartialEq)]
enum LayerRef {
    Mark { id: ObjectId, group: GroupId },
    Group(GroupId),
}

/// Where a dragged row would go.
#[derive(Clone, Copy, Debug, PartialEq)]
enum LayerDrop {
    /// In front of this row (above it in the list).
    Before(usize),
    /// Behind this row.
    After(usize),
    /// Onto this row: group with it.
    Into(usize),
}

/// A text mark being typed right on the canvas (ZK-49): every keystroke goes into the document,
/// the canvas draws the real text with the caret and the selection over it; a hidden TextInput
/// only takes the keys (and the input method).
struct TextEdit {
    /// The mark; `None` until the first character of a new text.
    id: Option<ObjectId>,
    at: (i32, i32),
    /// All the typing is one undo step.
    merge: MergeKey,
    /// Something went into the document (Esc takes it back).
    changed: bool,
    cursor: usize,
    anchor: usize,
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
    /// A video document opened for its marks (ZK-145): written back as a video, never as a
    /// screenshot of its poster.
    pub video: Option<znimok_format::VideoPart>,
    /// The «Відео» mode of the editor (ZK-181): the player, the timeline, what frame is shown.
    pub vid: Option<crate::video::Vid>,
}

/// "Over the screen" (ZK-58): the editor window covers the frozen display without chrome; the
/// document is that display 1:1 and its crop is the frame. Nothing is written to the library
/// until the picture is copied or saved (Esc leaves no trace); "to the window" keeps the same
/// document, marks and undo, so the pixels are the same either way.
pub struct Over {
    pub display: znimok_platform::Rect,
    restore: Option<crate::over::Restore>,
    source: &'static str,
}

pub struct App {
    /// The library window (the hub) or an editor window (ZK-107).
    pub role: crate::wins::Role,
    /// This window, for its own timers and worker threads.
    me: crate::wins::WeakCtx,
    pub tr: std::rc::Rc<Localizer>,
    pub lib_dir: PathBuf,
    entries: Vec<Entry>,
    filter: String,
    pub s: Option<Session>,
    pub over: Option<Over>,
    renderer: Renderer,
    view: View,
    /// The canvas as shown: `base` plus the selection, caret and marquee.
    pixmap: Pixmap,
    /// The picture with its marks and the frame edge, repainted only where it changed (ZK-130).
    base: Pixmap,
    tracker: Tracker,
    /// Where the selection, caret and marquee were drawn last frame (canvas pixels).
    overlay_prev: Vec<IRect>,
    pub gpu: Option<Gpu>,
    pub dpr: f64,
    dirty: bool,
    /// The video frame on the canvas changed: everything is repainted (ZK-181).
    frame_changed: bool,
    fit_pending: bool,
    tool: usize,
    color: Rgb,
    /// New markers: their own colour and thickness step (ZK-171).
    marker_color: Rgb,
    marker_size_i: usize,
    thick: usize,
    /// Defaults for new marks (and what the inspector changes on a selection), ZK-54.
    alpha: u8,
    /// Rectangle / ellipse without an outline (a solid plate of the fill).
    no_stroke: bool,
    fill: Option<Rgb>,
    /// Outline colour of new texts (its own default: a shape's fill is not a text's outline).
    text_outline: Option<Rgb>,
    dash: Dash,
    corners: Corners,
    head_start: Head,
    head_end: Head,
    /// Pen trails have their own heads, none by default (ZK-48).
    pen_head_start: Head,
    pen_head_end: Head,
    shadow: Effect,
    glow: Effect,
    text_size_i: usize,
    /// Point size for new text, screenshot pixels (0 = automatic, from the picture height).
    text_px: i32,
    bold: bool,
    italic: bool,
    /// Alignment of new texts (and of the selection's, ZK-49).
    align: Align,
    /// New counters and stamps (ZK-51): shape, numbering group, digit colour (None = auto
    /// black or white), which stamp or emoji.
    counter_shape: CounterShape,
    /// Size step of new counters (ZK-166), an index into [`COUNTER_SIZES`].
    counter_size_i: usize,
    counter_group: u32,
    digit: Option<Rgb>,
    /// The eyedropper is armed for this colour control ("color", "fill", "outline", "digit"):
    /// the next click on the canvas picks the colour there (ZK-160).
    eyedrop: Option<String>,
    /// For the self-test: Alt held (the real one is read from the system, ZK-167).
    pub test_alt: bool,
    /// The next document opens in this very window, the library's too (ZK-192): a card clicked
    /// in the library opens where it was clicked — library and editor are one window.
    open_here: bool,
    /// Text found on the picture (ZK-184); the panel is open while this is `Some`.
    text: Option<TextFind>,
    /// The export sheet while it is open (ZK-187).
    exp: Option<ExportSheet>,
    /// What was exported last in this session: Ctrl+Shift+E repeats it (ZK-187).
    exp_last: Option<znimok_settings::ExportPrefs>,
    stamp_id: u32,
    /// Size step of new stamps (ZK-173), an index into [`COUNTER_SIZES`] like the counter's.
    stamp_size_i: usize,
    /// One undo step per drag of the opacity slider.
    alpha_merge: Option<MergeKey>,
    /// Crop being edited (Crop tool), picture pixels. The document gets it as one `SetCrop`
    /// on Enter / another tool; Esc drops it (ZK-53).
    crop: Option<IRect>,
    crop_lock: bool,
    /// "Compare" held: the picture is drawn with the tone as captured.
    compare: bool,
    /// One undo step per drag of a tone slider.
    tone_merge: Option<MergeKey>,
    /// Picture size last pushed to the "Image size" fields (they are not overwritten while
    /// the user types).
    size_shown: (u32, u32),
    drag: Option<Drag>,
    next_merge: u64,
    editing: Option<TextEdit>,
    /// Caret blink phase while typing.
    caret_on: bool,
    caret_timer: slint::Timer,
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
    /// Layers list as shown, and groups folded in it (view state, not the document).
    layer_rows: Vec<LayerRef>,
    /// The rows shown in the Layers tab. Updated in place: a new model would rebuild every row,
    /// and the row under a pressed button would lose the press — a click selects (and syncs)
    /// on press, so dragging a layer never started (owner, 29.09).
    layers_model: std::rc::Rc<VecModel<crate::LayerRow>>,
    layers_bound: bool,
    collapsed: std::collections::HashSet<GroupId>,
    /// How the picture last left the editor — Enter repeats it (ZK-60, LH): false copy,
    /// true export.
    last_export: bool,
    /// settings.json (ZK-56); `None` when the OS gives no config folder. Shared by the windows.
    store: Option<std::rc::Rc<znimok_settings::Store>>,
    /// Page shown before the settings (Esc / back returns there).
    settings_from: i32,
    /// The documents last moved to the trash: (where each is now, where it was) — for "Undo".
    undo_trash: Vec<(PathBuf, PathBuf)>,
    /// The library window shows the library's trash (ZK-175).
    trash_view: bool,
    /// Cards picked with Shift / Ctrl (ZK-176), by path, and the last one clicked (the start
    /// of a Shift range).
    picked: Vec<PathBuf>,
    pick_anchor: Option<PathBuf>,
    /// The cards as shown, in order, and their model (a pick changes its mark in place, the
    /// thumbnails are not decoded again).
    shown: Vec<PathBuf>,
    cards_model: std::rc::Rc<VecModel<CardData>>,
    /// The groups of cards as shown (ZK-177): a title and how many cards, in `shown` order.
    groups: Vec<(String, usize)>,
    /// Columns of the grid as the window last reported them.
    lib_cols: usize,
    /// The keyboard's card in the grid (ZK-179).
    cur: Option<PathBuf>,
    /// The library index (ZK-131) and the folder it belongs to.
    index: Option<(PathBuf, library::Index)>,
    /// Updates (ZK-142): a check or a download is running; what the last check found.
    update_busy: bool,
    update_found: Option<crate::update::Found>,
    /// macOS (ZK-143): where the Sparkle update stands.
    update_phase: crate::update::Phase,
    /// The folder's fingerprint at the last look, and when that was (the watcher, ZK-131).
    lib_fp: u64,
    lib_polled: Option<Instant>,
    /// Re-reads the macOS permissions while the first-run guide is open.
    recheck_timer: slint::Timer,
    /// Watches the system's light / dark.
    theme_timer: slint::Timer,
}

/// The library folder: `ZNIMOK_LIBRARY` (tests, the CLI), then the one chosen in the settings,
/// then the default next to the other app data.
pub fn library_dir(prefs: &znimok_settings::Settings) -> PathBuf {
    if std::env::var_os("ZNIMOK_LIBRARY").is_some() {
        return library::default_dir();
    }
    prefs
        .library
        .dir
        .clone()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or_else(library::default_dir)
}

thread_local! {
    /// The theme mode for windows made later (the card after a capture).
    pub static THEME_MODE: std::cell::Cell<i32> = const { std::cell::Cell::new(0) };
    /// The system's own light / dark, as winit last told it.
    pub static SYSTEM_DARK: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

fn theme_mode(t: znimok_settings::Theme) -> i32 {
    match t {
        znimok_settings::Theme::Auto => 0,
        znimok_settings::Theme::Light => 1,
        znimok_settings::Theme::Dark => 2,
    }
}

fn args(pairs: &[(&'static str, String)]) -> FluentArgs<'static> {
    let mut a = FluentArgs::new();
    for (k, v) in pairs {
        a.set(*k, v.clone());
    }
    a
}

/// Which date group a moment falls in (ZK-177): 0 today, 1 yesterday, 2 earlier this week,
/// 3 earlier this month, then 4 + months back (one group per month).
fn date_bucket(ms: i64) -> usize {
    use chrono::{Datelike, Local, TimeZone};
    let now = Local::now();
    let Some(t) = Local.timestamp_millis_opt(ms).single() else {
        return 0;
    };
    let (d, today) = (t.date_naive(), now.date_naive());
    if d >= today {
        return 0;
    }
    if today.signed_duration_since(d).num_days() == 1 {
        return 1;
    }
    let monday = today - chrono::Duration::days(today.weekday().num_days_from_monday() as i64);
    if d >= monday {
        return 2;
    }
    if (d.year(), d.month()) == (today.year(), today.month()) {
        return 3;
    }
    let months = (today.year() - d.year()) * 12 + today.month() as i32 - d.month() as i32;
    3 + months.max(1) as usize
}

/// A number for a plural selector (a string would always take the default variant).
fn count_args(name: &'static str, n: i64) -> FluentArgs<'static> {
    let mut a = FluentArgs::new();
    a.set(name, n);
    a
}

/// Fluent arguments from pairs, for other modules.
pub fn fargs(pairs: &[(&'static str, String)]) -> FluentArgs<'static> {
    args(pairs)
}

fn bounds_of_points(points: &[(i32, i32)]) -> IRect {
    Object::new(IRect::default(), Data::pen(points.to_vec())).bounds()
}

impl App {
    /// The library window's state.
    pub fn new(tr: std::rc::Rc<Localizer>, lib_dir: PathBuf) -> Self {
        Self {
            role: crate::wins::Role::Library,
            me: crate::wins::WeakCtx::UNBOUND,
            tr,
            lib_dir,
            entries: Vec::new(),
            filter: String::new(),
            s: None,
            over: None,
            renderer: Renderer::new(),
            view: View {
                scale: 1.0,
                origin: Point::ZERO,
                width: 1,
                height: 1,
            },
            pixmap: Pixmap::new(1, 1),
            base: Pixmap::new(1, 1),
            tracker: Tracker::default(),
            overlay_prev: Vec::new(),
            gpu: None,
            dpr: 1.0,
            dirty: true,
            frame_changed: false,
            fit_pending: true,
            tool: tool::RECT,
            color: PALETTE[0],
            marker_color: MARKER_PALETTE[0],
            marker_size_i: 1,
            thick: 1,
            alpha: 100,
            no_stroke: false,
            fill: None,
            text_outline: None,
            dash: Dash::Solid,
            corners: Corners::Sharp,
            head_start: Head::None,
            head_end: Head::Triangle,
            pen_head_start: Head::None,
            pen_head_end: Head::None,
            shadow: Effect::None,
            glow: Effect::None,
            text_size_i: 1,
            text_px: 0,
            bold: false,
            italic: false,
            align: Align::Left,
            counter_shape: CounterShape::Circle,
            counter_size_i: 1,
            counter_group: 1,
            digit: None,
            eyedrop: None,
            test_alt: false,
            open_here: false,
            text: None,
            exp: None,
            exp_last: None,
            stamp_id: 0,
            stamp_size_i: 1,
            alpha_merge: None,
            crop: None,
            crop_lock: false,
            compare: false,
            tone_merge: None,
            size_shown: (0, 0),
            drag: None,
            next_merge: 1,
            editing: None,
            caret_on: true,
            caret_timer: slint::Timer::default(),
            autosave: true,
            saving: false,
            toast_at: None,
            anim: None,
            anim_timer: slint::Timer::default(),
            marquee: None,
            last_out: Point::ZERO,
            layer_rows: Vec::new(),
            layers_model: std::rc::Rc::new(VecModel::default()),
            layers_bound: false,
            collapsed: std::collections::HashSet::new(),
            last_export: false,
            store: None,
            settings_from: 0,
            undo_trash: Vec::new(),
            trash_view: false,
            picked: Vec::new(),
            pick_anchor: None,
            shown: Vec::new(),
            cards_model: std::rc::Rc::new(VecModel::default()),
            groups: Vec::new(),
            lib_cols: 4,
            cur: None,
            index: None,
            update_busy: false,
            update_found: None,
            update_phase: crate::update::Phase::Idle,
            lib_fp: 0,
            lib_polled: None,
            recheck_timer: slint::Timer::default(),
            theme_timer: slint::Timer::default(),
        }
    }

    /// An editor window's state (ZK-107): the language and the folder of the hub, no library
    /// index of its own (the index is a database, opened once, by the library window).
    pub fn new_editor(parent: &App) -> Self {
        let mut a = Self::new(parent.tr.clone(), parent.lib_dir.clone());
        a.role = crate::wins::Role::Editor;
        a
    }

    /// Ties the state to its window's handle (once, right after creation).
    pub fn bind(&mut self, me: crate::wins::WeakCtx) {
        self.me = me;
    }

    /// This window, for timers and worker threads.
    pub fn me(&self) -> crate::wins::WeakCtx {
        self.me
    }

    pub fn store_handle(&self) -> Option<std::rc::Rc<znimok_settings::Store>> {
        self.store.clone()
    }

    // ------------------------------------------------------------------ settings (ZK-56)

    /// Takes the settings store and applies what the app already honours.
    pub fn use_settings(
        &mut self,
        ui: &AppWindow,
        store: Option<std::rc::Rc<znimok_settings::Store>>,
    ) {
        self.store = store;
        if self.role == crate::wins::Role::Library {
            let days = self.prefs().library.trash_days as u64;
            library::purge_trash(&self.lib_dir, days);
        }
        let p = self.prefs();
        self.autosave = p.editor.autosave;
        ui.set_autosave(self.autosave);
        crate::filemeta::set_enabled(p.editor.write_metadata);
        ui.set_export_meta(p.editor.write_metadata);
        crate::overlay::set_prefs(&p.capture);
        self.show_recent_colours(ui);
        self.apply_theme(ui, &p);
        self.settings_sync(ui);
    }

    /// The settings changed in another window (ZK-107): what this window shows of them.
    pub fn reload_prefs(&mut self, ui: &AppWindow) {
        let p = self.prefs();
        let lang = znimok_i18n::choose_language(
            p.general.language.as_deref(),
            znimok_i18n::system_language().as_deref(),
        );
        if lang != self.tr.lang() {
            self.tr = std::rc::Rc::new(Localizer::new(lang));
        }
        self.autosave = p.editor.autosave;
        ui.set_autosave(self.autosave);
        ui.set_export_meta(p.editor.write_metadata);
        self.lib_dir = library_dir(&p);
        self.show_recent_colours(ui);
        self.apply_theme(ui, &p);
        self.settings_sync(ui);
        self.show_capture_key(ui);
        self.show_cards(ui);
        self.sync(ui);
        ui.window().request_redraw();
    }

    /// Light / dark / as the system for the main window, the card after a capture and the
    /// macOS window chrome (the overlay stays dark by itself).
    pub fn apply_theme(&self, ui: &AppWindow, p: &znimok_settings::Settings) {
        let mode = theme_mode(p.general.theme);
        let dark = crate::system::system_dark();
        ui.global::<crate::Theme>().set_system_dark(dark);
        SYSTEM_DARK.with(|d| d.set(dark));
        ui.global::<crate::Theme>().set_mode(mode);
        // "As the system" follows a change of the system's theme within a couple of seconds
        // (one watcher, in the library window; every window is told).
        if self.role == crate::wins::Role::Library && !self.theme_timer.running() {
            self.theme_timer.start(
                slint::TimerMode::Repeated,
                std::time::Duration::from_secs(2),
                move || {
                    let dark = crate::system::system_dark();
                    if SYSTEM_DARK.with(|d| d.get()) != dark {
                        crate::wins::for_each(|a, ui| a.system_theme(ui, dark));
                    }
                },
            );
        }
        THEME_MODE.with(|m| m.set(mode));
        crate::frame::set_dark(ui, ui.global::<crate::Theme>().get_dark());
    }

    /// The system switched light / dark (winit's ThemeChanged).
    pub fn system_theme(&mut self, ui: &AppWindow, dark: bool) {
        SYSTEM_DARK.with(|d| d.set(dark));
        ui.global::<crate::Theme>().set_system_dark(dark);
        crate::frame::set_dark(ui, ui.global::<crate::Theme>().get_dark());
        self.dirty = true;
        ui.window().request_redraw();
    }

    /// Before the process ends (ZK-146): close what must be closed cleanly — the library index
    /// (a database) — while everything is still alive.
    pub fn before_exit(&mut self) {
        self.index = None;
    }

    pub fn prefs(&self) -> znimok_settings::Settings {
        self.store.as_ref().map(|s| s.get()).unwrap_or_default()
    }

    /// Changes the settings file (a failure to write is shown, the app keeps the value).
    fn save_prefs(&mut self, ui: &AppWindow, f: impl FnOnce(&mut znimok_settings::Settings)) {
        let Some(store) = self.store.as_ref() else {
            return;
        };
        if let Err(e) = store.update(f) {
            let msg = format!("{} ({e})", self.tr.tr("err-library-save"));
            self.toast(ui, msg);
        }
        // The other windows follow (ZK-107).
        crate::wins::prefs_changed(self.me);
        // The overlay reads its settings from a copy (it opens where the app is borrowed).
        crate::overlay::set_prefs(&self.prefs().capture);
    }

    /// Once a day, when the person turned daily checks on (ZK-142). Cheap until it is due.
    /// macOS: also where Sparkle's events are taken (every timer tick).
    pub fn update_tick(&mut self, ui: &AppWindow) {
        #[cfg(target_os = "macos")]
        self.sparkle_events(ui);
        if self.update_busy {
            return;
        }
        let p = self.prefs();
        let now = chrono::Local::now().timestamp().max(0) as u64;
        if !p.updates.check_daily || now.saturating_sub(p.updates.last_check) < 24 * 3600 {
            return;
        }
        self.update_check(ui, false);
    }

    /// «Перевірити зараз» (`user_initiated`), and the daily check.
    pub fn update_check(&mut self, ui: &AppWindow, user_initiated: bool) {
        if self.update_busy {
            return;
        }
        #[cfg(target_os = "macos")]
        {
            // Sparkle: a background check is silent unless there is an update; a check by the
            // person answers either way (through the events).
            if !crate::update::mac::available() {
                self.update_checked(ui, crate::update::Found::NotConfigured);
                return;
            }
            if crate::update::mac::check(user_initiated) {
                self.update_busy = true;
                if user_initiated {
                    ui.set_upd_status(self.tr.tr("upd-checking").into());
                }
                ui.set_upd_busy(true);
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = user_initiated;
            self.update_busy = true;
            ui.set_upd_status(self.tr.tr("upd-checking").into());
            ui.set_upd_busy(true);
            let me = self.me();
            crate::update::check(move |found| {
                me.with(|a, ui| a.update_checked(ui, found));
            });
        }
    }

    /// macOS: what Sparkle said since the last tick, into the page's state.
    #[cfg(target_os = "macos")]
    fn sparkle_events(&mut self, ui: &AppWindow) {
        use crate::update::mac::{Choice, Event};
        use crate::update::{Found, Phase};
        let events = crate::update::mac::poll();
        if events.is_empty() {
            return;
        }
        for e in events {
            match e {
                Event::Checking => {
                    self.update_busy = true;
                }
                Event::Found {
                    version,
                    notes_url,
                    size,
                    ..
                } => {
                    self.update_phase = Phase::Idle;
                    self.update_checked(
                        ui,
                        Found::Sparkle {
                            version,
                            notes: notes_url,
                            size,
                        },
                    );
                }
                Event::NotFound(_) => {
                    self.update_phase = Phase::Idle;
                    self.update_checked(ui, Found::UpToDate);
                }
                Event::Error(e) => {
                    self.update_phase = Phase::Idle;
                    self.update_checked(ui, Found::Failed(e));
                }
                Event::NotesFailed(_) => {}
                Event::DownloadStarted => {
                    self.update_busy = true;
                    self.update_phase = Phase::Downloading {
                        received: 0,
                        total: 0,
                    };
                }
                Event::DownloadTotal(t) => {
                    if let Phase::Downloading { total, .. } = &mut self.update_phase {
                        *total = t;
                    }
                }
                Event::DownloadProgress(n) => {
                    if let Phase::Downloading { received, .. } = &mut self.update_phase {
                        *received += n;
                    }
                }
                Event::Extracting(p) => {
                    self.update_busy = true;
                    self.update_phase = Phase::Extracting(p);
                }
                Event::ReadyToInstall => {
                    // «Оновити зараз» means the whole way, as on Windows: save, then relaunch.
                    self.save_now(ui);
                    self.update_phase = Phase::Installing;
                    crate::update::mac::reply(Choice::Install);
                }
                Event::Installing { app_terminated } => {
                    self.update_phase = Phase::Installing;
                    if !app_terminated {
                        // Sparkle's installer waits for the app: leave the loop (before_exit
                        // saves and flushes), the new version comes up by itself.
                        let _ = slint::quit_event_loop();
                    }
                }
                Event::InstalledAndRelaunched(_) => {
                    self.update_phase = Phase::Idle;
                    self.update_busy = false;
                }
                Event::Dismissed => {
                    self.update_phase = Phase::Idle;
                    self.update_busy = false;
                }
            }
        }
        let p = self.prefs();
        self.agents_sync(ui, &p);
    }

    fn update_checked(&mut self, ui: &AppWindow, found: crate::update::Found) {
        use crate::update::Found;
        self.update_busy = false;
        let now = chrono::Local::now().timestamp().max(0) as u64;
        let tag = match &found {
            Found::Available(a) => Some(a.tag.clone()),
            Found::Sparkle { version, .. } => Some(format!("v{version}")),
            _ => None,
        };
        let told = self.prefs().updates.notified_tag;
        self.save_prefs(ui, |p| {
            p.updates.last_check = now;
            p.updates.available_tag = tag.clone();
        });
        // A new release is told once (a toast); the Updates page always shows it.
        let new_version = match &found {
            Found::Available(a) => Some((a.tag.clone(), a.version.clone())),
            Found::Sparkle { version, .. } => Some((format!("v{version}"), version.clone())),
            _ => None,
        };
        if let Some((t, version)) = new_version
            && told.as_deref() != Some(t.as_str())
        {
            let msg = self
                .tr
                .tr_args("update-available", &args(&[("version", version)]));
            self.toast(ui, msg);
            self.save_prefs(ui, |p| p.updates.notified_tag = Some(t));
        }
        self.update_found = Some(found);
        let p = self.prefs();
        self.agents_sync(ui, &p);
    }

    /// «Встановити»: Windows — download, verify, hand over to the installer and exit; macOS —
    /// Sparkle downloads, verifies and, once ready, relaunches (see `sparkle_events`).
    pub fn update_install(&mut self, ui: &AppWindow) {
        #[cfg(target_os = "macos")]
        {
            if self.update_busy
                || !matches!(
                    self.update_found,
                    Some(crate::update::Found::Sparkle { .. })
                )
            {
                return;
            }
            if crate::update::mac::reply(crate::update::mac::Choice::Install) {
                self.update_busy = true;
                self.update_phase = crate::update::Phase::Downloading {
                    received: 0,
                    total: 0,
                };
                ui.set_upd_status(self.tr.tr("upd-downloading").into());
                ui.set_upd_busy(true);
                ui.set_upd_progress(0.0);
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let Some(crate::update::Found::Available(a)) = self.update_found.clone() else {
                return;
            };
            if self.update_busy {
                return;
            }
            self.update_busy = true;
            self.save_now(ui);
            ui.set_upd_busy(true);
            ui.set_upd_status(self.tr.tr("upd-downloading").into());
            let me = self.me();
            crate::update::install(a, move |reason| {
                me.with(|a, ui| {
                    a.update_busy = false;
                    a.update_found = Some(crate::update::Found::Failed(reason));
                    let p = a.prefs();
                    a.agents_sync(ui, &p);
                });
            });
        }
    }

    /// The Agents and Updates pages: clients with lasting permissions, the last actions, the
    /// version and the last update check.
    fn agents_sync(&self, ui: &AppWindow, p: &znimok_settings::Settings) {
        use znimok_agents::permissions::Scope;
        let clients: Vec<crate::AgentClient> =
            znimok_agents::permissions::Permissions::open_default()
                .map(|perm| perm.clients())
                .unwrap_or_default()
                .into_iter()
                .map(|(name, scopes)| crate::AgentClient {
                    name: name.into(),
                    scopes: scopes
                        .iter()
                        .map(|s| {
                            self.tr.tr(match s {
                                Scope::Capture => "agents-scope-screen",
                                Scope::LibraryRead => "agents-scope-library",
                                Scope::LibraryWrite => "agents-scope-marks",
                                Scope::Settings => "agents-scope-settings",
                            })
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                        .into(),
                })
                .collect();
        ui.set_agent_clients(std::rc::Rc::new(VecModel::from(clients)).into());
        let mut log: Vec<SharedString> = znimok_agents::audit::Audit::open_default()
            .map(|a| a.entries())
            .unwrap_or_default()
            .into_iter()
            .rev()
            .take(8)
            .map(|e| {
                let when = chrono::Local
                    .timestamp_millis_opt(e.ts)
                    .single()
                    .map(|t| t.format("%d.%m %H:%M").to_string())
                    .unwrap_or_default();
                if !e.ok && e.grant.is_none() {
                    format!(
                        "{when} · {}",
                        self.tr
                            .tr_args("agents-log-denied", &args(&[("client", e.client.clone())]))
                    )
                    .into()
                } else {
                    self.tr
                        .tr_args(
                            "agents-log-entry",
                            &args(&[("when", when), ("client", e.client), ("tool", e.tool)]),
                        )
                        .into()
                }
            })
            .collect();
        log.truncate(8);
        ui.set_agent_log(std::rc::Rc::new(VecModel::from(log)).into());
        ui.set_agent_model(p.agents.assistant_model.clone().into());
        ui.set_upd_version(
            self.tr
                .tr_args(
                    "about-version",
                    &args(&[("version", env!("CARGO_PKG_VERSION").to_string())]),
                )
                .into(),
        );
        let last = if p.updates.last_check == 0 {
            self.tr.tr("upd-never-checked")
        } else {
            let when = chrono::Local
                .timestamp_opt(p.updates.last_check as i64, 0)
                .single()
                .map(|t| t.format("%d.%m.%Y %H:%M").to_string())
                .unwrap_or_default();
            self.tr.tr_args("upd-last-check", &args(&[("when", when)]))
        };
        ui.set_upd_last(last.into());
        // What the last check (this run) found, and what can be done about it.
        use crate::update::Found;
        use crate::update::Phase;
        let mb = |b: u64| format!("{:.0} MB", b as f64 / (1 << 20) as f64);
        let available_text = |version: &str, size: u64| {
            format!(
                "{} · {}",
                self.tr.tr_args(
                    "update-available",
                    &args(&[("version", version.to_string())])
                ),
                mb(size)
            )
        };
        let status = match &self.update_found {
            None => String::new(),
            Some(Found::UpToDate) => self.tr.tr("upd-up-to-date"),
            Some(Found::NotConfigured) => self.tr.tr("upd-not-configured"),
            Some(Found::Failed(e)) => self
                .tr
                .tr_args("upd-failed", &args(&[("reason", e.clone())])),
            Some(Found::Available(a)) => available_text(&a.version, a.installer_size()),
            Some(Found::Sparkle { version, size, .. }) => available_text(version, *size),
        };
        // macOS: the Sparkle phases on top of what was found.
        let (status, progress) = match &self.update_phase {
            Phase::Idle => (status, -1.0),
            Phase::Downloading { received, total } => (
                if *total > 0 {
                    format!(
                        "{} {} / {}",
                        self.tr.tr("upd-downloading"),
                        mb(*received),
                        mb(*total)
                    )
                } else {
                    self.tr.tr("upd-downloading")
                },
                if *total > 0 {
                    (*received as f32 / *total as f32).clamp(0.0, 1.0)
                } else {
                    0.0
                },
            ),
            Phase::Extracting(p) => (self.tr.tr("upd-extracting"), *p as f32),
            Phase::Installing => (self.tr.tr("upd-installing"), 1.0),
        };
        if !self.update_busy || self.update_phase != Phase::Idle {
            ui.set_upd_status(status.into());
        }
        ui.set_upd_progress(progress);
        ui.set_upd_busy(self.update_busy);
        let (win, mac) = (
            matches!(self.update_found, Some(Found::Available(_))),
            matches!(self.update_found, Some(Found::Sparkle { .. })),
        );
        ui.set_upd_can_install((win && cfg!(windows)) || (mac && self.update_phase == Phase::Idle));
        ui.set_upd_can_open((win && !cfg!(windows)) || mac);
    }

    pub fn settings_open(&mut self, ui: &AppWindow) {
        self.finish_text(ui);
        let page = ui.get_page();
        if page != 2 {
            self.settings_from = page;
        }
        self.settings_sync(ui);
        ui.set_page(2);
        ui.invoke_focus_settings();
    }

    pub fn settings_close(&mut self, ui: &AppWindow) {
        if crate::REC.with(|r| r.get()).is_some() {
            self.hotkey_stop(ui);
        }
        if self.settings_from == 3 {
            self.settings_from = 0;
            self.onboarding_open(ui);
            return;
        }
        let back = if self.s.is_some() {
            self.settings_from.max(0)
        } else {
            0
        };
        ui.set_page(back);
        if back == 1 {
            ui.invoke_focus_canvas();
        } else {
            ui.invoke_focus_library();
        }
    }

    /// The hotkey that works for region shots, for the hints and the tray.
    pub fn show_capture_key(&self, ui: &AppWindow) {
        let os = znimok_platform::Os::current();
        let k = crate::hotkeys::active(crate::hotkeys::Action::Region)
            .or(self.prefs().capture.hotkeys.region)
            .map(|k| k.display(os))
            .unwrap_or_default();
        ui.set_capture_key(k.into());
    }

    /// A hotkey field waits for the next combination (the hotkeys are released meanwhile).
    pub fn hotkey_record(&mut self, ui: &AppWindow, i: i32) {
        let Some(a) = crate::hotkeys::Action::ALL.get(i as usize).copied() else {
            return;
        };
        let p = self.prefs();
        crate::hotkeys::set_paused(true, &p.capture.hotkeys, p.capture.enabled);
        crate::REC.with(|r| r.set(Some(a)));
        ui.set_key_recording(i);
    }

    fn hotkey_stop(&mut self, ui: &AppWindow) {
        crate::REC.with(|r| r.set(None));
        ui.set_key_recording(-1);
        let p = self.prefs();
        crate::hotkeys::set_paused(false, &p.capture.hotkeys, p.capture.enabled);
    }

    /// A key pressed while a field records: Esc cancels, Backspace / Delete alone clears, a key
    /// with modifiers becomes the new combination — if the system gives it; if not, the old one
    /// stays and the message says so.
    pub fn hotkey_key(
        &mut self,
        ui: &AppWindow,
        code: &str,
        mods: slint::winit_030::winit::keyboard::ModifiersState,
    ) {
        let Some(a) = crate::REC.with(|r| r.get()) else {
            return;
        };
        if code.starts_with("Shift")
            || code.starts_with("Control")
            || code.starts_with("Alt")
            || code.starts_with("Super")
            || code.starts_with("Meta")
        {
            return;
        }
        let bare = mods.is_empty();
        if code == "Escape" && bare {
            self.hotkey_stop(ui);
            self.settings_sync(ui);
            return;
        }
        let new = if (code == "Backspace" || code == "Delete") && bare {
            None
        } else {
            let Some(k) = crate::hotkeys::from_w3c(code) else {
                return;
            };
            let mut text = String::new();
            if mods.control_key() {
                text.push_str("Ctrl+");
            }
            if mods.alt_key() {
                text.push_str("Alt+");
            }
            if mods.shift_key() {
                text.push_str("Shift+");
            }
            if mods.super_key() {
                text.push_str("Meta+");
            }
            text.push_str(k.name());
            match znimok_platform::KeyCombo::parse(&text) {
                Ok(c) => Some(c),
                Err(e) => {
                    self.toast(ui, e.to_string());
                    return;
                }
            }
        };
        self.hotkey_stop(ui);
        if crate::hotkeys::try_set(a, new) {
            self.save_prefs(ui, |p| {
                crate::hotkeys::Action::set(&mut p.capture.hotkeys, a, new)
            });
        } else {
            let msg = self.tr.tr("keys-taken");
            self.toast(ui, msg);
        }
        self.show_capture_key(ui);
        self.settings_sync(ui);
    }

    /// The first-run guide (ZK-57): shown until "Done" or "Skip all"; also from the settings.
    pub fn onboarding_open(&mut self, ui: &AppWindow) {
        self.settings_sync(ui);
        ui.set_page(3);
        let weak = ui.as_weak();
        // Permissions change outside the app (System Settings): look again while the guide is up.
        self.recheck_timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_secs(2),
            move || {
                if let Some(ui) = weak.upgrade() {
                    if ui.get_page() != 3 {
                        return;
                    }
                    ui.set_onb_screen_ok(crate::system::screen_ok());
                }
            },
        );
    }

    /// Leaves the guide; `remember` = "Don't show next time" (ticked by default).
    fn onboarding_done(&mut self, ui: &AppWindow, remember: bool) {
        self.recheck_timer.stop();
        self.save_prefs(ui, |p| p.general.onboarding_done = remember);
        ui.set_page(if self.s.is_some() { 1 } else { 0 });
        ui.invoke_focus_library();
    }

    /// The settings page's controls, from the file.
    pub fn settings_sync(&self, ui: &AppWindow) {
        let p = self.prefs();
        {
            use znimok_platform::AutostartState as A;
            let st = crate::system::autostart_state();
            ui.set_pref_autostart(matches!(st, A::On | A::NeedsApproval));
            ui.set_pref_autostart_ok(st != A::Unavailable);
            ui.set_pref_autostart_note(
                match st {
                    A::NeedsApproval => self.tr.tr("autostart-needs-approval"),
                    A::DisabledInSystem => self.tr.tr("autostart-disabled-in-system"),
                    _ => String::new(),
                }
                .into(),
            );
            ui.set_onb_screen_ok(crate::system::screen_ok());
            let os = znimok_platform::Os::current();
            let k = |c: Option<znimok_platform::KeyCombo>| {
                c.map(|c| c.display(os))
                    .unwrap_or_else(|| self.tr.tr("keys-not-set"))
            };
            let keys = &p.capture.hotkeys;
            ui.set_onb_keys(
                self.tr
                    .tr_args(
                        if cfg!(target_os = "macos") {
                            "onb-keys-mac"
                        } else {
                            "onb-keys-win"
                        },
                        &args(&[
                            ("region", k(keys.region)),
                            ("screen", k(keys.screen)),
                            ("record", k(keys.video)),
                        ]),
                    )
                    .into(),
            );
        }
        {
            use crate::hotkeys::{Action, State};
            let os = znimok_platform::Os::current();
            let paused = crate::hotkeys::is_paused() && p.capture.enabled;
            let mut texts = Vec::new();
            let mut warns = Vec::new();
            for a in Action::ALL {
                let want = Action::of(&p.capture.hotkeys, a);
                texts.push(slint::SharedString::from(
                    want.map(|k| k.display(os))
                        .unwrap_or_else(|| self.tr.tr("keys-not-set")),
                ));
                warns.push(slint::SharedString::from(match crate::hotkeys::state(a) {
                    State::Taken { active, .. } => match active {
                        Some(f) => format!("{} → {}", self.tr.tr("keys-taken"), f.display(os)),
                        None => self.tr.tr("keys-taken"),
                    },
                    _ if paused => self.tr.tr("tray-tooltip-paused-keys"),
                    _ => String::new(),
                }));
            }
            // The guide says it once: "Taken by another program: A, B. Region shots work on C."
            let mut taken = Vec::new();
            let mut region_on = None;
            for a in Action::ALL {
                if let State::Taken { wanted, active } = crate::hotkeys::state(a) {
                    taken.push(wanted.display(os));
                    if a == Action::Region {
                        region_on = active;
                    }
                }
            }
            let mut warn = String::new();
            if !taken.is_empty() {
                warn = self
                    .tr
                    .tr_args("onb-keys-taken", &args(&[("keys", taken.join(", "))]));
                if let Some(k) = region_on {
                    warn.push(' ');
                    warn.push_str(
                        &self
                            .tr
                            .tr_args("onb-keys-fallback", &args(&[("key", k.display(os))])),
                    );
                }
            }
            ui.set_onb_keys_warn(warn.into());
            ui.set_key_texts(std::rc::Rc::new(VecModel::from(texts)).into());
            ui.set_key_warns(std::rc::Rc::new(VecModel::from(warns)).into());
        }
        ui.set_pref_capture(p.capture.enabled);
        ui.set_pref_quick_library(p.capture.quick_save_to_library);
        // ZK-155/156: what each gesture does (0 editor, 1 over the screen, 2 clipboard), hints.
        let code = |a: znimok_settings::CaptureAction| match a {
            znimok_settings::CaptureAction::Editor => 0,
            znimok_settings::CaptureAction::OverScreen => 1,
            znimok_settings::CaptureAction::Clipboard => 2,
        };
        let g = p.capture.gestures.valid();
        ui.set_pref_gesture_plain(code(g.plain));
        ui.set_pref_gesture_shift(code(g.shift));
        ui.set_pref_gesture_alt(code(g.alt));
        ui.set_pref_show_hints(p.capture.show_hints);
        ui.set_pref_dev_page(p.general.developer_page.as_deref() == Some(build_id().as_str()));
        ui.set_pref_keep_tool(p.editor.keep_tool);
        ui.set_pref_autosave(p.editor.autosave);
        ui.set_pref_metadata(p.editor.write_metadata);
        ui.set_pref_updates(p.updates.check_daily);
        ui.set_pref_mcp(p.agents.mcp_enabled);
        self.agents_sync(ui, &p);
        ui.set_pref_lang(match p.general.language.as_deref() {
            Some("uk") => 1,
            Some("en") => 2,
            _ => 0,
        });
        ui.set_pref_ret_size(p.library.retention.by == znimok_settings::RetentionBy::Size);
        ui.set_pref_ret_count(p.library.retention.count.to_string().into());
        ui.set_pref_ret_mb(p.library.retention.size_mb.to_string().into());
        ui.set_pref_trash_days(p.library.trash_days.to_string().into());
        ui.set_pref_rec_fps(if p.video.fps >= 45 { 60 } else { 30 });
        ui.set_pref_rec_quality(match p.video.quality {
            znimok_settings::Quality::Small => 0,
            znimok_settings::Quality::Normal => 1,
            znimok_settings::Quality::High => 2,
        });
        ui.set_pref_rec_follow(p.video.follow_window);
        ui.set_pref_rec_sound(
            i32::from(p.video.audio.system) | (i32::from(p.video.audio.microphone) << 1),
        );
        ui.set_pref_rec_cursor(p.video.cursor);
        ui.set_pref_lib_dir(self.lib_dir.display().to_string().into());
        ui.set_pref_file(
            self.store
                .as_ref()
                .map(|s| s.path().display().to_string())
                .unwrap_or_default()
                .into(),
        );
        ui.set_pref_version(
            self.tr
                .tr_args(
                    "about-version-line",
                    &args(&[("version", env!("CARGO_PKG_VERSION").to_string())]),
                )
                .into(),
        );
    }

    /// One control of the settings page changed. Numbers come as `value`; texts are read from
    /// the page's own fields.
    pub fn setting(&mut self, ui: &AppWindow, key: &str, value: i32) {
        let on = value != 0;
        match key {
            "capture" => {
                self.save_prefs(ui, |p| p.capture.enabled = on);
                let p = self.prefs();
                crate::hotkeys::apply(&p.capture.hotkeys, on);
            }
            "key-record" => {
                self.hotkey_record(ui, value);
                return;
            }
            "keys-defaults" => {
                self.save_prefs(ui, |p| p.capture.hotkeys = Default::default());
                let p = self.prefs();
                crate::hotkeys::apply(&p.capture.hotkeys, p.capture.enabled);
                self.show_capture_key(ui);
            }
            "quick-library" => self.save_prefs(ui, |p| p.capture.quick_save_to_library = on),
            "show-hints" => self.save_prefs(ui, |p| p.capture.show_hints = on),
            // Ten clicks on «Changes apply immediately» (ZK-140): the developer page shows for
            // this build, or hides again.
            "dev-toggle" => {
                let id = build_id();
                let shown = self.prefs().general.developer_page.as_deref() == Some(id.as_str());
                self.save_prefs(ui, |p| p.general.developer_page = (!shown).then_some(id));
                if shown && ui.get_settings_page() == 7 {
                    ui.set_settings_page(6);
                }
                self.settings_sync(ui);
            }
            // A gesture gets an action; the gesture that had it takes the old one (ZK-155).
            "gesture-plain" | "gesture-shift" | "gesture-alt" => {
                use znimok_settings::{CaptureAction as A, Gesture as G};
                let g = match key {
                    "gesture-shift" => G::Shift,
                    "gesture-alt" => G::Alt,
                    _ => G::Plain,
                };
                let a = match value {
                    1 => A::OverScreen,
                    2 => A::Clipboard,
                    _ => A::Editor,
                };
                self.save_prefs(ui, |p| p.capture.gestures.set(g, a));
                self.settings_sync(ui);
            }
            "keep-tool" => self.save_prefs(ui, |p| p.editor.keep_tool = on),
            "autosave" => {
                self.autosave = on;
                ui.set_autosave(on);
                self.save_prefs(ui, |p| p.editor.autosave = on);
            }
            "metadata" => {
                crate::filemeta::set_enabled(on);
                ui.set_export_meta(on);
                self.save_prefs(ui, |p| p.editor.write_metadata = on);
            }
            "updates" => self.save_prefs(ui, |p| p.updates.check_daily = on),
            // Agents page (ZK-132): the switch the MCP server checks, revoking, the log file.
            "mcp" => {
                self.save_prefs(ui, |p| p.agents.mcp_enabled = on);
                ui.set_pref_mcp(on);
            }
            "agents-revoke" => {
                if let Some(perm) = znimok_agents::permissions::Permissions::open_default() {
                    let _ = perm.revoke_all();
                }
                let p = self.prefs();
                self.agents_sync(ui, &p);
            }
            "upd-check" => {
                self.update_check(ui, true);
                return;
            }
            "upd-install" => {
                self.update_install(ui);
                return;
            }
            "upd-page" => {
                match &self.update_found {
                    Some(crate::update::Found::Available(a)) => crate::update::open_page(&a.page),
                    Some(crate::update::Found::Sparkle { notes: Some(n), .. }) => {
                        crate::update::open_page(n)
                    }
                    _ => {}
                }
                return;
            }
            "agents-log" => {
                if let Some(a) = znimok_agents::audit::Audit::open_default() {
                    crate::library::show_in_folder(a.path());
                }
            }
            "theme" => {
                use znimok_settings::Theme as T;
                let t = match value {
                    1 => T::Light,
                    2 => T::Dark,
                    _ => T::Auto,
                };
                self.save_prefs(ui, |p| p.general.theme = t);
                let p = self.prefs();
                self.apply_theme(ui, &p);
                self.dirty = true;
            }
            "lang" => {
                let lang = match value {
                    1 => Some("uk".to_string()),
                    2 => Some("en".to_string()),
                    _ => None,
                };
                self.save_prefs(ui, |p| p.general.language = lang.clone());
                // Live: the Slint strings and the app's own messages switch at once.
                let l = znimok_i18n::choose_language(
                    lang.as_deref(),
                    znimok_i18n::system_language().as_deref(),
                );
                let _ = slint::select_bundled_translation(l);
                self.tr = std::rc::Rc::new(Localizer::new(l));
                self.show_cards(ui);
                self.sync(ui);
            }
            "ret-by" => self.save_prefs(ui, |p| {
                p.library.retention.by = if on {
                    znimok_settings::RetentionBy::Size
                } else {
                    znimok_settings::RetentionBy::Count
                }
            }),
            "ret-count" => {
                let n = ui.get_pref_ret_count().trim().parse::<u32>().unwrap_or(100);
                self.save_prefs(ui, |p| p.library.retention.count = n);
            }
            // Recording (ZK-180); the sound's choice is kept for ZK-89 (ZK-189).
            "rec-fps" => self.save_prefs(ui, |p| p.video.fps = if value >= 45 { 60 } else { 30 }),
            "rec-quality" => self.save_prefs(ui, |p| {
                p.video.quality = match value {
                    0 => znimok_settings::Quality::Small,
                    2 => znimok_settings::Quality::High,
                    _ => znimok_settings::Quality::Normal,
                }
            }),
            "rec-follow" => self.save_prefs(ui, |p| p.video.follow_window = on),
            "rec-sound" => self.save_prefs(ui, |p| {
                p.video.audio.system = value & 1 != 0;
                p.video.audio.microphone = value & 2 != 0;
            }),
            "rec-cursor" => self.save_prefs(ui, |p| {
                p.video.cursor = on;
                p.video.clicks = on;
            }),
            "trash-days" => {
                let n = ui
                    .get_pref_trash_days()
                    .trim()
                    .parse::<u32>()
                    .unwrap_or(7)
                    .clamp(1, 3650);
                self.save_prefs(ui, |p| p.library.trash_days = n);
                library::purge_trash(&self.lib_dir, n as u64);
                self.refresh_library(ui);
            }
            "ret-mb" => {
                let n = ui.get_pref_ret_mb().trim().parse::<u64>().unwrap_or(500);
                self.save_prefs(ui, |p| p.library.retention.size_mb = n);
            }
            "lib-folder" => {
                let Some(dir) = rfd::FileDialog::new()
                    .set_directory(&self.lib_dir)
                    .pick_folder()
                else {
                    return;
                };
                self.save_prefs(ui, |p| p.library.dir = Some(dir.clone()));
                self.lib_dir = dir;
                self.refresh_library(ui);
            }
            "lib-default" => {
                self.save_prefs(ui, |p| p.library.dir = None);
                self.lib_dir = library::default_dir();
                self.refresh_library(ui);
            }
            "show-folder" => crate::library::show_in_folder(&self.lib_dir),
            "close" => {
                self.settings_close(ui);
                return;
            }
            "autostart" => {
                if let Err(e) = crate::system::set_autostart(on) {
                    self.toast(ui, e);
                }
            }
            "onb-open" => {
                self.onboarding_open(ui);
                return;
            }
            "onb-done" => {
                self.onboarding_done(ui, on);
                return;
            }
            "onb-screen" => crate::system::ask_screen(),
            "dev-onboarding" => {
                self.save_prefs(ui, |p| p.general.onboarding_done = false);
                self.onboarding_open(ui);
                return;
            }
            "dev-pill" => {
                let raster = self
                    .s
                    .as_ref()
                    .map(|s| (*s.ed.doc.banks[s.ed.doc.source as usize]).clone())
                    .unwrap_or_else(|| Raster::new(320, 200, vec![200; 320 * 200 * 4]));
                let path = self
                    .s
                    .as_ref()
                    .map(|s| s.path.clone())
                    .unwrap_or_else(|| self.lib_dir.join("demo.znimok"));
                let (w, h) = (raster.width, raster.height);
                let heading = self.tr.tr("pill-region-copied");
                let sub = self.tr.tr_args(
                    "pill-where",
                    &args(&[("width", w.to_string()), ("height", h.to_string())]),
                );
                crate::pill::show(
                    raster,
                    path,
                    "Znimok".into(),
                    heading,
                    sub,
                    znimok_platform::Rect::new(0, 0, 1920, 1080),
                );
            }
            "dev-crash" => {
                let ctx = crate::CTX.with(|c| c.borrow().clone());
                if let Some((app, _)) = ctx {
                    let title = self.tr.tr("crash-title");
                    let buttons = vec![self.tr.tr("common-close")];
                    let _ = app;
                    crate::dialog::ask(ui, title, "(test)".into(), buttons, 0, Some(0), |_, _| {});
                }
            }
            "dev-open-settings" => {
                if let Some(dir) = self
                    .store
                    .as_ref()
                    .and_then(|s| s.path().parent().map(Path::to_path_buf))
                {
                    library::show_in_folder(&dir);
                }
            }
            "dev-open-logs" => library::show_in_folder(&znimok_log::logs_dir()),
            "dev-empty-trash" => {
                let _ = std::fs::remove_dir_all(library::trash_dir(&self.lib_dir));
                let msg = self.tr.tr("dev-done");
                self.toast(ui, msg);
            }
            "dev-reset" => {
                if let Some(store) = self.store.as_ref() {
                    let _ = store.reset();
                }
                let store = self.store.take();
                self.use_settings(ui, store);
                let p = self.prefs();
                crate::hotkeys::apply(&p.capture.hotkeys, p.capture.enabled);
                self.show_capture_key(ui);
                let msg = self.tr.tr("dev-done");
                self.toast(ui, msg);
            }
            "onb-keys" => {
                self.recheck_timer.stop();
                // Back / Esc returns to the guide, not past it (owner, 29.09).
                self.settings_from = 3;
                ui.set_settings_page(1);
                ui.set_page(2);
                ui.invoke_focus_settings();
            }
            "reset" => {
                if let Some(store) = self.store.as_ref() {
                    let _ = store.reset();
                }
                let store = self.store.take();
                self.use_settings(ui, store);
            }
            _ => {}
        }
        // Retention applies when its limit changes.
        if key.starts_with("ret-") {
            self.apply_retention(ui);
        }
        self.settings_sync(ui);
    }

    // ------------------------------------------------------------------ library

    pub fn refresh_library(&mut self, ui: &AppWindow) {
        if self.role == crate::wins::Role::Editor {
            // The cards and the index live in the library window (ZK-107).
            crate::wins::library_later(|a, ui| a.refresh_library(ui));
            return;
        }
        if self.trash_view {
            // The trash (ZK-175): what is over its days goes first; no index (it is small).
            library::purge_trash(&self.lib_dir, self.prefs().library.trash_days as u64);
            let dir = library::trash_dir(&self.lib_dir);
            self.lib_fp = library::fingerprint(&dir);
            self.entries = library::scan(&dir, None);
            self.entries
                .sort_by_key(|e| std::cmp::Reverse(library::trashed_at_ms(&e.path).unwrap_or(0)));
        } else {
            if self.index.as_ref().is_none_or(|(d, _)| *d != self.lib_dir) {
                self.index = library::Index::open(&self.lib_dir).map(|i| (self.lib_dir.clone(), i));
            }
            self.lib_fp = library::fingerprint(&self.lib_dir);
            self.entries = library::scan(&self.lib_dir, self.index.as_ref().map(|(_, i)| i));
        }
        ui.set_trash_count(library::trash_count(&self.lib_dir) as i32);
        self.show_cards(ui);
    }

    /// The folder the library page shows now: the library, or its trash.
    fn shown_dir(&self) -> PathBuf {
        if self.trash_view {
            library::trash_dir(&self.lib_dir)
        } else {
            self.lib_dir.clone()
        }
    }

    /// The watcher (ZK-131): while the library is on screen, a look at the folder every 2 s;
    /// files added, removed, renamed or synced from outside show up by themselves.
    pub fn lib_poll(&mut self, ui: &AppWindow, now: bool) {
        if ui.get_page() != 0
            || (!now
                && self
                    .lib_polled
                    .is_some_and(|t| t.elapsed().as_millis() < 2000))
        {
            return;
        }
        self.lib_polled = Some(Instant::now());
        if library::fingerprint(&self.shown_dir()) != self.lib_fp {
            self.refresh_library(ui);
        }
    }

    /// Card: to the trash, with "Undo" in the status line (ZK-55).
    pub fn lib_trash(&mut self, ui: &AppWindow, path: &Path) {
        self.trash_paths(ui, vec![path.to_path_buf()]);
    }

    /// Documents to the trash, with one "Undo" for all of them (ZK-176).
    fn trash_paths(&mut self, ui: &AppWindow, paths: Vec<PathBuf>) {
        self.undo_trash.clear();
        let mut failed = None;
        for path in &paths {
            // An open document: its window goes first (autosave is on, or the user saved).
            crate::wins::close_editor_of(path);
            match library::move_to_trash(&self.lib_dir, path) {
                Ok(to) => self.undo_trash.push((to, path.clone())),
                Err(e) => failed = Some(e),
            }
        }
        self.clear_picks();
        match failed {
            Some(e) => {
                let msg = format!("{} ({e})", self.tr.tr("lib-error-delete"));
                self.toast(ui, msg);
            }
            None => {
                let n = self.undo_trash.len();
                let msg = if n == 1 {
                    self.tr.tr("lib-trashed-toast")
                } else {
                    self.tr
                        .tr_args("lib-trashed-many-toast", &count_args("count", n as i64))
                };
                self.toast(ui, msg);
                ui.set_toast_action(self.tr.tr("lib-undo").into());
            }
        }
        self.refresh_library(ui);
    }

    /// From the trash back into the library (ZK-175).
    fn restore_paths(&mut self, ui: &AppWindow, paths: Vec<PathBuf>) {
        let mut ok = 0;
        let mut failed = None;
        for p in &paths {
            match library::restore_to_library(&self.lib_dir, p) {
                Ok(_) => ok += 1,
                Err(e) => failed = Some(e),
            }
        }
        self.clear_picks();
        let msg = match failed {
            Some(e) => format!("{} ({e})", self.tr.tr("lib-error-delete")),
            None => self
                .tr
                .tr_args("trash-restored-toast", &count_args("count", ok as i64)),
        };
        self.toast(ui, msg);
        self.refresh_library(ui);
    }

    /// Deleted for good, after one question (ZK-175): the trash's cards, the picked ones, or
    /// everything in the trash.
    fn ask_destroy(&mut self, ui: &AppWindow, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            return;
        }
        let (title, body, yes, no) = (
            self.tr.tr("trash-destroy-title"),
            self.tr.tr_args(
                "trash-destroy-body",
                &count_args("count", paths.len() as i64),
            ),
            self.tr.tr("trash-destroy"),
            self.tr.tr("common-cancel"),
        );
        let me = self.me();
        crate::dialog::ask(
            ui,
            title,
            body,
            vec![yes, no],
            1,
            Some(1),
            move |_, answer| {
                if answer == Some(0) {
                    me.with(|a, ui| a.destroy_paths(ui, &paths));
                }
            },
        );
    }

    fn destroy_paths(&mut self, ui: &AppWindow, paths: &[PathBuf]) {
        // Every file is removed; the last failure (if any) is what the message says.
        let mut failed = None;
        for p in paths {
            if let Err(e) = std::fs::remove_file(p) {
                failed = Some(e);
            }
        }
        self.clear_picks();
        let msg = match failed {
            Some(e) => format!("{} ({e})", self.tr.tr("lib-error-delete")),
            None => self.tr.tr("lib-deleted-forever-toast"),
        };
        ui.set_toast_action("".into());
        self.toast(ui, msg);
        self.refresh_library(ui);
    }

    fn clear_picks(&mut self) {
        self.picked.clear();
        self.pick_anchor = None;
    }

    /// A card clicked (ZK-176): `mode` 0 a plain click (the library opens the document, the
    /// trash picks just this card — nothing opens from the trash, ZK-175), 1 Ctrl / ⌘ (one more
    /// or one less), 2 Shift (the range from the last clicked card).
    pub fn card_click(&mut self, ui: &AppWindow, path: &Path, mode: i32) {
        let path = path.to_path_buf();
        self.set_current(ui, Some(path.clone()));
        match mode {
            1 => {
                if let Some(i) = self.picked.iter().position(|p| *p == path) {
                    self.picked.remove(i);
                } else {
                    self.picked.push(path.clone());
                }
                self.pick_anchor = Some(path);
            }
            2 => {
                let anchor = self.pick_anchor.clone().unwrap_or_else(|| path.clone());
                let at = |p: &PathBuf| self.shown.iter().position(|s| s == p);
                match (at(&anchor), at(&path)) {
                    (Some(a), Some(b)) => {
                        self.picked = self.shown[a.min(b)..=a.max(b)].to_vec();
                    }
                    _ => self.picked = vec![path.clone()],
                }
                self.pick_anchor = Some(anchor);
            }
            // Alt (⌥)+click: in a new window (ZK-192).
            3 if !self.trash_view => {
                self.clear_picks();
                self.show_picks(ui);
                self.open_path(ui, &path);
                return;
            }
            // A plain click: in this window — the library and the editor are one (ZK-192).
            _ if !self.trash_view => {
                self.clear_picks();
                self.show_picks(ui);
                self.open_here = true;
                self.open_path(ui, &path);
                self.open_here = false;
                return;
            }
            _ => {
                self.picked = vec![path.clone()];
                self.pick_anchor = Some(path);
            }
        }
        self.show_picks(ui);
    }

    /// The library page's actions (ZK-175/176). `path` is the card's, for the card buttons.
    pub fn lib_action(&mut self, ui: &AppWindow, what: &str, path: &Path) {
        match what {
            "view-library" | "view-trash" => {
                let trash = what == "view-trash";
                if trash != self.trash_view {
                    self.trash_view = trash;
                    self.clear_picks();
                    self.refresh_library(ui);
                }
            }
            "pick-all" => {
                self.picked = self.shown.clone();
                self.show_picks(ui);
            }
            "pick-none" => {
                if self.picked.is_empty() {
                    self.set_current(ui, None);
                }
                self.clear_picks();
                self.show_picks(ui);
            }
            "picked-trash" if !self.trash_view => {
                let v = self.picked.clone();
                self.trash_paths(ui, v);
            }
            "pin" => {
                let on = !self.entries.iter().any(|e| e.path == path && e.pinned);
                self.pin_paths(ui, vec![path.to_path_buf()], on);
            }
            // All picked pinned → unpin them; otherwise pin them all.
            "picked-pin" if !self.trash_view => {
                let v = self.picked.clone();
                let all = v
                    .iter()
                    .all(|p| self.entries.iter().any(|e| e.path == *p && e.pinned));
                self.pin_paths(ui, v, !all);
            }
            "restore" => self.restore_paths(ui, vec![path.to_path_buf()]),
            "picked-restore" if self.trash_view => {
                let v = self.picked.clone();
                self.restore_paths(ui, v);
            }
            "destroy" => self.ask_destroy(ui, vec![path.to_path_buf()]),
            "picked-destroy" if self.trash_view => {
                let v = self.picked.clone();
                self.ask_destroy(ui, v);
            }
            "trash-empty" => {
                let dir = library::trash_dir(&self.lib_dir);
                let all: Vec<PathBuf> = std::fs::read_dir(&dir)
                    .map(|rd| {
                        rd.flatten()
                            .map(|e| e.path())
                            .filter(|p| p.is_file())
                            .collect()
                    })
                    .unwrap_or_default();
                self.ask_destroy(ui, all);
            }
            _ => {}
        }
    }

    /// The pick marks on the cards and the count, in place.
    fn show_picks(&self, ui: &AppWindow) {
        use slint::Model;
        let m = &self.cards_model;
        for i in 0..m.row_count() {
            if let Some(mut c) = m.row_data(i) {
                let want = self
                    .picked
                    .iter()
                    .any(|p| p.as_os_str() == std::ffi::OsStr::new(c.path.as_str()));
                if c.selected != want {
                    c.selected = want;
                    m.set_row_data(i, c);
                }
            }
        }
        ui.set_picked_count(self.picked.len() as i32);
        let all_pinned = !self.picked.is_empty()
            && self
                .picked
                .iter()
                .all(|p| self.entries.iter().any(|e| e.path == *p && e.pinned));
        ui.set_picked_pinned(all_pinned);
    }

    /// Shift+trash on a card (owner 29.09): the file is deleted, not moved to the trash.
    pub fn lib_delete_forever(&mut self, ui: &AppWindow, path: &Path) {
        crate::wins::close_editor_of(path);
        let msg = match std::fs::remove_file(path) {
            Ok(()) => self.tr.tr("lib-deleted-forever-toast"),
            Err(e) => format!("{} ({e})", self.tr.tr("lib-error-delete")),
        };
        ui.set_toast_action("".into());
        self.toast(ui, msg);
        self.refresh_library(ui);
    }

    /// "Undo" next to the message: the last trashed document comes back.
    pub fn toast_action(&mut self, ui: &AppWindow) {
        ui.set_toast_action("".into());
        let undo = std::mem::take(&mut self.undo_trash);
        if !undo.is_empty() {
            let back = undo
                .iter()
                .filter(|(trashed, original)| library::restore(trashed, original).is_ok())
                .count();
            if back > 0 {
                ui.set_toast("".into());
                self.refresh_library(ui);
            }
        }
    }

    /// Card: a new name for the document (inside the file; the file name stays).
    pub fn lib_rename(&mut self, ui: &AppWindow, path: &Path, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            self.show_cards(ui);
            return;
        }
        // Open in an editor window (ZK-107): renamed there, so the two do not disagree.
        if let Some((app, win)) = crate::wins::editor_of(path)
            && let Ok(mut a) = app.try_borrow_mut()
        {
            a.rename_open(&win, name);
            self.refresh_library(ui);
            return;
        }
        let r = znimok_format::open_parts(path)
            .map_err(|e| e.to_string())
            .and_then(|(mut doc, video)| {
                doc.name = name.to_string();
                let opts = self.options_for(&doc);
                let _guard = SAVE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
                znimok_format::save_same_kind(path, &doc, video.as_ref(), &opts)
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            });
        if let Err(e) = r {
            let msg = format!("{} ({e})", self.tr.tr("lib-error-rename"));
            self.toast(ui, msg);
        }
        self.refresh_library(ui);
    }

    /// The card's new name for the document open in this window.
    pub fn rename_open(&mut self, ui: &AppWindow, name: &str) {
        self.apply(
            ui,
            Command::SetName {
                name: name.to_string(),
            },
        );
        self.save_now(ui);
    }

    /// Keeps the library within its limit (settings → library): the oldest screenshots go to
    /// the trash, never an open one.
    pub fn apply_retention(&mut self, ui: &AppWindow) {
        if self.role == crate::wins::Role::Editor {
            crate::wins::library_later(|a, ui| a.apply_retention(ui));
            return;
        }
        let r = self.prefs().library.retention;
        let open = crate::wins::open_paths();
        let entries = library::scan(&self.lib_dir, self.index.as_ref().map(|(_, i)| i));
        let mut used: u64 = 0;
        let mut kept: u32 = 0;
        let mut gone = 0;
        for e in &entries {
            // Pinned documents are never trashed by the limit, nor counted in it (ZK-178).
            if e.pinned {
                continue;
            }
            let size = std::fs::metadata(&e.path).map(|m| m.len()).unwrap_or(0);
            let over = match r.by {
                znimok_settings::RetentionBy::Count => kept >= r.count.max(1),
                znimok_settings::RetentionBy::Size => used + size > r.size_mb.max(1) * 1024 * 1024,
            };
            if over && !open.iter().any(|p| p == &e.path) {
                if library::move_to_trash(&self.lib_dir, &e.path).is_ok() {
                    gone += 1;
                }
                continue;
            }
            kept += 1;
            used += size;
        }
        if gone > 0 {
            self.refresh_library(ui);
        }
    }

    pub fn set_filter(&mut self, ui: &AppWindow, f: &str) {
        self.filter = f.trim().to_string();
        self.show_cards(ui);
    }

    fn show_cards(&mut self, ui: &AppWindow) {
        let days = self.prefs().library.trash_days as i64;
        // Groups (ZK-177): pinned first (not in the trash), then by date — of capture in the
        // library, of deletion in the trash. Each group keeps the newest first.
        let when = |e: &library::Entry| {
            if self.trash_view {
                library::trashed_at_ms(&e.path).unwrap_or(0)
            } else {
                e.created_ms
            }
        };
        let mut keyed: Vec<(usize, i64, &library::Entry)> = self
            .entries
            .iter()
            .filter(|e| e.matches(&self.filter))
            .map(|e| {
                let t = when(e);
                let g = if e.pinned && !self.trash_view {
                    0
                } else {
                    1 + date_bucket(t)
                };
                (g, t, e)
            })
            .collect();
        keyed.sort_by_key(|(g, t, _)| (*g, std::cmp::Reverse(*t)));
        let mut groups: Vec<(String, usize)> = Vec::new();
        let mut last = usize::MAX;
        for (g, t, _) in &keyed {
            if *g != last {
                groups.push((self.group_title(*g, *t), 0));
                last = *g;
            }
            if let Some(x) = groups.last_mut() {
                x.1 += 1;
            }
        }
        self.groups = groups;
        let shown: Vec<&library::Entry> = keyed.iter().map(|(_, _, e)| *e).collect();
        let cards: Vec<CardData> = shown
            .iter()
            .map(|e| CardData {
                name: e.name.as_str().into(),
                meta: if self.trash_view {
                    self.trash_meta(e, days)
                } else {
                    e.meta_line()
                }
                .into(),
                thumb: e
                    .thumb_png
                    .as_deref()
                    .and_then(library::thumb_image)
                    .unwrap_or_default(),
                path: e.path.display().to_string().into(),
                selected: self.picked.contains(&e.path),
                pinned: e.pinned,
                current: self.cur.as_ref() == Some(&e.path),
                ..Default::default()
            })
            .collect();
        self.shown = shown.iter().map(|e| e.path.clone()).collect();
        // Picks of cards gone (trashed, filtered out, removed from outside) go too.
        let shown_now = &self.shown;
        self.picked.retain(|p| shown_now.contains(p));
        if self.cur.as_ref().is_some_and(|c| !shown_now.contains(c)) {
            self.cur = None;
        }
        self.cards_model = std::rc::Rc::new(VecModel::from(cards));
        ui.set_cards(self.cards_model.clone().into());
        self.lay_out_cards(ui);
        ui.set_trash_view(self.trash_view);
        ui.set_picked_count(self.picked.len() as i32);
        ui.set_library_dir(self.shown_dir().display().to_string().into());
    }

    /// A group's title (ZK-177): 0 pinned, then [`date_bucket`] + 1.
    fn group_title(&self, g: usize, t: i64) -> String {
        match g {
            0 => self.tr.tr("lib-group-pinned"),
            1 => self.tr.tr("lib-group-today"),
            2 => self.tr.tr("lib-group-yesterday"),
            3 => self.tr.tr("lib-group-week"),
            4 => self.tr.tr("lib-group-month"),
            _ => {
                use chrono::Datelike;
                let d = chrono::Local
                    .timestamp_millis_opt(t)
                    .single()
                    .unwrap_or_else(chrono::Local::now);
                let month = self.tr.tr(&format!("month-{}", d.month()));
                if d.year() == chrono::Local::now().year() {
                    month
                } else {
                    format!("{month} {}", d.year())
                }
            }
        }
    }

    /// The grid's columns changed (the window was resized): the cards and titles take new
    /// places (ZK-177).
    pub fn lib_layout(&mut self, ui: &AppWindow, cols: i32) {
        let cols = cols.max(1) as usize;
        if cols != self.lib_cols {
            self.lib_cols = cols;
            self.lay_out_cards(ui);
        }
    }

    /// Places of the cards and group titles in the grid, in rows (ZK-177): for a title the
    /// titles and card rows above it; for a card the titles up to its own and the card rows
    /// above its row, and its column. Slint turns them into pixels.
    fn lay_out_cards(&self, ui: &AppWindow) {
        use slint::Model;
        let cols = self.lib_cols.max(1);
        let mut headers = Vec::new();
        let (mut hb, mut rb, mut i) = (0i32, 0i32, 0usize);
        let m = &self.cards_model;
        for (title, n) in &self.groups {
            headers.push(crate::LibHeader {
                title: title.as_str().into(),
                hb,
                rb,
            });
            hb += 1;
            for k in 0..*n {
                if let Some(mut c) = m.row_data(i) {
                    let (r, col) = (rb + (k / cols) as i32, (k % cols) as i32);
                    if (c.hb, c.rb, c.col) != (hb, r, col) {
                        (c.hb, c.rb, c.col) = (hb, r, col);
                        m.set_row_data(i, c);
                    }
                }
                i += 1;
            }
            rb += n.div_ceil(cols) as i32;
        }
        ui.set_lib_headers(std::rc::Rc::new(VecModel::from(headers)).into());
        ui.set_lib_hb(hb);
        ui.set_lib_rb(rb);
    }

    /// The keyboard in the grid (ZK-179): arrows, Home / End (Shift: the pick grows from the
    /// last clicked card), Enter opens, Space picks, Delete, F2 renames.
    pub fn lib_key(&mut self, ui: &AppWindow, key: &str, shift: bool) {
        if self.shown.is_empty() {
            return;
        }
        let at = self
            .cur
            .as_ref()
            .and_then(|c| self.shown.iter().position(|p| p == c));
        let target = match (key, at) {
            ("left" | "right" | "up" | "down" | "home" | "end", None) => Some(0),
            ("left", Some(i)) => Some(i.saturating_sub(1)),
            ("right", Some(i)) => Some((i + 1).min(self.shown.len() - 1)),
            ("up", Some(i)) => Some(self.grid_step(i, false)),
            ("down", Some(i)) => Some(self.grid_step(i, true)),
            ("home", _) => Some(0),
            ("end", _) => Some(self.shown.len() - 1),
            _ => None,
        };
        if let Some(t) = target {
            let path = self.shown[t].clone();
            if shift {
                let anchor = self
                    .pick_anchor
                    .clone()
                    .or_else(|| self.cur.clone())
                    .unwrap_or_else(|| path.clone());
                let a = self.shown.iter().position(|p| *p == anchor).unwrap_or(t);
                self.picked = self.shown[a.min(t)..=a.max(t)].to_vec();
                self.pick_anchor = Some(anchor);
            }
            self.set_current(ui, Some(path));
            self.show_picks(ui);
            return;
        }
        let Some(i) = at else { return };
        let path = self.shown[i].clone();
        match key {
            "enter" if !self.trash_view => self.card_click(ui, &path, 0),
            "space" => self.card_click(ui, &path, 1),
            "delete" if !self.picked.is_empty() => {
                let v = self.picked.clone();
                if self.trash_view {
                    self.ask_destroy(ui, v);
                } else {
                    self.trash_paths(ui, v);
                }
            }
            "delete" if self.trash_view => self.ask_destroy(ui, vec![path]),
            "delete" => self.trash_paths(ui, vec![path]),
            "f2" if !self.trash_view => ui.set_lib_rename_tick(ui.get_lib_rename_tick() + 1),
            _ => {}
        }
    }

    /// One row up or down from card `i`, through the groups: the same column where it can be,
    /// else the nearest card (a short last row, a shorter group).
    fn grid_step(&self, i: usize, down: bool) -> usize {
        let cols = self.lib_cols.max(1);
        // Which group, and where in it.
        let (mut g, mut start) = (0usize, 0usize);
        while g < self.groups.len() && i >= start + self.groups[g].1 {
            start += self.groups[g].1;
            g += 1;
        }
        let Some(&(_, n)) = self.groups.get(g) else {
            return i;
        };
        let k = i - start;
        if down {
            if k + cols < n {
                return i + cols;
            }
            if k / cols < (n - 1) / cols {
                return start + n - 1;
            }
            match self.groups.get(g + 1) {
                Some(&(_, n2)) => start + n + (k % cols).min(n2 - 1),
                None => i,
            }
        } else {
            if k >= cols {
                return i - cols;
            }
            if g == 0 {
                return i;
            }
            let n0 = self.groups[g - 1].1;
            let s0 = start - n0;
            let last_row = (n0 - 1) / cols * cols;
            s0 + (last_row + k % cols).min(n0 - 1)
        }
    }

    /// The keyboard's card: its ring, and the grid scrolls to it.
    fn set_current(&mut self, ui: &AppWindow, cur: Option<PathBuf>) {
        use slint::Model;
        self.cur = cur;
        let m = &self.cards_model;
        for i in 0..m.row_count() {
            if let Some(mut c) = m.row_data(i) {
                let want = self
                    .cur
                    .as_ref()
                    .is_some_and(|p| p.as_os_str() == std::ffi::OsStr::new(c.path.as_str()));
                if c.current != want {
                    c.current = want;
                    if want {
                        ui.invoke_lib_scroll_to(c.hb, c.rb);
                    }
                    m.set_row_data(i, c);
                }
            }
        }
    }

    /// Pin or unpin documents (ZK-178): written into each file; an open one through its window.
    fn pin_paths(&mut self, ui: &AppWindow, paths: Vec<PathBuf>, on: bool) {
        let mut failed = None;
        for path in &paths {
            if let Some((app, win)) = crate::wins::editor_of(path)
                && let Ok(mut a) = app.try_borrow_mut()
            {
                a.pin_open(&win, on);
                continue;
            }
            let r = znimok_format::open_parts(path)
                .map_err(|e| e.to_string())
                .and_then(|(mut doc, video)| {
                    doc.meta.pinned = on;
                    let opts = self.options_for(&doc);
                    let _guard = SAVE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
                    znimok_format::save_same_kind(path, &doc, video.as_ref(), &opts)
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                });
            if let Err(e) = r {
                failed = Some(e);
            }
        }
        if let Some(e) = failed {
            let msg = format!("{} ({e})", self.tr.tr("err-library-save"));
            self.toast(ui, msg);
        }
        self.refresh_library(ui);
    }

    /// The pin of the document open in this window, saved at once.
    pub fn pin_open(&mut self, ui: &AppWindow, on: bool) {
        let Some(s) = self.s.as_ref() else { return };
        let mut meta = s.ed.doc.meta.clone();
        meta.pinned = on;
        self.apply(ui, Command::SetMeta { meta });
        self.save_now(ui);
    }

    /// A trashed card's line: when it went, and in how many days it goes for good (ZK-175).
    fn trash_meta(&self, e: &library::Entry, days: i64) -> String {
        let at = library::trashed_at_ms(&e.path).unwrap_or(0);
        let when = chrono::Local
            .timestamp_millis_opt(at)
            .single()
            .map(|t| t.format("%d.%m %H:%M").to_string())
            .unwrap_or_default();
        let left_ms = at + days * 86_400_000 - chrono::Local::now().timestamp_millis();
        let left = (left_ms + 86_399_999).div_euclid(86_400_000).max(1);
        let mut a = count_args("days", left);
        a.set("date", when);
        self.tr.tr_args("trash-card-meta", &a)
    }

    pub fn toast(&mut self, ui: &AppWindow, text: impl Into<SharedString>) {
        ui.set_toast(text.into());
        ui.set_toast_action(SharedString::new());
        self.toast_at = Some(Instant::now());
    }

    /// Messages go after 3.5 s; one with an action ("Undo") stays 8 s, then the action is gone.
    pub fn tick_toast(&mut self, ui: &AppWindow) {
        let keep = if ui.get_toast_action().is_empty() {
            3.5
        } else {
            8.0
        };
        if self
            .toast_at
            .is_some_and(|t| t.elapsed().as_secs_f32() > keep)
        {
            self.toast_at = None;
            ui.set_toast(SharedString::new());
            ui.set_toast_action(SharedString::new());
            self.undo_trash.clear();
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

    /// An empty editor (hotkey): a white 1280 × 800 sheet to draw on or paste into.
    pub fn open_blank(&mut self, ui: &AppWindow) {
        let (w, h) = (1280u32, 800u32);
        let raster = Raster::new(w, h, vec![255; (w * h * 4) as usize]);
        self.new_document(ui, raster, "blank", None);
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
        self.open_session(ui, Editor::new(doc), path, true, None);
        self.apply_retention(ui);
    }

    /// Alt at the end of a capture (ZK-58): edit right over the frozen display.
    pub fn over_open(
        &mut self,
        ui: &AppWindow,
        raster: Raster,
        frame: IRect,
        source: &'static str,
        display: znimok_platform::Rect,
    ) {
        // A window of its own (ZK-107), like any document.
        if self.takes_no_document() {
            if let Some((app, win)) = self.spawn_editor(ui) {
                app.borrow_mut()
                    .over_open(&win, raster, frame, source, display);
            }
            return;
        }
        let (mut doc, path) = self.build_document(raster, source, None);
        let (w, h) = doc.image_size();
        let img = IRect::new(0, 0, w as i32, h as i32);
        let x0 = frame.x.clamp(0, img.w);
        let y0 = frame.y.clamp(0, img.h);
        let x1 = frame.right().clamp(0, img.w);
        let y1 = frame.bottom().clamp(0, img.h);
        let f = IRect::new(x0, y0, (x1 - x0).max(1), (y1 - y0).max(1));
        doc.crop = (f != img).then_some(f);
        self.open_session(ui, Editor::new(doc), path, true, None);
        let restore = crate::over::enter(ui, display);
        self.over = Some(Over {
            display,
            restore: Some(restore),
            source,
        });
        ui.set_over_screen(true);
        self.fit_pending = true;
        self.dirty = true;
        self.sync(ui);
        ui.window().request_redraw();
    }

    /// "Open in the editor window": the same document, marks and undo; the window comes back
    /// where it was, and from now on it is saved to the library like any other.
    pub fn over_to_window(&mut self, ui: &AppWindow) {
        self.finish_text(ui);
        let Some(mut o) = self.over.take() else {
            return;
        };
        ui.set_over_screen(false);
        if let Some(r) = o.restore.take() {
            crate::over::leave(ui, r);
        }
        if let Some(s) = self.s.as_mut() {
            s.changed_at = Instant::now();
        }
        self.fit_pending = true;
        self.dirty = true;
        self.sync(ui);
        ui.window().request_redraw();
        crate::wins::come_back();
    }

    /// Leaves "over the screen": `copy` = to the clipboard, `save` = to the library (copying
    /// saves too, as the quick Shift capture does); neither = Esc, nothing is kept.
    pub fn over_finish(&mut self, ui: &AppWindow, copy: bool, save: bool) {
        self.finish_text(ui);
        let Some(mut o) = self.over.take() else {
            return;
        };
        self.commit_crop(ui);
        let flat = if copy || save { self.flatten() } else { None };
        let copied = match (&flat, copy) {
            (Some((w, h, rgba)), true) => Some(io::copy_image(*w, *h, rgba.clone())),
            _ => None,
        };
        let saved = (copy || save) && self.save_now(ui);
        let file = self
            .s
            .as_ref()
            .map(|s| (s.path.clone(), s.ed.doc.name.clone()));
        ui.set_over_screen(false);
        // Out of sight first, so the window does not flash back at its old place.
        let _ = ui.hide();
        if let Some(r) = o.restore.take() {
            crate::over::leave(ui, r);
        }
        self.close_document(ui);
        crate::wins::come_back();
        if let (Some((w, h, rgba)), true, Some((path, name))) = (flat, saved, file) {
            let heading = match copied {
                Some(Ok(())) => self.tr.tr(match o.source {
                    "window" => "pill-window-copied",
                    "screen" => "pill-screen-copied",
                    _ => "pill-region-copied",
                }),
                _ => self.tr.tr("pill-saved"),
            };
            let sub = self.tr.tr_args(
                "pill-where",
                &args(&[("width", w.to_string()), ("height", h.to_string())]),
            );
            crate::pill::show(Raster::new(w, h, rgba), path, name, heading, sub, o.display);
        }
        if let Some(Err(e)) = copied {
            let msg = format!("{} ({e})", self.tr.tr("clipboard-error"));
            self.toast(ui, msg);
        }
    }

    /// Straight to the library without opening the editor (Shift in the capture overlay).
    /// Returns the file and the document's name.
    pub fn store_quietly(
        &mut self,
        ui: &AppWindow,
        raster: Raster,
        source: &str,
    ) -> Result<(PathBuf, String), String> {
        let (doc, path) = self.build_document(raster, source, None);
        let name = doc.name.clone();
        let opts = self.options_for(&doc);
        let _guard = SAVE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _ = std::fs::create_dir_all(&self.lib_dir);
        let r = znimok_format::save(&path, &doc, &opts)
            .map(|()| (path, name))
            .map_err(|e| format!("{} ({e})", self.tr.tr("err-library-save")));
        drop(_guard);
        self.apply_retention(ui);
        if self.s.is_none() {
            self.refresh_library(ui);
        }
        r
    }

    /// This window cannot take a document: it is the library, or it has one already — the
    /// document goes to a new editor window (ZK-107).
    fn takes_no_document(&self) -> bool {
        self.role == crate::wins::Role::Library || self.s.is_some()
    }

    /// A new editor window; a failure to make one is shown here.
    fn spawn_editor(&mut self, ui: &AppWindow) -> Option<(crate::Shared, AppWindow)> {
        match crate::wins::spawn_editor(self) {
            Ok(pair) => Some(pair),
            Err(e) => {
                self.toast(ui, e.to_string());
                None
            }
        }
    }

    fn open_session(
        &mut self,
        ui: &AppWindow,
        ed: Editor,
        path: PathBuf,
        fresh: bool,
        video: Option<znimok_format::VideoPart>,
    ) {
        let here = std::mem::take(&mut self.open_here) && self.s.is_none();
        if !here && self.takes_no_document() {
            if let Some((app, win)) = self.spawn_editor(ui) {
                app.borrow_mut().open_session(&win, ed, path, fresh, video);
            }
            return;
        }
        self.s = Some(Session {
            ed,
            path,
            changed_at: Instant::now(),
            save_failed: fresh,
            video,
            vid: None,
        });
        self.video_open(ui);
        self.drag = None;
        self.editing = None;
        self.crop = None;
        self.compare = false;
        self.size_shown = (0, 0);
        if self.tool == tool::CROP {
            self.tool = tool::SELECT;
            ui.set_tool(self.tool as i32);
        }
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
        // Already open in another window: two windows autosaving one file would overwrite each
        // other — go there, or open a copy that saves as a new document (ZK-192).
        if let Some((_, win)) = crate::wins::editor_of(path) {
            let here = self.open_here;
            let me = self.me();
            let path = path.to_path_buf();
            let name = self.doc_title_of(&path);
            crate::dialog::ask(
                ui,
                self.tr
                    .tr_args("open-twice-title", &args(&[("name", name)])),
                self.tr.tr("open-twice-body"),
                vec![
                    self.tr.tr("open-twice-go"),
                    self.tr.tr("open-twice-copy"),
                    self.tr.tr("common-cancel"),
                ],
                0,
                Some(2),
                move |_, answer| match answer {
                    Some(0) => crate::wins::focus(&win),
                    Some(1) => me.with(|a, ui| {
                        a.open_here = here;
                        a.open_copy(ui, &path);
                        a.open_here = false;
                    }),
                    _ => {}
                },
            );
            return;
        }
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
            match znimok_format::open_parts(path) {
                Ok((doc, video)) => {
                    self.open_session(ui, Editor::new(doc), path.to_path_buf(), false, video);
                }
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

    /// A document's title for a question: its name inside, else the file name.
    fn doc_title_of(&self, path: &Path) -> String {
        library::read_entry(path)
            .map(|e| e.name)
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| {
                path.file_stem()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            })
    }

    /// A copy of a document open elsewhere (ZK-192): its own id and file in the library, named
    /// «… (copy)», not saved until changed — the original is never touched from here.
    pub fn open_copy(&mut self, ui: &AppWindow, path: &Path) {
        match znimok_format::open_parts(path) {
            Ok((mut doc, video)) => {
                doc.id = uuid::Uuid::new_v4();
                doc.name = self
                    .tr
                    .tr_args("doc-copy-name", &args(&[("name", doc.name.clone())]));
                let copy = library::new_path(&self.lib_dir, &doc.id.simple().to_string());
                self.open_session(ui, Editor::new(doc), copy, true, video);
            }
            Err(e) => self.toast(ui, e.to_string()),
        }
    }

    /// For the self-test: the file of this window's document.
    pub fn doc_path(&self) -> Option<PathBuf> {
        self.s.as_ref().map(|s| s.path.clone())
    }

    /// A new document from the clipboard; false (and a message) when there is no picture.
    pub fn open_clipboard(&mut self, ui: &AppWindow) -> bool {
        match io::paste_image() {
            Some(r) => {
                self.new_document(ui, r, "clipboard", None);
                true
            }
            None => {
                let msg = self.tr.tr("open-error-no-image");
                self.toast(ui, msg);
                false
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
        match znimok_format::save_same_kind(&s.path, &s.ed.doc, s.video.as_ref(), &opts) {
            Ok(video) => {
                if s.video.is_some() {
                    s.video = video;
                }
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
    ) -> Option<(
        PathBuf,
        Document,
        znimok_format::WriteOptions,
        Option<znimok_format::VideoPart>,
    )> {
        if !self.autosave
            || self.saving
            || self.over.is_some()
            || self.drag.is_some()
            || self.editing.is_some()
        {
            return None;
        }
        let s = self.s.as_ref()?;
        if !(s.ed.is_dirty() || s.save_failed) || s.changed_at.elapsed().as_millis() < 700 {
            return None;
        }
        let opts = self.write_options()?;
        let s = self.s.as_mut()?;
        let job = (s.path.clone(), s.ed.doc.clone(), opts, s.video.clone());
        s.ed.mark_saved();
        s.save_failed = false;
        self.saving = true;
        ui.set_save_state(2);
        Some(job)
    }

    pub fn save_finished(
        &mut self,
        ui: &AppWindow,
        path: &Path,
        result: Result<Option<znimok_format::VideoPart>, String>,
    ) {
        self.saving = false;
        // A video's stream moved inside the rewritten file: the next save reads it from there.
        if let Ok(Some(v)) = &result
            && let Some(s) = self.s.as_mut()
            && s.path == path
        {
            s.video = Some(v.clone());
        }
        let result = result.map(|_| ());
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
        self.commit_crop(ui);
        self.s = None;
        self.drag = None;
        self.editing = None;
        ui.set_editing(false);
        if self.role == crate::wins::Role::Editor {
            // The window was the document (ZK-107): it goes; the library shows the result.
            crate::wins::destroy(self.me);
            self.refresh_library(ui);
            return;
        }
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
        self.reset_crop_draft();
        self.sync(ui);
    }

    pub fn redo(&mut self, ui: &AppWindow) {
        self.finish_text(ui);
        self.apply(ui, Command::Redo);
        self.reset_crop_draft();
        self.sync(ui);
    }

    /// Undo / redo may change the picture under the crop being edited: start it again from
    /// the document's frame.
    fn reset_crop_draft(&mut self) {
        if self.crop.is_some() {
            self.crop = self.s.as_ref().map(|s| s.ed.doc.frame());
        }
    }

    fn selection(&self) -> Vec<ObjectId> {
        self.s
            .as_ref()
            .map(|s| s.ed.selection().to_vec())
            .unwrap_or_default()
    }

    /// The automatic size of counters and stamps: ~3.6 % of the picture's short side (LH).
    fn badge(&self) -> i32 {
        let side = self
            .s
            .as_ref()
            .map(|s| {
                let f = s.ed.doc.frame();
                f.w.min(f.h)
            })
            .unwrap_or(1000);
        ((side as f64 * 0.036).round() as i32).clamp(28, 96)
    }

    fn style_for(&self, t: usize) -> Style {
        let shape = matches!(t, tool::RECT | tool::ELLIPSE);
        // Without an outline a shape is a plate of the fill colour (ZK-161).
        let plate = self.no_stroke && shape;
        let base = Style {
            color: if plate {
                self.fill.unwrap_or(self.color)
            } else {
                self.color
            },
            thick: THICK[self.thick],
            alpha: self.alpha,
            no_main: plate,
            color2: if plate {
                None
            } else if shape {
                self.fill
            } else if t == tool::TEXT {
                self.text_outline
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
            shadow: if fx_tool(t) {
                self.shadow
            } else {
                Effect::None
            },
            glow: if fx_tool(t) { self.glow } else { Effect::None },
            ..Style::default()
        };
        let badge = self.badge();
        match t {
            tool::MARKER => Style {
                color: self.marker_color,
                thick: self.marker_thick(self.marker_size_i),
                ..base
            },
            tool::COUNTER => Style {
                thick: counter_size(badge, self.counter_size_i),
                ..base
            },
            tool::STAMP => Style {
                color: if self.color == PALETTE[0] {
                    Rgb::GREEN
                } else {
                    self.color
                },
                thick: stamp_size(badge, self.stamp_size_i),
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
        if self.text_px > 0 {
            return self.text_px;
        }
        let base = [18, 24, 36][self.text_size_i.min(2)];
        ((base as f64 * self.text_size_base()).round() as i32).max(10)
    }

    fn pen_data(&self, points: Vec<(i32, i32)>) -> Data {
        Data::Pen {
            points,
            head_front: self.pen_head_end,
            head_back: self.pen_head_start,
        }
    }

    /// A marker thickness step on this picture (ZK-171).
    fn marker_thick(&self, i: usize) -> i32 {
        let side = self
            .s
            .as_ref()
            .map(|s| {
                let f = s.ed.doc.frame();
                f.w.min(f.h)
            })
            .unwrap_or(1080);
        let k = (side as f64 / 1080.0).max(1.0);
        (MARKER_SIZES[i.min(MARKER_SIZES.len() - 1)] as f64 * k).round() as i32
    }

    /// The marker's bar from `a` to `b` (ZK-171): as long as the drag, its own thickness across,
    /// turned along the drag (Shift: in 45° steps). The box is the bar as if level, about its
    /// middle; the angle turns it there.
    fn marker_bar(&self, a: (i32, i32), b: (i32, i32), shift: bool) -> (IRect, u16) {
        let (dx, dy) = ((b.0 - a.0) as f64, (b.1 - a.1) as f64);
        let mut deg = dy.atan2(dx).to_degrees();
        if shift {
            deg = (deg / 45.0).round() * 45.0;
        }
        // A bar has no direction: keep the angle within ±90°, so the handle stays on top.
        if deg > 90.0 {
            deg -= 180.0;
        } else if deg <= -90.0 {
            deg += 180.0;
        }
        let len = dx.hypot(dy).round().max(1.0) as i32;
        let h = self.marker_thick(self.marker_size_i);
        let (cx, cy) = ((a.0 + b.0) as f64 / 2.0, (a.1 + b.1) as f64 / 2.0);
        let rect = IRect::new(
            (cx - len as f64 / 2.0).round() as i32,
            (cy - h as f64 / 2.0).round() as i32,
            len,
            h,
        );
        (rect, (deg.round() as i32).rem_euclid(360) as u16)
    }

    /// The colour controls speak of markers: a marker tool with nothing selected, or a selection
    /// of markers only (their own palette, ZK-171).
    fn marker_colours(&self) -> bool {
        let sel = self.selection();
        if sel.is_empty() {
            return self.tool == tool::MARKER;
        }
        let Some(s) = self.s.as_ref() else {
            return false;
        };
        sel.iter()
            .all(|id| s.ed.doc.get(*id).is_some_and(|o| o.kind() == Kind::Mark))
    }

    fn new_object(&self, t: usize, a: (i32, i32), b: (i32, i32)) -> Object {
        if t == tool::MARKER {
            let (rect, rot) = self.marker_bar(a, b, false);
            let mut o = Object::new(rect, Data::Mark).with_style(self.style_for(t));
            o.rot = rot;
            return o;
        }
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
        // A press on the canvas takes the keys back from the timeline (ZK-181).
        if kind == 0
            && let Some(v) = self.s.as_mut().and_then(|s| s.vid.as_mut())
        {
            v.focus = false;
        }
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
        self.update_cursor(ui, out, p, ctrl);
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
        if button == 2 && self.over.is_none() {
            self.drag = Some(Drag::Pan {
                start_out: out,
                orig: self.view.origin,
            });
            return;
        }
        if button != 0 {
            return;
        }
        // With the text panel open the canvas reads text: a frame = only that part, a click =
        // copy the line under it (ZK-184).
        if self.text.is_some() {
            self.drag = Some(Drag::TextRegion {
                start: p,
                start_out: out,
            });
            return;
        }
        // The eyedropper takes the colour under the click, nothing else happens (ZK-160).
        if let Some(key) = self.eyedrop.clone() {
            self.eyedrop_cancel(ui);
            if let Some(c) = self.colour_at_canvas(out) {
                let v = ((c.r as i32) << 16) | ((c.g as i32) << 8) | c.b as i32;
                self.set_prop(ui, &format!("{key}-rgb"), v);
            }
            return;
        }
        if self.editing.is_some() && self.text_press(ui, out, shift) {
            return;
        }
        self.finish_text(ui);
        if self.tool == tool::CROP {
            self.crop_down(out, p);
            return;
        }
        // Over the screen the frame's corners and edges are always there to drag (the crop).
        if self.over.is_some()
            && !ctrl
            && let Some(c) = self.s.as_ref().map(|s| s.ed.doc.frame())
            && let Some(h) = self.frame_handle_at(c, out)
        {
            self.crop = Some(c);
            self.drag = Some(Drag::CropEdit {
                handle: h,
                orig: c,
                start: p,
            });
            return;
        }
        let px = self.view.scale / self.dpr;
        let sel = self.selection();

        // The rotation handle comes first, before the resize handles and the tool (ZK-164).
        if let Some((grip, center, _)) = self.rotation_grip() {
            let pd = self.view.to_doc(out.x, out.y);
            let k = px.max(1e-6);
            if ((grip.0 - pd.x).powi(2) + (grip.1 - pd.y).powi(2)).sqrt() <= 7.0 / k {
                let orig = {
                    let doc = &self.s.as_ref().unwrap().ed.doc;
                    sel.iter()
                        .filter_map(|id| doc.get(*id))
                        .map(|o| (o.id, o.bounds().center(), o.rot, o.kind().can_rotate()))
                        .collect()
                };
                self.drag = Some(Drag::Rotate {
                    center,
                    ang0: hit::angle_from_up(center, (pd.x, pd.y)),
                    orig,
                    merge: self.merge_key(),
                });
                return;
            }
        }

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
                    // Shift / Ctrl on a mark: a click toggles it in the selection, a drag moves
                    // the selection with it (owner, 29.09: Ctrl as a temporary Select must move
                    // marks too). Which one it is shows once the pointer moves.
                    self.drag = Some(Drag::Toggle {
                        id,
                        was_selected: sel.contains(&id),
                        start: p,
                        start_out: out,
                    });
                }
                Some(id) => {
                    // Alt (⌥)+click: this one member alone, the group stays (ZK-169).
                    let alt = self.test_alt
                        || crate::overlay::held_modifiers().is_some_and(|(_, alt)| alt);
                    if alt {
                        if sel != [id] {
                            self.apply(
                                ui,
                                Command::Select {
                                    ids: vec![id],
                                    add: false,
                                },
                            );
                        }
                    } else if !sel.contains(&id) {
                        // A member of a group brings the whole group (ZK-159).
                        let ids = self.with_groups(vec![id]);
                        self.apply(ui, Command::Select { ids, add: false });
                    }
                    let merge = self.merge_key();
                    self.drag = Some(Drag::Move { last: p, merge });
                }
                None => {
                    // Empty space: Ctrl is just "Select for the moment", so a Ctrl+click clears
                    // the selection like a plain click (owner, 28.09); only Shift adds a band
                    // to what is selected.
                    if !shift {
                        self.apply(ui, Command::ClearSelection);
                    }
                    self.drag = Some(Drag::Marquee {
                        start: p,
                        add: shift,
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
                    // The group keeps its start number; a new group starts at 1.
                    let start = self
                        .s
                        .as_ref()
                        .and_then(|s| {
                            s.ed.doc.objects.iter().find_map(|o| match o.data {
                                Data::Counter { group, start, .. }
                                    if group == self.counter_group =>
                                {
                                    Some(start)
                                }
                                _ => None,
                            })
                        })
                        .unwrap_or(1);
                    Data::Counter {
                        seq: 0,
                        group: self.counter_group,
                        start,
                        shape: self.counter_shape,
                    }
                } else {
                    Data::Stamp { id: self.stamp_id }
                };
                let mut st = st;
                if self.tool == tool::COUNTER {
                    st.color2 = self.digit;
                }
                // A pin is taller than wide: its point sits under the pointer's click.
                let (w, h) =
                    if self.tool == tool::COUNTER && self.counter_shape == CounterShape::Pin {
                        (d, d * 13 / 10)
                    } else {
                        (d, d)
                    };
                let obj =
                    Object::new(IRect::new(p.0 - w / 2, p.1 - h / 2, w, h), data).with_style(st);
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
            Drag::Toggle {
                id,
                was_selected,
                start,
                start_out,
            } => {
                if (out - start_out).hypot() < 3.0 * self.dpr {
                    return;
                }
                if !was_selected {
                    let ids = self.with_groups(vec![id]);
                    self.apply(ui, Command::Select { ids, add: true });
                }
                let merge = self.merge_key();
                self.drag = Some(Drag::Move { last: start, merge });
                self.pointer_move(ui, out, p, shift);
            }
            Drag::TextRegion { start, start_out } => {
                if (out - start_out).hypot() >= 3.0 * self.dpr {
                    self.marquee = Some(
                        IRect::new(start.0, start.1, p.0 - start.0, p.1 - start.1).normalized(),
                    );
                    self.dirty = true;
                }
            }
            Drag::TextSelect { anchor } => {
                if let Some(o) = self.edited_object().cloned() {
                    let pd = self.view.to_doc(out.x, out.y);
                    if let Some(i) = self.renderer.text_hit(&o, pd.x, pd.y) {
                        ui.invoke_edit_select(anchor as i32, i as i32);
                        self.text_cursor(ui, i as i32, anchor as i32);
                    }
                }
            }
            Drag::CropNew { start, .. } => {
                self.crop = Some(self.crop_from(start, p, shift));
                self.dirty = true;
            }
            Drag::CropEdit {
                handle,
                orig,
                start,
            } => {
                self.crop = Some(self.crop_edit(handle, orig, (p.0 - start.0, p.1 - start.1)));
                self.dirty = true;
            }
            Drag::Pending { start, start_out } => {
                if (out - start_out).hypot() < 3.0 * self.dpr {
                    return;
                }
                let merge = self.merge_key();
                if self.tool == tool::PEN {
                    let points = vec![start, p];
                    let mut obj =
                        Object::new(bounds_of_points(&points), self.pen_data(points.clone()))
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
            Drag::Create { id, start, merge } if self.tool == tool::MARKER => {
                let (rect, rot) = self.marker_bar(start, p, shift);
                self.apply(
                    ui,
                    Command::UpdateObjects {
                        ids: vec![id],
                        patch: ObjectPatch {
                            rect: Some(rect),
                            rot: Some(rot),
                            ..Default::default()
                        },
                        merge: Some(merge),
                    },
                );
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
                    data: Some(self.pen_data(points.clone())),
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
            Drag::Rotate {
                center,
                ang0,
                orig,
                merge,
            } => {
                let pd = self.view.to_doc(out.x, out.y);
                let mut delta = hit::angle_from_up(center, (pd.x, pd.y)) - ang0;
                let single = orig.len() == 1;
                // Shift: steps of 45° — of the mark's own angle alone, of the turn for several
                // (owner, 30.09: 15° steps were too fine).
                if shift && !single {
                    delta = (delta / 45.0).round() * 45.0;
                }
                let norm = |a: f64| (a.round() as i32).rem_euclid(360) as u16;
                for (id, c0, rot0, turns) in orig {
                    if !single {
                        let (tx, ty) = hit::turn(c0, center, delta);
                        let cur = self
                            .s
                            .as_ref()
                            .and_then(|s| s.ed.doc.get(id))
                            .map(|o| o.bounds().center())
                            .unwrap_or(c0);
                        let (dx, dy) = ((tx - cur.0).round() as i32, (ty - cur.1).round() as i32);
                        if dx != 0 || dy != 0 {
                            self.apply(
                                ui,
                                Command::MoveObjects {
                                    ids: vec![id],
                                    dx,
                                    dy,
                                    merge: Some(merge.clone()),
                                },
                            );
                        }
                    }
                    if turns {
                        let mut a = rot0 as f64 + delta;
                        if shift && single {
                            a = (a / 45.0).round() * 45.0;
                        }
                        self.apply(
                            ui,
                            Command::UpdateObjects {
                                ids: vec![id],
                                patch: ObjectPatch {
                                    rot: Some(norm(a)),
                                    ..Default::default()
                                },
                                merge: Some(merge.clone()),
                            },
                        );
                    }
                }
                // The angle beside the pointer while turning.
                let shown = if single {
                    self.s
                        .as_ref()
                        .and_then(|s| s.ed.doc.get(orig_first_id(&self.drag)))
                        .map_or(0, |o| o.rot as i32)
                } else {
                    (delta.round() as i32).rem_euclid(360)
                };
                let lp = ((out.x / self.dpr) as f32, (out.y / self.dpr) as f32);
                ui.set_rot_hint(format!("{shown}°").into());
                ui.set_rot_hint_x(lp.0);
                ui.set_rot_hint_y(lp.1);
                self.dirty = true;
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
                // Shift — proportional, Alt (⌥) — about the centre (ZK-167). Alt is read from the
                // system: the canvas pointer event does not carry it.
                let alt =
                    self.test_alt || crate::overlay::held_modifiers().is_some_and(|(_, alt)| alt);
                self.apply(
                    ui,
                    Command::ResizeObject {
                        id,
                        handle,
                        orig,
                        dx,
                        dy,
                        keep_ratio: shift,
                        from_centre: alt,
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
                let ids = self.with_groups(ids);
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

    fn pointer_up(&mut self, ui: &AppWindow) {
        if self.over.is_some() && matches!(self.drag, Some(Drag::CropEdit { .. })) {
            self.drag = None;
            self.commit_crop(ui);
        }
        if matches!(self.drag, Some(Drag::Create { .. } | Drag::Pen { .. }))
            && !self.prefs().editor.keep_tool
        {
            self.drag = None;
            self.set_tool(ui, tool::SELECT);
        }
        if matches!(self.drag, Some(Drag::Rotate { .. })) {
            ui.set_rot_hint(SharedString::new());
        }
        if let Some(Drag::TextRegion { start, .. }) = self.drag {
            self.drag = None;
            match self.marquee.take() {
                Some(r) if r.w >= 4 && r.h >= 4 => self.text_read(ui, Some(r)),
                _ => {
                    // A click: the line under it goes to the clipboard.
                    let line = self.text.as_ref().and_then(|t| {
                        t.lines
                            .iter()
                            .find(|l| {
                                let r = l.rect.normalized();
                                start.0 >= r.x - 2
                                    && start.0 <= r.right() + 2
                                    && start.1 >= r.y - 2
                                    && start.1 <= r.bottom() + 2
                            })
                            .map(|l| l.text.clone())
                    });
                    if let Some(line) = line {
                        let msg = if crate::text::copy(&line) {
                            self.tr.tr("clipboard-copied")
                        } else {
                            self.tr.tr("clipboard-error")
                        };
                        self.toast(ui, msg);
                    }
                }
            }
            self.dirty = true;
        }
        if let Some(Drag::Toggle {
            id, was_selected, ..
        }) = self.drag
        {
            self.drag = None;
            self.toggle_selected(ui, id, was_selected);
        }
        // A click without a drag on the picture keeps the frame that was there.
        if let Some(Drag::CropNew { prev, .. }) = self.drag
            && self.crop.is_none_or(|c| c.w < 2 || c.h < 2)
        {
            self.crop = Some(prev);
        }
        self.drag = None;
        self.marquee = None;
        self.dirty = true;
    }

    /// The rotation handle of the selection (ZK-164): (the handle, the centre it turns about, the
    /// foot of its stem), in screenshot coordinates. One mark that can turn: above its top edge,
    /// turned with it; several marks: above the middle of their box (LH `EdManyRotHandle`).
    fn rotation_grip(&self) -> Option<(Pt, Pt, Pt)> {
        let s = self.s.as_ref()?;
        if self.editing.is_some() || self.tool == tool::CROP {
            return None;
        }
        let px = (self.view.scale / self.dpr).max(1e-6);
        let doc = &s.ed.doc;
        let sel: Vec<&Object> =
            s.ed.selection()
                .iter()
                .filter_map(|id| doc.get(*id))
                .collect();
        match sel.as_slice() {
            [] => None,
            [o] => {
                let g = hit::rotation_handle(o, px)?;
                let b = o.bounds();
                let c = b.center();
                let foot = hit::turn(
                    (c.0, b.y as f64),
                    c,
                    if hit::turned(o) { o.rot as f64 } else { 0.0 },
                );
                Some((g, c, foot))
            }
            many => {
                let mut b = many[0].bounds();
                for o in &many[1..] {
                    b = b.union(o.bounds());
                }
                let c = b.center();
                let foot = (c.0, b.y as f64);
                Some(((c.0, b.y as f64 - hit::ROTATION_STEM / px), c, foot))
            }
        }
    }

    /// For the self-test: the rotation handle and the centre it turns about, in logical canvas
    /// pixels.
    pub fn rotation_grip_probe(&self) -> Option<((f32, f32), (f32, f32))> {
        let (g, c, _) = self.rotation_grip()?;
        Some((self.doc_to_logical(g.0, g.1), self.doc_to_logical(c.0, c.1)))
    }

    /// The marks with every group any of them is in (ZK-159): on the canvas a group is picked,
    /// toggled and caught by the marquee as one; the layers list can still pick a single member.
    /// Order kept, no repeats.
    fn with_groups(&self, ids: Vec<ObjectId>) -> Vec<ObjectId> {
        let Some(s) = self.s.as_ref() else {
            return ids;
        };
        let doc = &s.ed.doc;
        let mut out: Vec<ObjectId> = Vec::with_capacity(ids.len());
        for id in ids {
            let g = doc.get(id).map_or(0, |o| o.group);
            let more = if g == 0 {
                vec![]
            } else {
                self.group_members(g)
            };
            for m in std::iter::once(id).chain(more) {
                if !out.contains(&m) {
                    out.push(m);
                }
            }
        }
        out
    }

    /// Shift / Ctrl click on a mark: a selected one leaves the selection, another one joins it
    /// (with its group, ZK-159).
    fn toggle_selected(&mut self, ui: &AppWindow, id: ObjectId, was_selected: bool) {
        let ids = self.with_groups(vec![id]);
        if was_selected {
            let rest: Vec<ObjectId> = self
                .selection()
                .into_iter()
                .filter(|s| !ids.contains(s))
                .collect();
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
            self.apply(ui, Command::Select { ids, add: true });
        }
    }

    // ------------------------------------------------------------------ cursor (ZK-47)

    /// The canvas cursor for what is under the pointer and what a press would do there
    /// (owner, 29.09): the tailless arrow of the Select icon, a four-way arrow over a mark that
    /// can be moved, the resize arrow of each handle, a crosshair for drawing and cropping, the
    /// I-beam for text, a closed hand while panning. Codes are `canvas-cursor` in app.slint.
    fn update_cursor(&mut self, ui: &AppWindow, out: Point, p: (i32, i32), ctrl: bool) {
        let c = self.cursor_at(out, p, ctrl);
        if ui.get_canvas_cursor() != c {
            ui.set_canvas_cursor(c);
        }
    }

    /// For the self-test: where a document point is on the canvas, in logical pixels.
    pub fn doc_to_logical(&self, x: f64, y: f64) -> (f32, f32) {
        let o = self.view.to_out(Point::new(x, y));
        ((o.x / self.dpr) as f32, (o.y / self.dpr) as f32)
    }

    /// For the self-test: the cursor over a document point.
    pub fn cursor_probe(&self, x: f64, y: f64, ctrl: bool) -> i32 {
        let out = self.view.to_out(Point::new(x, y));
        self.cursor_at(out, (x.round() as i32, y.round() as i32), ctrl)
    }

    fn cursor_at(&self, out: Point, p: (i32, i32), ctrl: bool) -> i32 {
        const ARROW: i32 = 0;
        const CROSS: i32 = 1;
        const TEXT: i32 = 2;
        const MOVE: i32 = 3;
        const GRABBING: i32 = 9;
        const ROTATE: i32 = 10;
        // Resize arrows per handle of a box, clockwise from the top-left corner.
        const BOX: [i32; 8] = [4, 6, 5, 7, 4, 6, 5, 7];
        let Some(s) = self.s.as_ref() else {
            return ARROW;
        };
        if self.eyedrop.is_some() || self.text.is_some() {
            return CROSS;
        }
        let doc = &s.ed.doc;
        if let Some(o) = self.edited_object() {
            let pd = self.view.to_doc(out.x, out.y);
            let b = o.bounds();
            if pd.x >= b.x as f64
                && pd.x <= b.right() as f64
                && pd.y >= b.y as f64
                && pd.y <= b.bottom() as f64
            {
                return TEXT;
            }
        }
        // Turned marks show the resize arrow turned with them (to the nearest 45°).
        let turn8 = |o: &Object| {
            if hit::turned(o) {
                ((o.rot as f64 / 45.0).round() as usize) % 8
            } else {
                0
            }
        };
        match &self.drag {
            Some(Drag::Rotate { .. }) => return ROTATE,
            Some(Drag::TextSelect { .. }) => return TEXT,
            Some(Drag::Pan { .. }) => return GRABBING,
            Some(Drag::Move { .. } | Drag::Toggle { .. }) => return MOVE,
            Some(Drag::Resize { id, handle, .. }) => {
                return match doc.get(*id) {
                    Some(o) if o.kind() == Kind::Line => CROSS,
                    Some(o) if matches!(o.kind(), Kind::Text | Kind::Mark) => {
                        BOX[(3 + turn8(o)) % 8]
                    }
                    Some(o) if hit::scales_only(o.kind()) => {
                        BOX[([0, 2, 4, 6][*handle % 4] + turn8(o)) % 8]
                    }
                    Some(o) => BOX[(*handle + turn8(o)) % 8],
                    None => BOX[*handle % 8],
                };
            }
            Some(Drag::CropEdit { handle, .. }) => {
                return if *handle < 8 { BOX[*handle] } else { MOVE };
            }
            Some(Drag::Marquee { .. }) => return ARROW,
            Some(_) => return CROSS,
            None => {}
        }
        if self.over.is_some()
            && !ctrl
            && let Some(h) = self.frame_handle_at(doc.frame(), out)
        {
            return BOX[h];
        }
        if self.tool == tool::CROP {
            let c = self.crop.unwrap_or_else(|| doc.frame());
            let reach = 12.0 * self.dpr;
            if let Some(h) = self
                .crop_handles(c)
                .iter()
                .position(|h| (*h - out).hypot() <= reach)
            {
                return BOX[h];
            }
            let inside = p.0 > c.x && p.0 < c.right() && p.1 > c.y && p.1 < c.bottom();
            return if inside { MOVE } else { CROSS };
        }
        let px = self.view.scale / self.dpr;
        let pd = self.view.to_doc(out.x, out.y);
        let sel = s.ed.selection();
        if let Some((grip, _, _)) = self.rotation_grip()
            && ((grip.0 - pd.x).powi(2) + (grip.1 - pd.y).powi(2)).sqrt() <= 7.0 / px.max(1e-6)
        {
            return ROTATE;
        }
        if sel.len() == 1
            && let Some(o) = doc.get(sel[0])
            && let Some(h) = hit::hit_handle(o, (pd.x, pd.y), px)
        {
            return match o.kind() {
                Kind::Line => CROSS,
                Kind::Text | Kind::Mark => BOX[(3 + turn8(o)) % 8],
                k if hit::scales_only(k) => BOX[([0, 2, 4, 6][h % 4] + turn8(o)) % 8],
                _ => BOX[(h + turn8(o)) % 8],
            };
        }
        let tool = if ctrl { tool::SELECT } else { self.tool };
        match tool {
            tool::SELECT => {
                if hit::pick(doc, (p.0 as f64, p.1 as f64), px).is_some() {
                    MOVE
                } else {
                    ARROW
                }
            }
            tool::TEXT => TEXT,
            _ => CROSS,
        }
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
        let existing = existing.and_then(|id| {
            let o = self.s.as_ref()?.ed.doc.get(id)?;
            match &o.data {
                Data::Text { text, .. } => Some((id, (o.rect.x, o.rect.y), text.clone())),
                _ => None,
            }
        });
        let (id, at, text) = match existing {
            Some((id, at, t)) => (Some(id), at, t),
            None => (None, p, String::new()),
        };
        if let Some(id) = id
            && !self.selection().contains(&id)
        {
            self.apply(
                ui,
                Command::Select {
                    ids: vec![id],
                    add: false,
                },
            );
        }
        let merge = self.merge_key();
        let len = text.len();
        self.editing = Some(TextEdit {
            id,
            at,
            merge,
            changed: false,
            cursor: len,
            anchor: if id.is_some() { 0 } else { len },
        });
        // The hidden input sits where the text is, so an input method's window appears there.
        let o = self.view.to_out(Point::new(at.0 as f64, at.1 as f64));
        ui.set_edit_x((o.x / self.dpr) as f32);
        ui.set_edit_y((o.y / self.dpr) as f32);
        ui.set_edit_text(text.into());
        ui.set_editing(true);
        // An existing text opens all selected (typing replaces it), a new one empty.
        ui.invoke_edit_select(if id.is_some() { 0 } else { len as i32 }, len as i32);
        self.blink_restart();
        self.dirty = true;
        self.sync(ui);
        ui.window().request_redraw();
    }

    /// The hidden input changed: the text goes into the document at once.
    pub fn text_edited(&mut self, ui: &AppWindow, cursor: i32, anchor: i32) {
        let text = ui.get_edit_text().to_string();
        let Some(ed) = self.editing.as_mut() else {
            return;
        };
        ed.cursor = cursor.max(0) as usize;
        ed.anchor = anchor.max(0) as usize;
        let (id, at, merge) = (ed.id, ed.at, ed.merge.clone());
        match id {
            None if text.is_empty() => {}
            None => {
                let size = self.text_size();
                let obj = Object::new(
                    IRect::new(at.0, at.1, 1, 1),
                    Data::Text {
                        text,
                        size,
                        bold: self.bold,
                        italic: self.italic,
                        align: self.align,
                        box_w: 0,
                    },
                )
                .with_style(self.style_for(tool::TEXT));
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
                    if let Some(ed) = self.editing.as_mut() {
                        ed.id = Some(id);
                        ed.changed = true;
                    }
                }
            }
            Some(id) => {
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
                if let Some(ed) = self.editing.as_mut() {
                    ed.changed = true;
                }
            }
        }
        self.blink_restart();
        self.dirty = true;
        self.sync(ui);
        ui.window().request_redraw();
    }

    /// The caret moved or the selection changed in the hidden input.
    pub fn text_cursor(&mut self, ui: &AppWindow, cursor: i32, anchor: i32) {
        if let Some(ed) = self.editing.as_mut() {
            ed.cursor = cursor.max(0) as usize;
            ed.anchor = anchor.max(0) as usize;
            self.blink_restart();
            self.dirty = true;
            ui.window().request_redraw();
        }
    }

    fn blink_restart(&mut self) {
        self.caret_on = true;
        self.caret_timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(530),
            {
                let me = self.me();
                move || {
                    me.with(|a, ui| {
                        if a.editing.is_some() {
                            a.caret_on = !a.caret_on;
                            a.dirty = true;
                            ui.window().request_redraw();
                        } else {
                            a.caret_timer.stop();
                        }
                    })
                }
            },
        );
    }

    /// Enter / a click elsewhere / another tool: the text stays; an emptied text goes away.
    pub fn commit_text(&mut self, ui: &AppWindow, text: &str) {
        let Some(ed) = self.editing.take() else {
            return;
        };
        self.caret_timer.stop();
        ui.set_editing(false);
        let trimmed = text.trim_end().to_string();
        if let Some(id) = ed.id {
            if trimmed.trim().is_empty() {
                self.apply(ui, Command::DeleteObjects { ids: vec![id] });
            } else if trimmed != text {
                // Trailing spaces and newlines do not stay.
                self.apply(
                    ui,
                    Command::UpdateObjects {
                        ids: vec![id],
                        patch: ObjectPatch {
                            text: Some(trimmed),
                            ..Default::default()
                        },
                        merge: Some(ed.merge.clone()),
                    },
                );
                self.fit_text(ui, id, Some(ed.merge));
            }
        }
        ui.invoke_focus_canvas();
        self.dirty = true;
        self.sync(ui);
        ui.window().request_redraw();
    }

    /// Esc: the text as it was before typing started.
    pub fn cancel_text(&mut self, ui: &AppWindow) {
        let Some(ed) = self.editing.take() else {
            return;
        };
        self.caret_timer.stop();
        ui.set_editing(false);
        if ed.changed {
            self.apply(ui, Command::Undo);
        }
        ui.invoke_focus_canvas();
        self.dirty = true;
        self.sync(ui);
        ui.window().request_redraw();
    }

    /// Commits a text being typed (clicking elsewhere, saving, undo).
    fn finish_text(&mut self, ui: &AppWindow) {
        if self.editing.is_some() {
            let t = ui.get_edit_text().to_string();
            self.commit_text(ui, &t);
        }
    }

    /// The mark being typed into, for the caret and clicks.
    fn edited_object(&self) -> Option<&Object> {
        let id = self.editing.as_ref()?.id?;
        self.s.as_ref()?.ed.doc.get(id)
    }

    /// A press inside the text being typed places the caret there (Shift extends); returns
    /// false when the press is elsewhere.
    fn text_press(&mut self, ui: &AppWindow, out: Point, shift: bool) -> bool {
        let Some(o) = self.edited_object().cloned() else {
            return false;
        };
        let pd = self.view.to_doc(out.x, out.y);
        let b = o.bounds();
        let slack = 6.0 / self.view.scale.max(1e-6);
        if pd.x < b.x as f64 - slack
            || pd.x > b.right() as f64 + slack
            || pd.y < b.y as f64 - slack
            || pd.y > b.bottom() as f64 + slack
        {
            return false;
        }
        let Some(i) = self.renderer.text_hit(&o, pd.x, pd.y) else {
            return false;
        };
        let anchor = if shift {
            self.editing.as_ref().map_or(i, |e| e.anchor)
        } else {
            i
        };
        ui.invoke_edit_select(anchor as i32, i as i32);
        self.text_cursor(ui, i as i32, anchor as i32);
        self.drag = Some(Drag::TextSelect { anchor });
        true
    }

    /// Double click with Select on a text mark: type into it.
    pub fn canvas_double(&mut self, ui: &AppWindow, x: f32, y: f32) {
        let (_, p) = self.to_doc(x, y);
        let px = self.view.scale / self.dpr;
        let hit = self.s.as_ref().and_then(|s| {
            let doc = &s.ed.doc;
            hit::pick(doc, (p.0 as f64, p.1 as f64), px)
                .map(|i| &doc.objects[i])
                .filter(|o| o.kind() == Kind::Text)
                .map(|o| o.id)
        });
        if let Some(id) = hit
            && self.editing.is_none()
        {
            self.drag = None;
            self.start_text(ui, p, Some(id));
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
        self.eyedrop_cancel(ui);
        let t = t.min(tool::CROP);
        let was_crop = self.tool == tool::CROP;
        if was_crop && t != tool::CROP {
            self.commit_crop(ui);
        }
        self.tool = t;
        ui.set_tool(self.tool as i32);
        // Choosing a drawing tool lets go of the selection, so the inspector shows the tool's
        // own properties (owner, 30.09: a mark just drawn stayed selected under a new tool).
        if t != tool::SELECT && !self.selection().is_empty() {
            self.apply(ui, Command::ClearSelection);
        }
        let (out, p) = (self.last_out, {
            let d = self.view.to_doc(self.last_out.x, self.last_out.y);
            (d.x.round() as i32, d.y.round() as i32)
        });
        self.update_cursor(ui, out, p, false);
        if t == tool::CROP && !was_crop && self.s.is_some() {
            // The whole picture comes into view, the frame on it can be pulled anywhere.
            self.crop = self.s.as_ref().map(|s| s.ed.doc.frame());
            if !self.selection().is_empty() {
                self.apply(ui, Command::ClearSelection);
            }
            ui.set_insp_tab(2);
            let k = self.fit_scale();
            self.animate_zoom(k, None);
        } else if was_crop && t != tool::CROP {
            let k = self.fit_scale();
            self.animate_zoom(k, None);
        }
        self.dirty = true;
        self.sync(ui);
        ui.window().request_redraw();
    }

    /// Keys of the canvas. Letters are matched on both the Latin and the Ukrainian layout, so a
    /// tool key works whatever layout is on (the physical-key API comes with ZK-35's hotkeys).
    pub fn key(&mut self, ui: &AppWindow, text: &str, ctrl: bool, shift: bool) -> KeyAction {
        const PAIRS: [(char, char); 21] = [
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
            ('x', 'ч'),
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
                // Over the screen: Ctrl+S = to the library; no file dialogs above the frame.
                Some('s') if self.over.is_some() => {
                    return if shift {
                        KeyAction::None
                    } else {
                        KeyAction::Save
                    };
                }
                Some('o') if self.over.is_some() => return KeyAction::None,
                Some('s') if shift => return KeyAction::Export,
                // The export sheet (ZK-187); with Shift — the last export again, no sheet.
                Some('e') if shift && self.over.is_none() => return KeyAction::ExportRepeat,
                Some('e') if self.over.is_none() => return KeyAction::Export,
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
                // Video (ZK-181): loop, the frame to the clipboard.
                Some('l') if self.is_video() => self.vid_transport(ui, "loop"),
                Some('0') => self.zoom_fit(ui),
                Some('1') => self.zoom_100(ui),
                // Zoom in / out by half a stop about the canvas centre ("=" is "+" unshifted).
                Some('=' | '+') => self.zoom_step(ui, 1),
                Some('-' | '_') => self.zoom_step(ui, -1),
                _ => return KeyAction::None,
            }
            self.sync(ui);
            ui.window().request_redraw();
            return KeyAction::None;
        }
        // Video (ZK-181): the transport and the timeline's keys.
        if let Some(a) = self.video_key(ui, text, latin, shift) {
            return a;
        }
        // I: a picture from a file (a dialog — so not over the screen, ZK-163).
        // X: the text on the picture (ZK-184).
        if latin == Some('x') {
            if self.text.is_some() {
                self.text_close(ui);
            } else {
                self.text_open(ui);
            }
            return KeyAction::None;
        }
        if latin == Some('i') {
            return if self.over.is_some() {
                KeyAction::None
            } else {
                KeyAction::InsertImage
            };
        }
        let tools = ['v', 'r', 'e', 'l', 'p', 't', 'b', 'h', 'n', 's', 'c'];
        // Over the screen the frame itself is the crop: no Crop tool there.
        if let Some(i) = latin
            .and_then(|c| tools.iter().position(|t| *t == c))
            .filter(|i| !(self.over.is_some() && *i == tool::CROP))
        {
            self.set_tool(ui, i);
            return KeyAction::None;
        }
        // [ and ] — thinner / thicker, as in LH (with Ctrl they change the order).
        if matches!(latin, Some('[' | ']')) {
            let t = self.thick as i32 + if latin == Some(']') { 1 } else { -1 };
            if (0..THICK.len() as i32).contains(&t) {
                self.set_prop(ui, "thick", t);
            }
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
                "\n" | "\r" if self.over.is_some() => return KeyAction::Copy,
                "\n" | "\r" if self.tool == tool::CROP => {
                    self.set_tool(ui, tool::SELECT);
                }
                // Enter repeats how the picture last left the editor (LH): copy by default.
                "\n" | "\r" => {
                    return if self.last_export {
                        KeyAction::ExportRepeat
                    } else {
                        KeyAction::Copy
                    };
                }
                // The text panel goes first (ZK-184), then the eyedropper (ZK-160).
                "\u{1b}" if self.text.is_some() => {
                    self.text_close(ui);
                }
                "\u{1b}" if self.eyedrop.is_some() => {
                    self.eyedrop_cancel(ui);
                }
                "\u{1b}" if self.tool == tool::CROP => {
                    self.drag = None;
                    self.crop = None;
                    self.set_tool(ui, tool::SELECT);
                }
                // Esc takes off one layer at a time (LH): a drag, the selection, the tool —
                // and only then leaves the document for the library.
                "\u{1b}" => {
                    if self.drag.take().is_none() {
                        if !self.selection().is_empty() {
                            self.apply(ui, Command::ClearSelection);
                        } else if self.tool != tool::SELECT && self.over.is_none() {
                            self.set_tool(ui, tool::SELECT);
                        } else {
                            return KeyAction::Back;
                        }
                    }
                    self.marquee = None;
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
        self.add_image_mark(ui, r);
    }

    /// An image file dropped on the open document (ZK-51): a mark, like Ctrl+V. A .znimok or
    /// an unreadable file returns false (the caller opens it instead).
    pub fn drop_image_mark(&mut self, ui: &AppWindow, path: &Path) -> bool {
        if self.s.is_none() || ui.get_page() != 1 {
            return false;
        }
        let head = std::fs::read(path)
            .ok()
            .map(|d| znimok_format::is_znimok(&d));
        if head != Some(false) {
            return false;
        }
        match io::load_image(path) {
            Ok(r) => {
                self.add_image_mark(ui, r);
                self.sync(ui);
                ui.window().request_redraw();
                true
            }
            Err(_) => false,
        }
    }

    /// The Image button / I (ZK-163): a picture file becomes a mark, as a dropped one does; one
    /// Znimok can't read says so. Returns whether a mark was added.
    pub fn insert_image_file(&mut self, ui: &AppWindow, path: &Path) -> bool {
        if self.s.is_none() {
            return false;
        }
        if self.drop_image_mark(ui, path) {
            return true;
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let msg = self
            .tr
            .tr_args("open-error-not-image", &args(&[("name", name)]));
        self.toast(ui, msg);
        false
    }

    /// A picture as a mark in the middle of the frame, at most 80 % of it.
    fn add_image_mark(&mut self, ui: &AppWindow, r: Raster) {
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

    /// The colour picker's "recent" row (ZK-160): newest first, eight at most, kept in the
    /// settings so it outlives the session.
    fn remember_colour(&mut self, ui: &AppWindow, c: Rgb) {
        let hex = hex_of(c);
        if let Some(store) = self.store.as_ref() {
            let _ = store.update(|p| {
                let r = &mut p.editor.recent_colours;
                r.retain(|h| !h.eq_ignore_ascii_case(&hex));
                r.insert(0, hex.clone());
                r.truncate(8);
            });
        }
        self.show_recent_colours(ui);
    }

    /// Hands the recent colours to the picker (ZK-160).
    pub fn show_recent_colours(&self, ui: &AppWindow) {
        let v: Vec<slint::Color> = self
            .recent_colours()
            .into_iter()
            .map(|c| slint::Color::from_rgb_u8(c.r, c.g, c.b))
            .collect();
        ui.global::<crate::ColourPick>()
            .set_recent(std::rc::Rc::new(slint::VecModel::from(v)).into());
    }

    /// The recent colours for the picker.
    pub fn recent_colours(&self) -> Vec<Rgb> {
        self.prefs()
            .editor
            .recent_colours
            .iter()
            .filter_map(|h| parse_hex(h))
            .collect()
    }

    /// A hex code typed into the picker: applied like a picked colour; `false` if it is not one.
    pub fn colour_hex(&mut self, ui: &AppWindow, key: &str, text: &str) -> bool {
        let Some(c) = parse_hex(text) else {
            return false;
        };
        let v = ((c.r as i32) << 16) | ((c.g as i32) << 8) | c.b as i32;
        self.set_prop(ui, &format!("{key}-rgb"), v);
        true
    }

    /// «Text from the screenshot» (ZK-184): opens the panel and reads the whole picture (as it
    /// looks, marks included) on a worker thread, on the device.
    pub fn text_open(&mut self, ui: &AppWindow) {
        if self.s.is_none() {
            return;
        }
        self.text = Some(TextFind {
            lines: Vec::new(),
            busy: false,
        });
        ui.set_text_open(true);
        ui.set_text_result(SharedString::new());
        self.text_read(ui, None);
    }

    /// Reads the whole frame, or only `region` (document coordinates).
    fn text_read(&mut self, ui: &AppWindow, region: Option<IRect>) {
        let Some(frame) = self.s.as_ref().map(|s| s.ed.doc.frame()) else {
            return;
        };
        // The picture itself, not the marks drawn on it (they only add noise) — but a Hide stays:
        // what was hidden must not come back as text.
        let Some((w, h, rgba)) = self.s.as_ref().map(|s| {
            let mut doc = s.ed.doc.clone();
            doc.objects.retain(|o| o.kind() == Kind::Hide);
            let mut pix = Pixmap::new(1, 1);
            self.renderer.render(&doc, View::one_to_one(&doc), &mut pix);
            (
                pix.width() as u32,
                pix.height() as u32,
                znimok_render::pixmap_to_rgba(&pix),
            )
        }) else {
            return;
        };
        let origin = (frame.x, frame.y);
        let job = match region {
            Some(r) => match crate::text::crop(w, h, &rgba, origin, r) {
                Some(part) => part,
                None => return,
            },
            None => (w, h, rgba, origin),
        };
        if let Some(t) = self.text.as_mut() {
            t.busy = true;
        }
        ui.set_text_busy(true);
        ui.set_text_status(self.tr.tr("text-busy").into());
        let me = self.me();
        std::thread::spawn(move || {
            let (w, h, rgba, at) = job;
            let reading = crate::text::recognize(w, h, rgba, at);
            let _ = slint::invoke_from_event_loop(move || {
                me.with(|a, ui| a.text_done(ui, reading));
            });
        });
    }

    fn text_done(&mut self, ui: &AppWindow, reading: crate::text::Reading) {
        let Some(t) = self.text.as_mut() else { return };
        t.busy = false;
        ui.set_text_busy(false);
        let status = match reading {
            Ok((lines, missing)) => {
                let mut status = if lines.is_empty() {
                    self.tr.tr("text-none")
                } else {
                    self.tr
                        .tr_args("text-count", &args(&[("n", lines.len().to_string())]))
                };
                if !missing.is_empty() {
                    status = format!(
                        "{status} · {}",
                        self.tr
                            .tr_args("text-missing", &args(&[("langs", missing.join(", "))]))
                    );
                }
                ui.set_text_result(crate::text::joined(&lines).into());
                t.lines = lines;
                status
            }
            Err(e) => {
                t.lines.clear();
                self.tr.tr_args("text-error", &args(&[("error", e)]))
            }
        };
        ui.set_text_status(status.into());
        self.dirty = true;
        ui.window().request_redraw();
    }

    /// Closes the text panel and takes the lines off the canvas.
    pub fn text_close(&mut self, ui: &AppWindow) {
        if self.text.take().is_some() {
            ui.set_text_open(false);
            ui.set_text_busy(false);
            self.marquee = None;
            self.dirty = true;
            ui.window().request_redraw();
        }
    }

    /// «Copy all»: the panel's text as it is now (the user may have corrected it).
    pub fn text_copy_all(&mut self, ui: &AppWindow) {
        let text = ui.get_text_result().to_string();
        let msg = if !text.is_empty() && crate::text::copy(&text) {
            self.tr.tr("clipboard-copied")
        } else {
            self.tr.tr("clipboard-error")
        };
        self.toast(ui, msg);
    }

    /// For the self-test: the lines found, as (text, rect).
    pub fn text_lines(&self) -> Vec<(String, IRect)> {
        self.text
            .as_ref()
            .map(|t| t.lines.iter().map(|l| (l.text.clone(), l.rect)).collect())
            .unwrap_or_default()
    }

    /// For the self-test: whether a reading is running.
    pub fn text_busy(&self) -> bool {
        self.text.as_ref().is_some_and(|t| t.busy)
    }

    /// Arms the eyedropper for a colour control: the next click on the canvas takes the colour
    /// of the picture there (the marks included, as shown); Esc or another tool cancels it.
    pub fn eyedrop(&mut self, ui: &AppWindow, key: &str) {
        self.eyedrop = Some(key.to_string());
        ui.set_canvas_cursor(1);
        let hint = self.tr.tr("colour-eyedropper-hint");
        self.toast(ui, hint);
    }

    fn eyedrop_cancel(&mut self, ui: &AppWindow) -> bool {
        let was = self.eyedrop.take().is_some();
        if was {
            ui.set_canvas_cursor(0);
            // Its hint goes with it.
            ui.set_toast(SharedString::new());
            self.toast_at = None;
        }
        was
    }

    /// The colour of the picture at a canvas point (physical pixels of the canvas), as shown.
    fn colour_at_canvas(&self, out: Point) -> Option<Rgb> {
        let (x, y) = (out.x.floor(), out.y.floor());
        let (w, h) = (self.base.width() as f64, self.base.height() as f64);
        if x < 0.0 || y < 0.0 || x >= w || y >= h {
            return None;
        }
        let px = self.base.sample(x as u16, y as u16);
        if px.a == 0 {
            return None;
        }
        let un = |v: u8| ((v as u32 * 255 + px.a as u32 / 2) / px.a as u32).min(255) as u8;
        Some(Rgb::new(un(px.r), un(px.g), un(px.b)))
    }

    /// For the self-test: the colour the eyedropper would take at a document point.
    pub fn colour_probe(&self, x: f64, y: f64) -> Option<Rgb> {
        self.colour_at_canvas(self.view.to_out(Point::new(x, y)))
    }

    /// A style patch of its own for each selected mark `ok` accepts.
    fn style_each(
        &self,
        ok: impl Fn(&Object) -> bool,
        patch: impl Fn(&Object) -> StylePatch,
    ) -> Vec<(ObjectId, StylePatch)> {
        let Some(s) = self.s.as_ref() else {
            return Vec::new();
        };
        s.ed.selection()
            .iter()
            .filter_map(|id| s.ed.doc.get(*id))
            .filter(|o| ok(o))
            .map(|o| (o.id, patch(o)))
            .collect()
    }

    /// Applies per-mark style patches as one undo step.
    fn apply_each(&mut self, ui: &AppWindow, items: Vec<(ObjectId, StylePatch)>) {
        let merge = (items.len() > 1).then(|| self.merge_key());
        for (id, sp) in items {
            self.apply(
                ui,
                Command::UpdateObjects {
                    ids: vec![id],
                    patch: ObjectPatch {
                        style: Some(sp),
                        ..Default::default()
                    },
                    merge: merge.clone(),
                },
            );
        }
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
        // A colour control gives a palette index, or — the colour picker, names ending in
        // "-rgb" (ZK-160) — 0xRRGGBB; below 0 is "none".
        let marker = self.marker_colours();
        let (name, col) = match name.strip_suffix("-rgb") {
            Some(base) => {
                let c = (v >= 0).then(|| Rgb::new((v >> 16) as u8, (v >> 8) as u8, v as u8));
                if let Some(c) = c {
                    self.remember_colour(ui, c);
                }
                (base, c)
            }
            // The main colour row of markers shows the marker's inks (ZK-171).
            None if name == "color" && marker => (
                name,
                (v >= 0).then(|| MARKER_PALETTE[(v as usize).min(MARKER_PALETTE.len() - 1)]),
            ),
            None => (
                name,
                (v >= 0).then(|| PALETTE[(v as usize).min(PALETTE.len() - 1)]),
            ),
        };
        match name {
            // Markers keep their own colour, apart from the other tools' (ZK-171).
            "color" if marker => {
                self.marker_color = col.unwrap_or(MARKER_PALETTE[0]);
                let c = self.marker_color;
                self.patch_selected(
                    ui,
                    |o| o.kind() == Kind::Mark,
                    style(StylePatch {
                        color: Some(c),
                        ..Default::default()
                    }),
                    None,
                );
            }
            // Fixed marker thicknesses (ZK-171): a selected bar keeps its middle, length and angle.
            "marker-size" => {
                self.marker_size_i = (v.max(0) as usize).min(MARKER_SIZES.len() - 1);
                let h = self.marker_thick(self.marker_size_i);
                let items: Vec<(ObjectId, IRect)> = {
                    let Some(s) = self.s.as_ref() else { return };
                    s.ed.selection()
                        .iter()
                        .filter_map(|id| s.ed.doc.get(*id))
                        .filter(|o| o.kind() == Kind::Mark)
                        .map(|o| {
                            let r = o.rect.normalized();
                            let cy2 = 2 * r.y + r.h;
                            (o.id, IRect::new(r.x, (cy2 - h).div_euclid(2), r.w, h))
                        })
                        .collect()
                };
                let merge = (items.len() > 1).then(|| self.merge_key());
                for (id, rect) in items {
                    self.apply(
                        ui,
                        Command::UpdateObjects {
                            ids: vec![id],
                            patch: ObjectPatch {
                                rect: Some(rect),
                                style: Some(StylePatch {
                                    thick: Some(h),
                                    ..Default::default()
                                }),
                                ..Default::default()
                            },
                            merge: merge.clone(),
                        },
                    );
                }
            }
            "color" => {
                self.color = col.unwrap_or(PALETTE[0]);
                // New shapes without an outline were plates of the fill colour: the fill stays.
                self.no_stroke = false;
                let c = self.color;
                // A plate gets its outline back and keeps its colour as the fill (ZK-161).
                let items = self.style_each(
                    |o| !matches!(o.kind(), Kind::Hide | Kind::Image | Kind::Mark),
                    |o| StylePatch {
                        color: Some(c),
                        no_main: Some(false),
                        color2: is_plate(o).then_some(Some(o.style.color)),
                        ..Default::default()
                    },
                );
                self.apply_each(ui, items);
            }
            "shadow" | "glow" => {
                let e = match v {
                    1 => Effect::Light,
                    2 => Effect::Strong,
                    _ => Effect::None,
                };
                let sp = if name == "shadow" {
                    self.shadow = e;
                    StylePatch {
                        shadow: Some(e),
                        ..Default::default()
                    }
                } else {
                    self.glow = e;
                    StylePatch {
                        glow: Some(e),
                        ..Default::default()
                    }
                };
                self.patch_selected(ui, |o| o.kind().fx_allowed(), style(sp), None);
            }
            "counter-shape" => {
                self.counter_shape = match v {
                    1 => CounterShape::RoundedBox,
                    2 => CounterShape::Pin,
                    _ => CounterShape::Circle,
                };
                let shape = self.counter_shape;
                self.patch_data(ui, Kind::Counter, move |d| {
                    if let Data::Counter { shape: s, .. } = d {
                        *s = shape;
                    }
                });
            }
            "digit" => {
                self.digit = col;
                let c = self.digit;
                self.patch_selected(
                    ui,
                    |o| o.kind() == Kind::Counter,
                    style(StylePatch {
                        color2: Some(c),
                        ..Default::default()
                    }),
                    None,
                );
            }
            // The whole numbering group of the selected counter (ZK-169): selected, so every
            // change of colour, shape, size or effects goes to all of it — or deleted.
            "counter-edit-group" | "counter-delete-group" => {
                let ids: Vec<ObjectId> = {
                    let Some(s) = self.s.as_ref() else { return };
                    let numbering =
                        s.ed.selection()
                            .iter()
                            .find_map(|id| match s.ed.doc.get(*id)?.data {
                                Data::Counter { group, .. } => Some(group),
                                _ => None,
                            });
                    let Some(n) = numbering else { return };
                    s.ed.doc
                        .objects
                        .iter()
                        .filter(|o| matches!(o.data, Data::Counter { group, .. } if group == n))
                        .map(|o| o.id)
                        .collect()
                };
                let cmd = if name == "counter-edit-group" {
                    Command::Select { ids, add: false }
                } else {
                    Command::DeleteObjects { ids }
                };
                self.apply(ui, cmd);
            }
            "counter-group-new" => {
                // The next counters number themselves from 1 in a group of their own.
                let top = self
                    .s
                    .as_ref()
                    .map(|s| {
                        s.ed.doc
                            .objects
                            .iter()
                            .filter_map(|o| match o.data {
                                Data::Counter { group, .. } => Some(group),
                                _ => None,
                            })
                            .max()
                            .unwrap_or(0)
                    })
                    .unwrap_or(0);
                self.counter_group = top.max(self.counter_group) + 1;
                if !self.selection().is_empty() {
                    self.apply(ui, Command::ClearSelection);
                }
                self.set_tool(ui, tool::COUNTER);
            }
            // Fixed sizes instead of the pin arrows — the rotation handle turns a pin any way
            // (ZK-166). Sizes are steps of the automatic one, so they fit any screenshot; a
            // selected counter keeps its centre.
            "counter-size" => {
                self.counter_size_i = (v.max(0) as usize).min(COUNTER_SIZES.len() - 1);
                let d = counter_size(self.badge(), self.counter_size_i);
                let items: Vec<(ObjectId, IRect)> = {
                    let Some(s) = self.s.as_ref() else { return };
                    s.ed.selection()
                        .iter()
                        .filter_map(|id| s.ed.doc.get(*id))
                        .filter(|o| o.kind() == Kind::Counter)
                        .map(|o| {
                            let (cx, cy) = o.bounds().center();
                            let h = o.rect.h * d / o.rect.w.max(1);
                            let r = IRect::new(
                                (cx - d as f64 / 2.0).round() as i32,
                                (cy - h as f64 / 2.0).round() as i32,
                                d,
                                h,
                            );
                            (o.id, r)
                        })
                        .collect()
                };
                let merge = (items.len() > 1).then(|| self.merge_key());
                for (id, rect) in items {
                    self.apply(
                        ui,
                        Command::UpdateObjects {
                            ids: vec![id],
                            patch: ObjectPatch {
                                rect: Some(rect),
                                ..Default::default()
                            },
                            merge: merge.clone(),
                        },
                    );
                }
            }
            // Fixed stamp sizes instead of W / H fields (ZK-173, owner 30.09); a selected stamp
            // keeps its centre and stays square.
            "stamp-size" => {
                self.stamp_size_i = (v.max(0) as usize).min(COUNTER_SIZES.len() - 1);
                let d = stamp_size(self.badge(), self.stamp_size_i);
                let items: Vec<(ObjectId, IRect)> = {
                    let Some(s) = self.s.as_ref() else { return };
                    s.ed.selection()
                        .iter()
                        .filter_map(|id| s.ed.doc.get(*id))
                        .filter(|o| o.kind() == Kind::Stamp)
                        .map(|o| {
                            let (cx, cy) = o.bounds().center();
                            let h = (d as f64 / 2.0).round() as i32;
                            let r = IRect::new(cx.round() as i32 - h, cy.round() as i32 - h, d, d);
                            (o.id, r)
                        })
                        .collect()
                };
                let merge = (items.len() > 1).then(|| self.merge_key());
                for (id, rect) in items {
                    self.apply(
                        ui,
                        Command::UpdateObjects {
                            ids: vec![id],
                            patch: ObjectPatch {
                                rect: Some(rect),
                                style: Some(StylePatch {
                                    thick: Some(d),
                                    ..Default::default()
                                }),
                                ..Default::default()
                            },
                            merge: merge.clone(),
                        },
                    );
                }
            }
            // The counter's colour ⇄ its number's (ZK-166); an automatic number swaps as the
            // colour it shows.
            "swap-digit" => {
                let auto = znimok_render::on_color;
                let digit = self.digit.unwrap_or_else(|| auto(self.color));
                (self.color, self.digit) = (digit, Some(self.color));
                let items = self.style_each(
                    |o| o.kind() == Kind::Counter,
                    |o| StylePatch {
                        color: Some(o.style.color2.unwrap_or_else(|| auto(o.style.color))),
                        color2: Some(Some(o.style.color)),
                        ..Default::default()
                    },
                );
                self.apply_each(ui, items);
            }
            "stamp" => {
                self.stamp_id = v.max(0) as u32;
                let id = self.stamp_id;
                self.patch_data(ui, Kind::Stamp, move |d| {
                    if let Data::Stamp { id: s } = d {
                        *s = id;
                    }
                });
                if self.selected_where(|o| o.kind() == Kind::Stamp).is_empty() {
                    self.set_tool(ui, tool::STAMP);
                }
            }
            "align" => {
                self.align = match v {
                    1 => Align::Center,
                    2 => Align::Right,
                    _ => Align::Left,
                };
                let a = self.align;
                self.patch_text(ui, move |t| {
                    if let Data::Text { align, .. } = t {
                        *align = a;
                    }
                });
            }
            "box-auto" => {
                // The block is as wide as its longest line again.
                self.patch_text(ui, |t| {
                    if let Data::Text { box_w, .. } = t {
                        *box_w = 0;
                    }
                });
            }
            "stroke-none" => {
                // A shape without an outline is a plate of one colour — the fill's, or the
                // outline's when there was no fill: never both colours gone (ZK-161).
                self.no_stroke = true;
                if self.fill.is_none() {
                    self.fill = Some(self.color);
                }
                let items = self.style_each(
                    |o| matches!(o.kind(), Kind::Rect | Kind::Ellipse) && !is_plate(o),
                    |o| StylePatch {
                        color: Some(o.style.color2.unwrap_or(o.style.color)),
                        no_main: Some(true),
                        color2: Some(None),
                        ..Default::default()
                    },
                );
                self.apply_each(ui, items);
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
                let old = self.fill;
                self.fill = col;
                // No fill on a plate: its colour becomes the outline instead (ZK-161).
                if self.fill.is_none() && self.no_stroke {
                    self.no_stroke = false;
                    if let Some(f) = old {
                        self.color = f;
                    }
                }
                let c2 = self.fill;
                // A plate (no outline) is drawn in its main colour: the fill row edits that one
                // (owner, 29.09: with the outline off the fill could not be changed).
                let items = self.style_each(
                    |o| matches!(o.kind(), Kind::Rect | Kind::Ellipse),
                    |o| match (is_plate(o), c2) {
                        (true, Some(c)) => StylePatch {
                            color: Some(c),
                            ..Default::default()
                        },
                        (true, None) => StylePatch {
                            no_main: Some(false),
                            color2: Some(None),
                            ..Default::default()
                        },
                        (false, c2) => StylePatch {
                            color2: Some(c2),
                            ..Default::default()
                        },
                    },
                );
                self.apply_each(ui, items);
            }
            // The outline of a text (owner 29.09: it did nothing — the row went to "fill", which
            // only touches shapes).
            "outline" => {
                self.text_outline = col;
                let c2 = self.text_outline;
                self.patch_selected(
                    ui,
                    |o| o.kind() == Kind::Text,
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
                // Defaults: the pen's own when the Pen tool is on (or a pen is selected).
                let pen = self.tool == tool::PEN
                    || !self.selected_where(|o| o.kind() == Kind::Pen).is_empty();
                match (pen, start) {
                    (true, true) => self.pen_head_start = h,
                    (true, false) => self.pen_head_end = h,
                    (false, true) => self.head_start = h,
                    (false, false) => self.head_end = h,
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
                            Data::Pen {
                                ref points,
                                head_front,
                                head_back,
                            } => Some((
                                o.id,
                                Data::Pen {
                                    points: points.clone(),
                                    head_front: if start { head_front } else { h },
                                    head_back: if start { h } else { head_back },
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
            "text-step" => {
                // The type ladder (point sizes in screenshot pixels); the next one up or down
                // from the current size, so a custom size snaps back onto the ladder.
                const LADDER: [i32; 24] = [
                    8, 9, 10, 11, 12, 14, 16, 18, 20, 22, 24, 28, 32, 36, 40, 48, 56, 64, 72, 84,
                    96, 112, 128, 160,
                ];
                let cur = self
                    .selected_where(|o| o.kind() == Kind::Text)
                    .first()
                    .and_then(|id| self.s.as_ref()?.ed.doc.get(*id))
                    .and_then(|o| match o.data {
                        Data::Text { size, .. } => Some(size),
                        _ => None,
                    })
                    .unwrap_or_else(|| self.text_size());
                let next = if v > 0 {
                    LADDER.iter().copied().find(|s| *s > cur).unwrap_or(cur)
                } else {
                    LADDER
                        .iter()
                        .rev()
                        .copied()
                        .find(|s| *s < cur)
                        .unwrap_or(cur)
                };
                self.set_text_px(ui, next);
                return;
            }
            "swap" => {
                // Stroke ↔ fill (text: letters ↔ outline). A missing side swaps too: a plate
                // without an outline becomes an outlined empty shape and back (owner, 28.09).
                let entries: Vec<(ObjectId, Kind, Option<Rgb>, Option<Rgb>)> = {
                    let Some(s) = self.s.as_ref() else { return };
                    s.ed.selection()
                        .iter()
                        .filter_map(|id| s.ed.doc.get(*id))
                        .filter(|o| matches!(o.kind(), Kind::Rect | Kind::Ellipse | Kind::Text))
                        .map(|o| {
                            // A plate is all fill, in its main colour (ZK-161).
                            let plate = is_plate(o);
                            (
                                o.id,
                                o.kind(),
                                (!plate).then_some(o.style.color),
                                if plate {
                                    Some(o.style.color)
                                } else {
                                    o.style.color2
                                },
                            )
                        })
                        // Text keeps its letters: it swaps only when it has an outline.
                        .filter(|(_, k, st, fi)| {
                            if *k == Kind::Text {
                                fi.is_some()
                            } else {
                                st.is_some() || fi.is_some()
                            }
                        })
                        .collect()
                };
                if entries.is_empty() {
                    // Nothing selected: the defaults for new marks.
                    let stroke = (!self.no_stroke).then_some(self.color);
                    let fill = self.fill;
                    if stroke.is_some() || fill.is_some() {
                        self.no_stroke = fill.is_none();
                        if let Some(f) = fill {
                            self.color = f;
                        }
                        self.fill = stroke;
                    }
                } else {
                    let merge = (entries.len() > 1).then(|| self.merge_key());
                    for (id, kind, stroke, fill) in entries {
                        let sp = if kind == Kind::Text {
                            StylePatch {
                                color: fill,
                                no_main: Some(fill.is_none()),
                                color2: Some(stroke),
                                ..Default::default()
                            }
                        } else {
                            // Outline ↔ fill; without a fill the outline becomes a plate of
                            // its colour (the swapped shape shows one colour, never none).
                            StylePatch {
                                color: fill.or(stroke),
                                no_main: Some(fill.is_none()),
                                color2: Some(if fill.is_some() { stroke } else { None }),
                                ..Default::default()
                            }
                        };
                        self.apply(
                            ui,
                            Command::UpdateObjects {
                                ids: vec![id],
                                patch: ObjectPatch {
                                    style: Some(sp),
                                    ..Default::default()
                                },
                                merge: merge.clone(),
                            },
                        );
                    }
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

    /// Size field of the text section: any point size 4…1600.
    pub fn set_text_size(&mut self, ui: &AppWindow, text: &str) {
        match text.trim().parse::<i32>() {
            Ok(px) => self.set_text_px(ui, px.clamp(4, 1600)),
            Err(_) => self.sync(ui),
        }
    }

    fn set_text_px(&mut self, ui: &AppWindow, px: i32) {
        self.text_px = px;
        let ids = self.selected_where(|o| o.kind() == Kind::Text);
        if !ids.is_empty() {
            let merge = self.merge_key();
            self.apply(
                ui,
                Command::UpdateObjects {
                    ids: ids.clone(),
                    patch: ObjectPatch {
                        size: Some(px),
                        ..Default::default()
                    },
                    merge: Some(merge.clone()),
                },
            );
            for id in ids {
                self.fit_text(ui, id, Some(merge.clone()));
            }
        }
        self.sync(ui);
        ui.window().request_redraw();
    }

    /// Changes the data of every selected mark of a kind (one undo step).
    fn patch_data(&mut self, ui: &AppWindow, kind: Kind, f: impl Fn(&mut Data)) {
        let marks: Vec<(ObjectId, Data)> = {
            let Some(s) = self.s.as_ref() else { return };
            s.ed.selection()
                .iter()
                .filter_map(|id| s.ed.doc.get(*id))
                .filter(|o| o.kind() == kind)
                .map(|o| {
                    let mut d = o.data.clone();
                    f(&mut d);
                    (o.id, d)
                })
                .collect()
        };
        let merge = self.merge_key();
        for (id, data) in marks {
            self.apply(
                ui,
                Command::UpdateObjects {
                    ids: vec![id],
                    patch: ObjectPatch {
                        data: Some(data),
                        ..Default::default()
                    },
                    merge: Some(merge.clone()),
                },
            );
        }
    }

    /// "Start numbering from…" of the selected counter's group (or the group new ones join).
    pub fn set_counter_start(&mut self, ui: &AppWindow, text: &str) {
        let Ok(n) = text.trim().parse::<i32>() else {
            self.sync(ui);
            return;
        };
        let group = self
            .selected_where(|o| o.kind() == Kind::Counter)
            .first()
            .and_then(|id| self.s.as_ref()?.ed.doc.get(*id))
            .and_then(|o| match o.data {
                Data::Counter { group, .. } => Some(group),
                _ => None,
            })
            .unwrap_or(self.counter_group);
        let exists = self.s.as_ref().is_some_and(|s| {
            s.ed.doc
                .objects
                .iter()
                .any(|o| matches!(o.data, Data::Counter { group: g, .. } if g == group))
        });
        if exists {
            self.apply(
                ui,
                Command::SetCounterStart {
                    group,
                    start: n.clamp(-9999, 9999),
                },
            );
        }
        self.sync(ui);
        ui.window().request_redraw();
    }

    /// Changes the data of every selected text mark (one undo step) and re-measures them.
    fn patch_text(&mut self, ui: &AppWindow, f: impl Fn(&mut Data)) {
        let texts: Vec<(ObjectId, Data)> = {
            let Some(s) = self.s.as_ref() else { return };
            s.ed.selection()
                .iter()
                .filter_map(|id| s.ed.doc.get(*id))
                .filter(|o| o.kind() == Kind::Text)
                .map(|o| {
                    let mut d = o.data.clone();
                    f(&mut d);
                    (o.id, d)
                })
                .collect()
        };
        let merge = self.merge_key();
        for (id, data) in texts {
            self.apply(
                ui,
                Command::UpdateObjects {
                    ids: vec![id],
                    patch: ObjectPatch {
                        data: Some(data),
                        ..Default::default()
                    },
                    merge: Some(merge.clone()),
                },
            );
            self.fit_text(ui, id, Some(merge.clone()));
        }
    }

    /// Typed block width of a text: 0 or empty = as wide as the text.
    pub fn set_text_box(&mut self, ui: &AppWindow, text: &str) {
        let w = text.trim().parse::<i32>().unwrap_or(0).clamp(0, 20000);
        self.patch_text(ui, move |t| {
            if let Data::Text { box_w, .. } = t {
                *box_w = if w < 8 { 0 } else { w };
            }
        });
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
            // A line's rectangle runs from its start (x, y) to its end (x + w, y + h).
            "x1" | "y1" | "x2" | "y2" => {
                let mut n = r;
                match field {
                    "x1" => {
                        n.w = r.x + r.w - v;
                        n.x = v;
                    }
                    "y1" => {
                        n.h = r.y + r.h - v;
                        n.y = v;
                    }
                    "x2" => n.w = v - r.x,
                    _ => n.h = v - r.y,
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
            // The angle, degrees clockwise (ZK-164).
            "rot" => Command::UpdateObjects {
                ids: vec![id],
                patch: ObjectPatch {
                    rot: Some(v.rem_euclid(360) as u16),
                    ..Default::default()
                },
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

    /// Group header: selects every member (Shift / Ctrl adds them).
    pub fn layer_group_click(&mut self, ui: &AppWindow, g: i32, add: bool) {
        let ids = self.group_members(g as GroupId);
        if ids.is_empty() {
            return;
        }
        self.apply(ui, Command::Select { ids, add });
        self.sync(ui);
        ui.window().request_redraw();
    }

    /// Group header's eye: all members hidden → show them all, otherwise hide them all.
    pub fn layer_group_eye(&mut self, ui: &AppWindow, g: i32) {
        let ids = self.group_members(g as GroupId);
        let Some(s) = self.s.as_ref() else { return };
        let all_hidden = ids
            .iter()
            .filter_map(|id| s.ed.doc.get(*id))
            .all(|o| o.hidden);
        if ids.is_empty() {
            return;
        }
        self.apply(
            ui,
            Command::UpdateObjects {
                ids,
                patch: ObjectPatch {
                    hidden: Some(!all_hidden),
                    ..Default::default()
                },
                merge: None,
            },
        );
        self.sync(ui);
        ui.window().request_redraw();
    }

    pub fn layer_collapse(&mut self, ui: &AppWindow, g: i32) {
        let g = g as GroupId;
        if !self.collapsed.remove(&g) {
            self.collapsed.insert(g);
        }
        self.sync(ui);
    }

    fn group_members(&self, g: GroupId) -> Vec<ObjectId> {
        self.s
            .as_ref()
            .map(|s| {
                s.ed.doc
                    .objects
                    .iter()
                    .filter(|o| g != 0 && o.group == g)
                    .map(|o| o.id)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Dragging a row of the layers list: `y` from the list's top, rows 32 px + 2 px apart.
    /// Moving shows where it would land; dropping restacks in one undo step.
    pub fn layer_drag(&mut self, ui: &AppWindow, from: i32, y: f32, phase: i32) {
        let target = self.layer_drop_at(from as usize, y as f64);
        match phase {
            0 => {
                let (row, mode) = match target {
                    Some(LayerDrop::Before(r)) => (r as i32, 0),
                    Some(LayerDrop::Into(r)) => (r as i32, 1),
                    Some(LayerDrop::After(r)) => (r as i32, 2),
                    None => (-1, 0),
                };
                ui.set_layer_drop_row(row);
                ui.set_layer_drop_mode(mode);
            }
            1 => {
                ui.set_layer_drop_row(-1);
                if let Some(t) = target {
                    self.layer_drop(ui, from as usize, t);
                }
            }
            _ => ui.set_layer_drop_row(-1),
        }
    }

    /// Where a row dragged from `from` lands at `y`: top third of a row — in front of it,
    /// bottom third — behind it, the middle — onto it (group). A group moves as a block and
    /// is never put inside another group.
    fn layer_drop_at(&self, from: usize, y: f64) -> Option<LayerDrop> {
        const STRIDE: f64 = 34.0;
        let rows = &self.layer_rows;
        let dragged = *rows.get(from)?;
        if rows.is_empty() {
            return None;
        }
        if y < 0.0 {
            return Some(LayerDrop::Before(0));
        }
        let idx = (y / STRIDE).floor() as usize;
        if idx >= rows.len() {
            return Some(LayerDrop::After(rows.len() - 1));
        }
        let frac = (y - idx as f64 * STRIDE) / 32.0;
        // Rows that belong to the dragged thing are not targets.
        let inside = |r: LayerRef| match (dragged, r) {
            (a, b) if a == b => true,
            (LayerRef::Group(g), LayerRef::Mark { group, .. }) => group == g,
            _ => false,
        };
        if inside(rows[idx]) {
            return None;
        }
        let drop = if frac < 0.3 {
            LayerDrop::Before(idx)
        } else if frac > 0.7 {
            LayerDrop::After(idx)
        } else {
            LayerDrop::Into(idx)
        };
        // A group cannot go into a group: onto a row means next to it, at the top level.
        if let LayerRef::Group(_) = dragged {
            let unit_first = |i: usize| -> usize {
                match rows[i] {
                    LayerRef::Mark { group, .. } if group != 0 => rows
                        .iter()
                        .position(|r| *r == LayerRef::Group(group))
                        .unwrap_or(i),
                    _ => i,
                }
            };
            let unit_last = |i: usize| -> usize {
                let g = match rows[i] {
                    LayerRef::Group(g) => g,
                    LayerRef::Mark { group, .. } => group,
                };
                if g == 0 {
                    return i;
                }
                rows.iter()
                    .rposition(|r| {
                        matches!(r, LayerRef::Mark { group, .. } if *group == g)
                            || *r == LayerRef::Group(g)
                    })
                    .unwrap_or(i)
            };
            return Some(match drop {
                LayerDrop::After(i) => LayerDrop::After(unit_last(i)),
                LayerDrop::Before(i) | LayerDrop::Into(i) => LayerDrop::Before(unit_first(i)),
            });
        }
        Some(drop)
    }

    fn layer_drop(&mut self, ui: &AppWindow, from: usize, target: LayerDrop) {
        let Some(s) = self.s.as_ref() else { return };
        let doc = &s.ed.doc;
        let rows = self.layer_rows.clone();
        let Some(dragged) = rows.get(from).copied() else {
            return;
        };
        // Front-first stack of (mark, group).
        let mut stack: Vec<(ObjectId, GroupId)> =
            doc.objects.iter().rev().map(|o| (o.id, o.group)).collect();
        let moving: Vec<(ObjectId, GroupId)> = match dragged {
            LayerRef::Mark { id, group } => vec![(id, group)],
            LayerRef::Group(g) => stack.iter().copied().filter(|(_, gg)| *gg == g).collect(),
        };
        stack.retain(|e| !moving.iter().any(|m| m.0 == e.0));
        let next_group = doc.next_group_id();
        let group_of_row = |r: LayerRef| match r {
            LayerRef::Group(g) => g,
            LayerRef::Mark { group, .. } => group,
        };
        let pos_of =
            |stack: &Vec<(ObjectId, GroupId)>, id: ObjectId| stack.iter().position(|e| e.0 == id);
        let first_of =
            |stack: &Vec<(ObjectId, GroupId)>, g: GroupId| stack.iter().position(|e| e.1 == g);
        let last_of = |stack: &Vec<(ObjectId, GroupId)>, g: GroupId| {
            stack.iter().rposition(|e| e.1 == g).map(|i| i + 1)
        };
        // Index in `stack` to insert at, and the group the moved marks get (a group keeps its own).
        let (at, group): (Option<usize>, Option<GroupId>) = match (
            target,
            rows[match target {
                LayerDrop::Before(i) | LayerDrop::After(i) | LayerDrop::Into(i) => i,
            }],
        ) {
            (LayerDrop::Before(_), LayerRef::Group(g)) => (first_of(&stack, g), Some(0)),
            (LayerDrop::Before(_), LayerRef::Mark { id, group }) => {
                (pos_of(&stack, id), Some(group))
            }
            (LayerDrop::After(_), LayerRef::Group(g)) => {
                if self.collapsed.contains(&g) {
                    (last_of(&stack, g), Some(0))
                } else {
                    (first_of(&stack, g), Some(g))
                }
            }
            (LayerDrop::After(_), LayerRef::Mark { id, group }) => {
                (pos_of(&stack, id).map(|i| i + 1), Some(group))
            }
            (LayerDrop::Into(_), LayerRef::Group(g)) => (first_of(&stack, g), Some(g)),
            (LayerDrop::Into(_), LayerRef::Mark { id, group }) => {
                if group == 0 {
                    // A new group of the two: the target joins it too.
                    if let Some(i) = pos_of(&stack, id) {
                        stack[i].1 = next_group;
                    }
                    (pos_of(&stack, id), Some(next_group))
                } else {
                    (pos_of(&stack, id), Some(group))
                }
            }
        };
        let _ = group_of_row;
        let Some(at) = at else { return };
        let at = at.min(stack.len());
        let moved: Vec<(ObjectId, GroupId)> = moving
            .iter()
            .map(|(id, g)| match dragged {
                LayerRef::Group(_) => (*id, *g),
                LayerRef::Mark { .. } => (*id, group.unwrap_or(0)),
            })
            .collect();
        for (k, e) in moved.iter().enumerate() {
            stack.insert(at + k, *e);
        }
        let ids: Vec<ObjectId> = moved.iter().map(|e| e.0).collect();
        let order: Vec<ObjectId> = stack.iter().rev().map(|e| e.0).collect();
        let groups: Vec<GroupId> = stack.iter().rev().map(|e| e.1).collect();
        let same = doc
            .objects
            .iter()
            .map(|o| (o.id, o.group))
            .eq(order.iter().copied().zip(groups.iter().copied()));
        if !same {
            self.apply(ui, Command::Restack { order, groups });
            self.apply(ui, Command::Select { ids, add: false });
        }
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

    // ------------------------------------------------------------------ picture (ZK-53)

    fn image_rect(&self) -> IRect {
        let (w, h) = self
            .s
            .as_ref()
            .map(|s| s.ed.doc.image_size())
            .unwrap_or((1, 1));
        IRect::new(0, 0, w as i32, h as i32)
    }

    /// Screen positions of the crop handles, in the order of [`Drag::CropEdit`].
    fn crop_handles(&self, c: IRect) -> [Point; 8] {
        let (x0, y0) = (c.x as f64, c.y as f64);
        let (x1, y1) = (c.right() as f64, c.bottom() as f64);
        let (mx, my) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
        [
            (x0, y0),
            (mx, y0),
            (x1, y0),
            (x1, my),
            (x1, y1),
            (mx, y1),
            (x0, y1),
            (x0, my),
        ]
        .map(|(x, y)| self.view.to_out(Point::new(x, y)))
    }

    /// A handle of the frame under the pointer (over the screen).
    fn frame_handle_at(&self, c: IRect, out: Point) -> Option<usize> {
        let reach = 10.0 * self.dpr;
        self.crop_handles(c)
            .iter()
            .enumerate()
            .map(|(i, h)| (i, (*h - out).hypot()))
            .filter(|(_, d)| *d <= reach)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    fn crop_down(&mut self, out: Point, p: (i32, i32)) {
        let img = self.image_rect();
        let c = self.crop.unwrap_or(img);
        let reach = 12.0 * self.dpr;
        let handle = self
            .crop_handles(c)
            .iter()
            .enumerate()
            .map(|(i, h)| (i, (*h - out).hypot()))
            .filter(|(_, d)| *d <= reach)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i);
        let inside = p.0 > c.x && p.0 < c.right() && p.1 > c.y && p.1 < c.bottom();
        self.drag = Some(match handle {
            Some(h) => Drag::CropEdit {
                handle: h,
                orig: c,
                start: p,
            },
            None if inside => Drag::CropEdit {
                handle: 8,
                orig: c,
                start: p,
            },
            None => Drag::CropNew { start: p, prev: c },
        });
    }

    /// A new frame from two corners, inside the picture; Shift or the lock keep a square /
    /// the locked proportions.
    fn crop_from(&self, a: (i32, i32), b: (i32, i32), shift: bool) -> IRect {
        let img = self.image_rect();
        let cl = |p: (i32, i32)| (p.0.clamp(0, img.w), p.1.clamp(0, img.h));
        let (a, mut b) = (cl(a), cl(b));
        let ratio = if shift {
            Some(1.0)
        } else if self.crop_lock {
            self.crop.map(|c| c.w.max(1) as f64 / c.h.max(1) as f64)
        } else {
            None
        };
        if let Some(r) = ratio {
            let (dx, dy) = ((b.0 - a.0) as f64, (b.1 - a.1) as f64);
            let w = dx.abs().max(dy.abs() * r);
            let h = w / r;
            b = cl((
                a.0 + (w.copysign(dx)).round() as i32,
                a.1 + (h.copysign(dy)).round() as i32,
            ));
        }
        IRect::new(
            a.0.min(b.0),
            a.1.min(b.1),
            (a.0 - b.0).abs(),
            (a.1 - b.1).abs(),
        )
    }

    fn crop_edit(&self, handle: usize, o: IRect, d: (i32, i32)) -> IRect {
        let img = self.image_rect();
        if handle == 8 {
            return IRect::new(
                (o.x + d.0).clamp(0, (img.w - o.w).max(0)),
                (o.y + d.1).clamp(0, (img.h - o.h).max(0)),
                o.w,
                o.h,
            );
        }
        let (mut x0, mut y0, mut x1, mut y1) = (o.x, o.y, o.right(), o.bottom());
        if matches!(handle, 0 | 6 | 7) {
            x0 += d.0;
        }
        if matches!(handle, 2..=4) {
            x1 += d.0;
        }
        if matches!(handle, 0..=2) {
            y0 += d.1;
        }
        if matches!(handle, 4..=6) {
            y1 += d.1;
        }
        if self.crop_lock && o.w > 0 && o.h > 0 {
            let r = o.w as f64 / o.h as f64;
            match handle {
                // Edges: the other side follows about the centre.
                1 | 5 => {
                    let w = ((y1 - y0).abs() as f64 * r).round() as i32;
                    let cx = o.x + o.w / 2;
                    x0 = cx - w / 2;
                    x1 = x0 + w;
                }
                3 | 7 => {
                    let h = ((x1 - x0).abs() as f64 / r).round() as i32;
                    let cy = o.y + o.h / 2;
                    y0 = cy - h / 2;
                    y1 = y0 + h;
                }
                // Corners: the opposite corner stays.
                _ => {
                    let w = (x1 - x0).abs() as f64;
                    let h = (y1 - y0).abs() as f64;
                    let (w, h) = if w / r >= h { (w, w / r) } else { (h * r, h) };
                    let (w, h) = (w.round() as i32, h.round() as i32);
                    if matches!(handle, 0 | 6) {
                        x0 = x1 - w;
                    } else {
                        x1 = x0 + w;
                    }
                    if matches!(handle, 0 | 2) {
                        y0 = y1 - h;
                    } else {
                        y1 = y0 + h;
                    }
                }
            }
        }
        let (x0, x1) = (x0.min(x1).clamp(0, img.w), x0.max(x1).clamp(0, img.w));
        let (y0, y1) = (y0.min(y1).clamp(0, img.h), y0.max(y1).clamp(0, img.h));
        IRect::new(x0, y0, (x1 - x0).max(1), (y1 - y0).max(1))
    }

    /// The crop being edited goes into the document: one undo step.
    fn commit_crop(&mut self, ui: &AppWindow) {
        let Some(c) = self.crop.take() else { return };
        self.drag = None;
        let Some(s) = self.s.as_ref() else { return };
        if c == s.ed.doc.frame() || c.w < 1 || c.h < 1 {
            return;
        }
        let img = self.image_rect();
        self.apply(
            ui,
            Command::SetCrop {
                rect: (c != img).then_some(c),
            },
        );
        self.dirty = true;
    }

    /// W / H typed in the crop panel; the frame grows from its top-left corner.
    pub fn set_crop_size(&mut self, ui: &AppWindow, field: &str, text: &str) {
        let (Some(c), Ok(v)) = (self.crop, text.trim().parse::<i32>()) else {
            self.sync(ui);
            return;
        };
        let img = self.image_rect();
        let r = c.w.max(1) as f64 / c.h.max(1) as f64;
        let (mut w, mut h) = (c.w, c.h);
        if field == "w" {
            w = v.clamp(1, img.w);
            if self.crop_lock {
                h = (w as f64 / r).round() as i32;
            }
        } else {
            h = v.clamp(1, img.h);
            if self.crop_lock {
                w = (h as f64 * r).round() as i32;
            }
        }
        let (w, h) = (w.clamp(1, img.w), h.clamp(1, img.h));
        let x = c.x.min(img.w - w);
        let y = c.y.min(img.h - h);
        self.crop = Some(IRect::new(x, y, w, h));
        self.dirty = true;
        self.sync(ui);
        ui.window().request_redraw();
    }

    /// Buttons of the Image tab and the crop panel.
    pub fn image_action(&mut self, ui: &AppWindow, what: &str) {
        self.finish_text(ui);
        match what {
            "crop" => {
                self.set_tool(ui, tool::CROP);
                return;
            }
            "crop-done" => {
                self.set_tool(ui, tool::SELECT);
                return;
            }
            "crop-cancel" => {
                self.crop = None;
                self.set_tool(ui, tool::SELECT);
                return;
            }
            "crop-whole" => {
                if self.crop.is_some() {
                    self.crop = Some(self.image_rect());
                } else {
                    self.apply(ui, Command::SetCrop { rect: None });
                    self.fit();
                }
            }
            "crop-lock" => self.crop_lock = !self.crop_lock,
            "tone-reset" => {
                self.apply(ui, Command::ResetTone);
            }
            "rotate-left" | "rotate-right" | "mirror-h" | "mirror-v" => {
                // A turn moves the picture under the frame: the crop being edited goes in first.
                let cropping = self.crop.is_some();
                self.commit_crop(ui);
                let cmd = match what {
                    "rotate-left" => Command::Rotate { quarters: -1 },
                    "rotate-right" => Command::Rotate { quarters: 1 },
                    "mirror-h" => Command::Mirror,
                    _ => Command::MirrorVertical,
                };
                self.apply(ui, cmd);
                if cropping {
                    self.crop = self.s.as_ref().map(|s| s.ed.doc.frame());
                }
                self.stop_anim();
                self.fit();
            }
            "size-apply" => self.resize_image(ui),
            _ => return,
        }
        self.dirty = true;
        self.sync(ui);
        ui.window().request_redraw();
    }

    /// Exposure / gamma / contrast sliders, position 0…1; one undo step per drag.
    pub fn set_tone(&mut self, ui: &AppWindow, field: &str, pos: f32, last: bool) {
        let pos = pos.clamp(0.0, 1.0) as f64;
        // A little stickiness at the neutral middle, so "back to zero" is easy to hit.
        let pos = if (pos - 0.5).abs() < 0.02 { 0.5 } else { pos };
        let merge = match &self.tone_merge {
            Some(m) => m.clone(),
            None => {
                let m = self.merge_key();
                self.tone_merge = Some(m.clone());
                m
            }
        };
        let (mut exposure, mut gamma, mut contrast) = (None, None, None);
        match field {
            "exposure" => exposure = Some(((pos * 4.0 - 2.0) * 20.0).round() as f32 / 20.0),
            "gamma" => gamma = Some((2f64.powf(pos * 2.0 - 1.0) * 100.0).round() as f32 / 100.0),
            _ => contrast = Some((pos * 100.0 - 50.0).round() as i32),
        }
        self.apply(
            ui,
            Command::SetTone {
                exposure,
                gamma,
                contrast,
                merge: Some(merge),
            },
        );
        if last {
            self.tone_merge = None;
        }
        self.dirty = true;
        self.sync(ui);
        ui.window().request_redraw();
    }

    pub fn set_compare(&mut self, ui: &AppWindow, on: bool) {
        if self.compare != on {
            self.compare = on;
            self.dirty = true;
            ui.window().request_redraw();
        }
    }

    /// Typing W or H with "keep proportions" fills in the other one.
    pub fn size_edited(&mut self, ui: &AppWindow, field: &str, text: &str) {
        let Ok(v) = text.trim().parse::<u32>() else {
            return;
        };
        if !ui.get_size_keep() {
            return;
        }
        let img = self.image_rect();
        let (w, h) = (img.w, img.h);
        if field == "w" {
            let nh = (v as f64 * h as f64 / w.max(1) as f64).round().max(1.0) as u32;
            ui.set_size_h(nh.to_string().into());
        } else {
            let nw = (v as f64 * w as f64 / h.max(1) as f64).round().max(1.0) as u32;
            ui.set_size_w(nw.to_string().into());
        }
    }

    fn resize_image(&mut self, ui: &AppWindow) {
        let parse = |t: slint::SharedString| t.trim().parse::<u32>().ok().filter(|v| *v > 0);
        let (Some(w), Some(h)) = (parse(ui.get_size_w()), parse(ui.get_size_h())) else {
            self.size_shown = (0, 0);
            return;
        };
        let img = self.image_rect();
        let (cw, ch) = (img.w, img.h);
        if (w, h) == (cw as u32, ch as u32) {
            return;
        }
        self.commit_crop(ui);
        self.apply(
            ui,
            Command::ResizeImage {
                width: w.min(16384),
                height: h.min(16384),
                scale_text: ui.get_size_scale_text(),
            },
        );
        self.size_shown = (0, 0);
        self.stop_anim();
        self.fit();
    }

    /// Marks lying wholly outside a frame (they stay in the document, just out of the picture).
    fn outside(&self, c: IRect) -> usize {
        let Some(s) = self.s.as_ref() else { return 0 };
        s.ed.doc
            .objects
            .iter()
            .filter(|o| {
                let b = o.bounds();
                b.right() <= c.x || b.x >= c.right() || b.bottom() <= c.y || b.y >= c.bottom()
            })
            .count()
    }

    /// Image tab and crop panel state.
    fn sync_picture(&mut self, ui: &AppWindow) {
        let Some(s) = self.s.as_ref() else { return };
        let doc = &s.ed.doc;
        let r = doc.recipe;
        let (iw, ih) = doc.image_size();
        ui.set_tone_exposure(((r.exposure as f64 + 2.0) / 4.0).clamp(0.0, 1.0) as f32);
        ui.set_tone_gamma((((r.gamma as f64).log2() + 1.0) / 2.0).clamp(0.0, 1.0) as f32);
        ui.set_tone_contrast(((r.contrast as f64 + 50.0) / 100.0).clamp(0.0, 1.0) as f32);
        let signed = |v: f64, digits: usize| -> String {
            if v.abs() < 1e-9 {
                format!("{:.*}", digits, 0.0)
            } else {
                format!("{:+.*}", digits, v)
            }
        };
        ui.set_tone_exposure_text(format!("{} EV", signed(r.exposure as f64, 2)).into());
        ui.set_tone_gamma_text(format!("γ {:.2}", r.gamma).into());
        ui.set_tone_contrast_text(signed(r.contrast as f64, 0).into());
        ui.set_tone_default(znimok_render::develop::tone_is_default(&r));
        ui.set_cropped(doc.crop.is_some());
        ui.set_crop_lock(self.crop_lock);
        let shown = self.crop.unwrap_or_else(|| doc.frame());
        ui.set_crop_w(shown.w.to_string().into());
        ui.set_crop_h(shown.h.to_string().into());
        ui.set_crop_sizes(
            self.tr
                .tr_args(
                    "crop-sizes",
                    &args(&[
                        ("width", iw.to_string()),
                        ("height", ih.to_string()),
                        ("cw", shown.w.to_string()),
                        ("ch", shown.h.to_string()),
                    ]),
                )
                .into(),
        );
        let out = self.outside(shown);
        ui.set_crop_outside(if out > 0 {
            let mut a = FluentArgs::new();
            a.set("count", out as i64);
            self.tr.tr_args("crop-outside-kept", &a).into()
        } else {
            "".into()
        });
        ui.set_crop_text(if self.crop.is_some() || doc.crop.is_some() {
            self.tr
                .tr_args(
                    "status-crop-size",
                    &args(&[
                        ("width", shown.w.to_string()),
                        ("height", shown.h.to_string()),
                    ]),
                )
                .into()
        } else {
            "".into()
        });
        if self.size_shown != (iw, ih) {
            self.size_shown = (iw, ih);
            ui.set_size_w(iw.to_string().into());
            ui.set_size_h(ih.to_string().into());
        }
    }

    // ------------------------------------------------------------------ view

    /// What the view fits and keeps in sight: the frame, or the whole picture while cropping.
    fn view_frame(&self) -> Option<IRect> {
        let s = self.s.as_ref()?;
        Some(if self.crop.is_some() || self.over.is_some() {
            let (w, h) = s.ed.doc.image_size();
            IRect::new(0, 0, w as i32, h as i32)
        } else {
            s.ed.doc.frame()
        })
    }

    fn canvas_px(&self, ui: &AppWindow) -> (u32, u32) {
        let w = (ui.get_canvas_width() as f64 * self.dpr).round().max(1.0) as u32;
        let h = (ui.get_canvas_height() as f64 * self.dpr).round().max(1.0) as u32;
        (w.min(16384), h.min(16384))
    }

    fn set_zoom(&mut self, scale: f64, around_out: Option<Point>) {
        if self.over.is_some() {
            return;
        }
        let Some(f) = self.view_frame() else { return };
        let scale = scale.clamp(0.05, 16.0);
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
        // Over the screen a document pixel is a screen pixel, in place.
        if self.over.is_some() {
            self.view.scale = 1.0;
            self.view.origin = Point::ZERO;
            return;
        }
        let Some(f) = self.view_frame() else { return };
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
        let Some(f) = self.view_frame() else {
            return 1.0;
        };
        let margin = 24.0 * self.dpr;
        let w = (self.view.width as f64 - 2.0 * margin).max(16.0);
        let h = (self.view.height as f64 - 2.0 * margin).max(16.0);
        (w / f.w as f64).min(h / f.h as f64).min(1.0)
    }

    fn fit(&mut self) {
        if self.over.is_some() {
            self.constrain();
            self.dirty = true;
            return;
        }
        let k = self.fit_scale();
        self.set_zoom(k, None);
    }

    pub fn zoom_fit(&mut self, ui: &AppWindow) {
        let k = self.fit_scale();
        self.animate_zoom(k, None);
        self.sync(ui);
    }

    /// Ctrl+= / Ctrl+-: half a stop in or out, about the canvas centre.
    pub fn zoom_step(&mut self, ui: &AppWindow, dir: i32) {
        let k = self.view.scale * 2f64.powf(0.5 * dir as f64);
        let c = Point::new(self.view.width as f64 / 2.0, self.view.height as f64 / 2.0);
        self.animate_zoom(k, Some(c));
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
                {
                    let me = self.me();
                    move || me.with(|a, ui| a.tick_anim(ui))
                },
            );
        }
    }

    /// For the self-test: settle a zoom animation at once.
    pub fn stop_anim_for_test(&mut self) {
        self.stop_anim();
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
    /// The fitted scale for the current canvas (self-test, ZK-127).
    pub fn fit_probe(&self) -> f64 {
        self.fit_scale()
    }

    /// Self-test (ZK-130): brings the canvas up to date the way a frame does (partially), then
    /// repaints it in full and counts the pixels that differ.
    pub fn canvas_vs_full(&mut self, ui: &AppWindow) -> usize {
        self.dirty = true;
        self.before_rendering(ui);
        let shown = self.pixmap.data_as_u8_slice().to_vec();
        self.tracker.reset();
        self.dirty = true;
        self.before_rendering(ui);
        let full = self.pixmap.data_as_u8_slice();
        if full.len() != shown.len() {
            return usize::MAX;
        }
        full.chunks(4)
            .zip(shown.chunks(4))
            .filter(|(a, b)| a.iter().zip(b.iter()).any(|(x, y)| x.abs_diff(*y) > 1))
            .count()
    }

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

    /// The picture as a PNG file for dragging out (ZK-64): a messenger or a folder takes a file.
    /// Named after the document; one temporary folder, overwritten on the next drag.
    pub fn drag_file(&mut self) -> Result<PathBuf, String> {
        let name = file_safe(&self.doc_name());
        let dir = std::env::temp_dir().join("Znimok").join("drag");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = dir.join(format!("{name}.png"));
        let (w, h, rgba) = self.flatten().ok_or("no document")?;
        io::write_image(&path, w, h, rgba, self.file_meta().as_ref())?;
        Ok(path)
    }

    /// Dragging the Copy button out of the window: the picture goes as a file.
    pub fn drag_out(&mut self, ui: &AppWindow) {
        self.finish_text(ui);
        let path = match self.drag_file() {
            Ok(p) => p,
            Err(e) => {
                let msg = format!("{} ({e})", self.tr.tr("export-error"));
                self.toast(ui, msg);
                return;
            }
        };
        #[cfg(windows)]
        {
            // OLE runs its own loop until the drop; the button release never reaches Slint.
            let r = crate::dnd_win::drag_files(vec![path]);
            release_pointer(ui.window());
            if let Err(e) = r {
                let msg = format!("{} ({e})", self.tr.tr("export-error"));
                self.toast(ui, msg);
            }
        }
        #[cfg(target_os = "macos")]
        {
            // AppKit takes the mouse from here: the release goes to the drag session.
            let started = crate::dnd_mac::drag_from(ui.window(), &path);
            release_pointer(ui.window());
            if !started {
                let msg = self.tr.tr("export-error");
                self.toast(ui, msg);
            }
        }
    }

    pub fn copy(&mut self, ui: &AppWindow) {
        self.finish_text(ui);
        self.set_last_share(ui, false);
        let Some((w, h, rgba)) = self.flatten() else {
            return;
        };
        let msg = match io::copy_image(w, h, rgba) {
            Ok(()) => {
                // The Copy button turns into a tick for a second (ZK-126); a repeat restarts it.
                ui.set_copied(false);
                ui.set_copied(true);
                self.tr.tr("clipboard-copied")
            }
            Err(e) => format!("{} ({e})", self.tr.tr("clipboard-error")),
        };
        self.toast(ui, msg);
    }

    /// «Зберегти як…» (ZK-65): a copy of the document as a .znimok file anywhere; the folder is
    /// remembered. The library keeps its own file; the copy is for sending or keeping elsewhere.
    pub fn save_as(&mut self, ui: &AppWindow) {
        self.finish_text(ui);
        let Some(doc) = self.s.as_ref().map(|s| s.ed.doc.clone()) else {
            return;
        };
        let dir = self.prefs().editor.save_dir;
        let mut dlg = rfd::FileDialog::new()
            .add_filter("Znimok", &["znimok"])
            .set_file_name(format!("{}.znimok", file_safe(&doc.name)));
        if let Some(d) = dir.filter(|d| d.is_dir()) {
            dlg = dlg.set_directory(d);
        }
        let Some(mut path) = dlg.save_file() else {
            return;
        };
        if path.extension().is_none() {
            path.set_extension("znimok");
        }
        let opts = self.options_for(&doc);
        let video = self.s.as_ref().and_then(|s| s.video.clone());
        let r = znimok_format::save_same_kind(&path, &doc, video.as_ref(), &opts).map(|_| ());
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let msg = match r {
            Ok(()) => {
                if let Some(parent) = path.parent().map(Path::to_path_buf) {
                    self.save_prefs(ui, |p| p.editor.save_dir = Some(parent));
                }
                self.tr.tr_args("doc-saved-as", &args(&[("name", name)]))
            }
            Err(e) => format!("{} ({e})", self.tr.tr("export-error")),
        };
        self.toast(ui, msg);
    }

    /// What an exported file says about the document (ZK-61).
    pub fn file_meta(&self) -> Option<crate::filemeta::FileMeta> {
        self.s
            .as_ref()
            .map(|s| crate::filemeta::FileMeta::from_doc(&s.ed.doc))
    }

    /// The "write metadata" switch of the Copy menu.
    pub fn toggle_export_meta(&mut self, ui: &AppWindow) {
        let on = !crate::filemeta::enabled();
        self.setting(ui, "metadata", on as i32);
    }

    /// Remembers Copy or Export for Enter and shows which one Enter repeats.
    pub fn set_last_share(&mut self, ui: &AppWindow, export: bool) {
        self.last_export = export;
        ui.global::<crate::Keys>().set_last_share(export as i32);
    }

    // ---------------------------------------------------------------- the export sheet (ZK-187)

    /// Opens the export sheet with the choices last kept.
    pub fn export_open(&mut self, ui: &AppWindow) {
        let Some((w, h, rgba)) = self.flatten() else {
            return;
        };
        let prefs = self.prefs().editor.export;
        // The preview: the picture small, as it will look (marks included).
        let (pw, ph) = {
            let k = (360.0 / w as f64).min(200.0 / h as f64).min(1.0);
            (
                ((w as f64 * k).round() as u32).max(1),
                ((h as f64 * k).round() as u32).max(1),
            )
        };
        let (pw, ph, small) = io::scaled(w, h, rgba.clone(), pw, ph);
        let buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(&small, pw, ph);
        ui.set_exp_preview(slint::Image::from_rgba8(buf));
        ui.set_exp_name(self.doc_name().into());
        ui.set_exp_metadata(crate::filemeta::enabled());
        self.exp = Some(ExportSheet {
            w,
            h,
            rgba: std::sync::Arc::new(rgba),
            prefs,
            sizes: [None; 3],
            serial: 0,
        });
        ui.set_exp_open(true);
        self.export_show(ui);
        self.export_estimate();
    }

    /// The sheet's controls from its state.
    fn export_show(&self, ui: &AppWindow) {
        use znimok_settings::{ExportFormat as F, ExportScale as S, ExportTo as T};
        let Some(e) = self.exp.as_ref() else { return };
        let p = &e.prefs;
        let est = |i: usize| match e.sizes[i] {
            Some(b) => format!("≈ {}", self.human_size(b)),
            None => "…".to_string(),
        };
        let card = |title: &str, sub: &str, desc: &str, i: usize| crate::ExportCard {
            title: self.tr.tr(title).into(),
            sub: self.tr.tr(sub).into(),
            desc: self.tr.tr(desc).into(),
            estimate: est(i).into(),
            enabled: true,
        };
        let cards = vec![
            card("export-png", "export-png-sub", "export-png-desc", 0),
            card("export-jpeg", "export-jpeg-sub", "export-jpeg-desc", 1),
            card("export-webp", "export-webp-sub", "export-webp-desc", 2),
        ];
        ui.set_exp_cards(std::rc::Rc::new(slint::VecModel::from(cards)).into());
        let fi = match p.format {
            F::Png => 0,
            F::Jpeg => 1,
            F::Webp => 2,
        };
        ui.set_exp_format(fi);
        ui.set_exp_quality(match p.format {
            F::Webp => p.webp_quality,
            _ => p.jpeg_quality,
        } as i32);
        ui.set_exp_lossless(p.webp_lossless);
        ui.set_exp_scale(match p.scale {
            S::Half => 0,
            S::Full => 1,
            S::Double => 2,
            S::Width => 3,
        });
        ui.set_exp_width(p.width.to_string().into());
        ui.set_exp_white(p.white_bg || p.format == F::Jpeg);
        ui.set_exp_white_forced(p.format == F::Jpeg);
        ui.set_exp_remember(p.remember);
        ui.set_exp_to(match p.to {
            T::Clipboard => 0,
            T::File => 1,
            T::Library => 2,
        });
        let (tw, th) = export_size(e.w, e.h, p);
        ui.set_exp_subtitle(
            self.tr
                .tr_args(
                    "export-subtitle",
                    &args(&[
                        ("w", e.w.to_string()),
                        ("h", e.h.to_string()),
                        ("tw", tw.to_string()),
                        ("th", th.to_string()),
                    ]),
                )
                .into(),
        );
        ui.set_exp_estimate(est(fi as usize).into());
        let format = ["PNG", "JPEG", "WebP"][fi as usize];
        ui.set_exp_go(
            match p.to {
                T::Clipboard => self.tr.tr("export-go-copy"),
                T::Library => self.tr.tr("export-go-library"),
                T::File => self
                    .tr
                    .tr_args("export-go", &args(&[("format", format.to_string())])),
            }
            .into(),
        );
    }

    /// «86 KB» / «1.4 MB».
    fn human_size(&self, bytes: u64) -> String {
        if bytes < 1024 * 1024 {
            self.tr.tr_args(
                "size-kb",
                &args(&[("n", (bytes.div_ceil(1024)).to_string())]),
            )
        } else {
            self.tr.tr_args(
                "size-mb",
                &args(&[("n", format!("{:.1}", bytes as f64 / 1048576.0))]),
            )
        }
    }

    /// Size estimates on a worker thread: each format encoded on a small copy, scaled to the
    /// chosen size by the number of pixels. The newest request wins.
    fn export_estimate(&mut self) {
        use znimok_settings::ExportFormat as F;
        let Some(e) = self.exp.as_mut() else { return };
        e.serial += 1;
        e.sizes = [None; 3];
        let (serial, w, h, rgba, prefs) = (e.serial, e.w, e.h, e.rgba.clone(), e.prefs.clone());
        let me = self.me();
        std::thread::spawn(move || {
            let (tw, th) = export_size(w, h, &prefs);
            let k = (640.0 / w.max(h) as f64).min(1.0);
            let (sw, sh) = (
                ((w as f64 * k).round() as u32).max(1),
                ((h as f64 * k).round() as u32).max(1),
            );
            let (sw, sh, small) = io::scaled(w, h, (*rgba).clone(), sw, sh);
            let ratio = (tw as f64 * th as f64) / (sw as f64 * sh as f64).max(1.0);
            let mut sizes = [0u64; 3];
            for (i, f) in [F::Png, F::Jpeg, F::Webp].into_iter().enumerate() {
                let opts = export_encode(&prefs, f);
                let n = io::encode(sw, sh, &small, opts, None).map_or(0, |b| b.len());
                sizes[i] = (n as f64 * ratio) as u64;
            }
            let _ = slint::invoke_from_event_loop(move || {
                me.with(|a, ui| a.export_estimated(ui, serial, sizes));
            });
        });
    }

    fn export_estimated(&mut self, ui: &AppWindow, serial: u64, sizes: [u64; 3]) {
        if let Some(e) = self.exp.as_mut()
            && e.serial == serial
        {
            e.sizes = sizes.map(Some);
            self.export_show(ui);
        }
    }

    /// A control of the sheet changed.
    pub fn export_set(&mut self, ui: &AppWindow, key: &str, v: i32) {
        use znimok_settings::{ExportFormat as F, ExportScale as S, ExportTo as T};
        if key == "metadata" {
            // The same switch as in Settings → Sharing.
            self.setting(ui, "metadata", v);
            ui.set_exp_metadata(v != 0);
            return;
        }
        let Some(e) = self.exp.as_mut() else { return };
        let p = &mut e.prefs;
        let mut estimate = true;
        match key {
            "format" => {
                p.format = match v {
                    1 => F::Jpeg,
                    2 => F::Webp,
                    _ => F::Png,
                }
            }
            "quality" => {
                let q = v.clamp(1, 100) as u8;
                match p.format {
                    F::Webp => p.webp_quality = q,
                    _ => p.jpeg_quality = q,
                }
            }
            "lossless" => p.webp_lossless = v != 0,
            "scale" => {
                p.scale = match v {
                    0 => S::Half,
                    2 => S::Double,
                    3 => S::Width,
                    _ => S::Full,
                }
            }
            "white" => p.white_bg = v != 0,
            "remember" => {
                p.remember = v != 0;
                estimate = false;
            }
            "to" => {
                p.to = match v {
                    0 => T::Clipboard,
                    2 => T::Library,
                    _ => T::File,
                };
                estimate = false;
            }
            _ => return,
        }
        self.export_show(ui);
        if estimate {
            self.export_estimate();
        }
    }

    /// The width field: a number of pixels, the height follows.
    pub fn export_width(&mut self, ui: &AppWindow, text: &str) {
        if let Ok(v) = text.trim().parse::<u32>()
            && let Some(e) = self.exp.as_mut()
        {
            e.prefs.width = v.clamp(16, 16384);
            e.prefs.scale = znimok_settings::ExportScale::Width;
            self.export_show(ui);
            self.export_estimate();
        } else {
            self.export_show(ui);
        }
    }

    pub fn export_close(&mut self, ui: &AppWindow) {
        self.exp = None;
        ui.set_exp_open(false);
    }

    /// «Export»: to the clipboard or the library here; for a file, what the save dialog needs
    /// (folder, name with its extension, format) — the caller asks, then `export_write`.
    pub fn export_go(
        &mut self,
        ui: &AppWindow,
    ) -> Option<(Option<PathBuf>, String, znimok_settings::ExportFormat)> {
        use znimok_settings::ExportTo as T;
        let e = self.exp.as_ref()?;
        let prefs = e.prefs.clone();
        let name = file_safe(ui.get_exp_name().trim());
        let name = if name.is_empty() {
            self.doc_name()
        } else {
            name
        };
        match prefs.to {
            T::File => Some((
                self.prefs().editor.export_dir,
                format!("{name}.{}", export_ext(prefs.format)),
                prefs.format,
            )),
            T::Clipboard | T::Library => {
                self.export_run(ui, &prefs, None, &name);
                self.export_close(ui);
                None
            }
        }
    }

    /// The file the dialog gave: written with the sheet's choices.
    pub fn export_write(&mut self, ui: &AppWindow, path: &Path) {
        let Some(prefs) = self.exp.as_ref().map(|e| e.prefs.clone()) else {
            return;
        };
        let name = path
            .file_stem()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if self.export_run(ui, &prefs, Some(path), &name) {
            self.export_close(ui);
        }
    }

    /// Ctrl+Shift+E / Enter after an export: the last export again, without the sheet — a file
    /// goes to the same folder under the document's name (never over another file). `false`
    /// when there is nothing to repeat yet (the caller opens the sheet).
    pub fn export_repeat(&mut self, ui: &AppWindow) -> bool {
        use znimok_settings::ExportTo as T;
        let Some(prefs) = self.exp_last.clone() else {
            return false;
        };
        let name = self.doc_name();
        let path = match prefs.to {
            T::File => {
                let Some(dir) = self.prefs().editor.export_dir else {
                    return false;
                };
                let ext = export_ext(prefs.format);
                let mut path = dir.join(format!("{name}.{ext}"));
                let mut n = 2;
                while path.exists() {
                    path = dir.join(format!("{name} {n}.{ext}"));
                    n += 1;
                }
                Some(path)
            }
            _ => None,
        };
        self.export_run(ui, &prefs, path.as_deref(), &name);
        true
    }

    /// Renders at the chosen size and sends it where `prefs` says; remembers the choices.
    fn export_run(
        &mut self,
        ui: &AppWindow,
        prefs: &znimok_settings::ExportPrefs,
        path: Option<&Path>,
        name: &str,
    ) -> bool {
        use znimok_settings::ExportTo as T;
        let Some((w, h, rgba)) = self.flatten() else {
            return false;
        };
        let (tw, th) = export_size(w, h, prefs);
        let (tw, th, rgba) = io::scaled(w, h, rgba, tw, th);
        let ok = match (prefs.to, path) {
            (T::Clipboard, _) => {
                let r = io::copy_image(tw, th, rgba);
                let msg = if r.is_ok() {
                    self.tr.tr("clipboard-copied")
                } else {
                    self.tr.tr("clipboard-error")
                };
                self.toast(ui, msg);
                r.is_ok()
            }
            (T::Library, _) => match self.store_quietly(ui, Raster::new(tw, th, rgba), "export") {
                Ok(_) => {
                    let msg = self.tr.tr("export-library-done");
                    self.toast(ui, msg);
                    true
                }
                Err(e) => {
                    self.toast(ui, e);
                    false
                }
            },
            (T::File, Some(path)) => {
                let opts = export_encode(prefs, prefs.format);
                let meta = self.file_meta();
                let r = io::encode(tw, th, &rgba, opts, meta.as_ref())
                    .and_then(|b| std::fs::write(path, b).map_err(|e| e.to_string()));
                if r.is_ok()
                    && let Some(m) = meta.as_ref()
                {
                    crate::filemeta::set_file_time(path, m.created_ms);
                }
                let shown = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| name.to_string());
                let msg = match &r {
                    Ok(()) => self
                        .tr
                        .tr_args("export-done-toast", &args(&[("name", shown)])),
                    Err(e) => format!("{} ({e})", self.tr.tr("export-error")),
                };
                self.toast(ui, msg);
                if r.is_ok() {
                    let dir = path.parent().map(Path::to_path_buf);
                    self.save_prefs(ui, |p| p.editor.export_dir = dir);
                }
                r.is_ok()
            }
            (T::File, None) => false,
        };
        if ok {
            self.exp_last = Some(prefs.clone());
            if prefs.remember {
                let keep = prefs.clone();
                self.save_prefs(ui, |p| p.editor.export = keep);
            }
            self.set_last_share(ui, true);
        }
        ok
    }

    /// For the self-test: the sheet's current estimates (PNG, JPEG, WebP).
    pub fn export_sizes(&self) -> Option<[Option<u64>; 3]> {
        self.exp.as_ref().map(|e| e.sizes)
    }

    pub fn export_to(&mut self, ui: &AppWindow, path: &Path) {
        let Some((w, h, rgba)) = self.flatten() else {
            return;
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let meta = self.file_meta();
        let msg = match io::write_image(path, w, h, rgba, meta.as_ref()) {
            Ok(()) => self
                .tr
                .tr_args("export-done-toast", &args(&[("name", name)])),
            Err(e) => format!("{} ({e})", self.tr.tr("export-error")),
        };
        self.toast(ui, msg);
    }

    // ------------------------------------------------------------------ UI state

    pub fn sync(&mut self, ui: &AppWindow) {
        self.sync_video(ui);
        let Some(s) = self.s.as_ref() else { return };
        let doc = &s.ed.doc;
        if self.over.is_some() {
            let f = self.crop.unwrap_or_else(|| doc.frame());
            let a = self.view.to_out(Point::new(f.x as f64, f.y as f64));
            let b = self
                .view
                .to_out(Point::new(f.right() as f64, f.bottom() as f64));
            let k = self.dpr.max(0.1);
            ui.set_ov_x((a.x / k) as f32);
            ui.set_ov_y((a.y / k) as f32);
            ui.set_ov_w(((b.x - a.x) / k) as f32);
            ui.set_ov_h(((b.y - a.y) / k) as f32);
        }
        ui.set_doc_name(doc.name.as_str().into());
        if self.role == crate::wins::Role::Editor {
            let title = format!("{} — Znimok", doc.name);
            if ui.get_window_title() != title {
                ui.set_window_title(title.into());
            }
        }
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
                // A counter says which numbering group it is in (ZK-169).
                let title = o.name.clone().unwrap_or_else(|| match o.data {
                    Data::Counter { group, .. } => self
                        .tr
                        .tr_args("counter-group-title", &args(&[("n", group.to_string())])),
                    _ => tool_name(kind),
                });
                ui.set_prop_title(title.into());
                let st = &o.style;
                let plate = matches!(o.kind(), Kind::Rect | Kind::Ellipse) && st.no_main;
                let (i, c) = if o.kind() == Kind::Mark {
                    ui.set_marker_size(
                        (0..MARKER_SIZES.len())
                            .find(|i| self.marker_thick(*i) == o.rect.h.abs())
                            .map_or(-1, |i| i as i32),
                    );
                    colour_shown_in(&MARKER_PALETTE, Some(st.color))
                } else {
                    colour_shown((!plate).then_some(st.color))
                };
                ui.set_color_index(i);
                ui.set_color_rgb(c);
                ui.set_thick_index(
                    THICK
                        .iter()
                        .position(|t| *t == st.thick)
                        .map_or(-1, |i| i as i32),
                );
                // A plate's colour is its fill (ZK-161).
                let fill = if plate { Some(st.color) } else { st.color2 };
                let (i, c) = colour_shown(fill);
                ui.set_fill_index(i);
                ui.set_fill_rgb(c);
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
                if let Data::Pen {
                    head_front,
                    head_back,
                    ..
                } = o.data
                {
                    ui.set_head_start(head_index(head_back));
                    ui.set_head_end(head_index(head_front));
                }
                ui.set_shadow_index(effect_index(st.shadow));
                ui.set_glow_index(effect_index(st.glow));
                ui.set_geom_x2((o.rect.x + o.rect.w).to_string().into());
                ui.set_geom_y2((o.rect.y + o.rect.h).to_string().into());
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
                    ui.set_text_size_px(size.to_string().into());
                    ui.set_text_bold(bold);
                    ui.set_text_italic(italic);
                }
                if let Data::Counter { shape, start, .. } = o.data {
                    ui.set_counter_shape(shape_index(shape));
                    ui.set_counter_start(start.to_string().into());
                    let (i, c) = colour_shown(st.color2);
                    ui.set_digit_index(i);
                    ui.set_digit_rgb(c);
                    let badge = self.badge();
                    ui.set_counter_size(
                        (0..COUNTER_SIZES.len())
                            .find(|i| counter_size(badge, *i) == o.rect.w)
                            .map_or(-1, |i| i as i32),
                    );
                }
                if let Data::Stamp { id } = o.data {
                    ui.set_stamp_id(id as i32);
                    let badge = self.badge();
                    ui.set_stamp_size(
                        (0..COUNTER_SIZES.len())
                            .find(|i| stamp_size(badge, *i) == o.rect.w)
                            .map_or(-1, |i| i as i32),
                    );
                }
                if let Data::Text { align, box_w, .. } = o.data {
                    ui.set_text_align(align_index(align));
                    ui.set_text_box(
                        if box_w > 0 {
                            box_w.to_string()
                        } else {
                            String::new()
                        }
                        .into(),
                    );
                }
                ui.set_geom_x(o.rect.x.to_string().into());
                ui.set_geom_y(o.rect.y.to_string().into());
                ui.set_geom_w(o.rect.w.to_string().into());
                ui.set_geom_h(o.rect.h.to_string().into());
                ui.set_geom_rot(o.rot.to_string().into());
                ui.set_prop_can_turn(o.kind().can_rotate());
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
                let plate = self.no_stroke && matches!(self.tool, tool::RECT | tool::ELLIPSE);
                let (i, c) = if self.tool == tool::MARKER {
                    colour_shown_in(&MARKER_PALETTE, Some(self.marker_color))
                } else {
                    colour_shown((!plate).then_some(self.color))
                };
                ui.set_color_index(i);
                ui.set_color_rgb(c);
                ui.set_marker_size(self.marker_size_i as i32);
                ui.set_thick_index(self.thick as i32);
                let fill = if self.tool == tool::TEXT {
                    self.text_outline
                } else {
                    self.fill
                };
                let (i, c) = colour_shown(fill);
                ui.set_fill_index(i);
                ui.set_fill_rgb(c);
                ui.set_dash_index(dash_index(self.dash));
                ui.set_corners_index(corners_index(self.corners));
                ui.set_alpha(self.alpha as f32 / 100.0);
                if self.tool == tool::PEN {
                    ui.set_head_start(head_index(self.pen_head_start));
                    ui.set_head_end(head_index(self.pen_head_end));
                } else {
                    ui.set_head_start(head_index(self.head_start));
                    ui.set_head_end(head_index(self.head_end));
                }
                ui.set_shadow_index(effect_index(self.shadow));
                ui.set_glow_index(effect_index(self.glow));
                ui.set_text_size_index(self.text_size_i as i32);
                ui.set_text_size_px(self.text_size().to_string().into());
                ui.set_text_bold(self.bold);
                ui.set_text_italic(self.italic);
                ui.set_text_align(align_index(self.align));
                ui.set_text_box("".into());
                ui.set_counter_shape(shape_index(self.counter_shape));
                ui.set_counter_size(self.counter_size_i as i32);
                let (i, c) = colour_shown(self.digit);
                ui.set_digit_index(i);
                ui.set_digit_rgb(c);
                ui.set_stamp_id(self.stamp_id as i32);
                ui.set_stamp_size(self.stamp_size_i as i32);
                // What the next counter will say.
                let (start, n) = doc
                    .objects
                    .iter()
                    .filter_map(|o| match o.data {
                        Data::Counter { group, start, .. } if group == self.counter_group => {
                            Some(start)
                        }
                        _ => None,
                    })
                    .fold((1, 0), |(_, n), st| (st, n + 1));
                ui.set_counter_start(start.to_string().into());
                ui.set_counter_next(
                    self.tr
                        .tr_args("counter-next", &args(&[("n", (start + n).to_string())]))
                        .into(),
                );
            }
        }
        // Layers: front first; unnamed marks are "<kind> <n>", numbered per kind bottom-up. A
        // group is a header row with its members indented under it (members are adjacent).
        let mut counts = [0usize; 10];
        let names: Vec<String> = doc
            .objects
            .iter()
            .map(|o| {
                let k = kind_index(o.kind());
                counts[k as usize] += 1;
                o.name
                    .clone()
                    .unwrap_or_else(|| format!("{} {}", tool_name(k), counts[k as usize]))
            })
            .collect();
        let mut rows: Vec<LayerRow> = Vec::new();
        let mut refs: Vec<LayerRef> = Vec::new();
        let mut shown_groups: Vec<GroupId> = Vec::new();
        for (i, o) in doc.objects.iter().enumerate().rev() {
            let g = o.group;
            if g != 0 && !shown_groups.contains(&g) {
                shown_groups.push(g);
                let members: Vec<&Object> = doc.objects.iter().filter(|m| m.group == g).collect();
                let name = doc.group_names.get(&g).cloned().unwrap_or_else(|| {
                    let mut a = FluentArgs::new();
                    a.set("n", shown_groups.len() as i64);
                    self.tr.tr_args("layers-group-default", &a)
                });
                rows.push(LayerRow {
                    id: g as i32,
                    name: name.into(),
                    kind: 10,
                    hidden: members.iter().all(|m| m.hidden),
                    selected: members.iter().all(|m| sel.contains(&m.id)),
                    is_group: true,
                    depth: 0,
                    collapsed: self.collapsed.contains(&g),
                    count: members.len() as i32,
                });
                refs.push(LayerRef::Group(g));
            }
            if g != 0 && self.collapsed.contains(&g) {
                continue;
            }
            rows.push(LayerRow {
                id: o.id as i32,
                name: names[i].as_str().into(),
                kind: kind_index(o.kind()),
                hidden: o.hidden,
                selected: sel.contains(&o.id),
                is_group: false,
                depth: (g != 0) as i32,
                collapsed: false,
                count: 0,
            });
            refs.push(LayerRef::Mark { id: o.id, group: g });
        }
        self.layer_rows = refs;
        {
            use slint::Model;
            let m = &self.layers_model;
            if m.row_count() == rows.len() {
                for (i, r) in rows.into_iter().enumerate() {
                    if m.row_data(i).as_ref() != Some(&r) {
                        m.set_row_data(i, r);
                    }
                }
            } else {
                m.set_vec(rows);
            }
            if !self.layers_bound {
                ui.set_layers(m.clone().into());
                self.layers_bound = true;
            }
        }
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
        ui.set_hint(
            if self.crop.is_some() {
                format!(
                    "Enter — {} · Esc — {} · {}",
                    self.tr.tr("crop-hint-done"),
                    self.tr.tr("crop-hint-cancel"),
                    self.tr.tr("crop-hint-move")
                )
            } else if self.editing.is_some() {
                format!(
                    "Enter — {} · Shift+Enter — {} · Esc — {}",
                    self.tr.tr("text-hint-done"),
                    self.tr.tr("text-hint-newline"),
                    self.tr.tr("text-hint-cancel")
                )
            } else {
                self.tr.tr(tool::NAMES[self.tool])
            }
            .into(),
        );
        ui.set_autosave(self.autosave);
        self.sync_picture(ui);
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
            // A fitted picture follows the window: it grows with it up to 100 % and shrinks
            // with it (owner, 29.09, ZK-127); a zoom the person chose is left alone.
            let fitted = (self.view.scale - self.fit_scale()).abs() < 1e-3;
            let c = self
                .view
                .to_doc(self.view.width as f64 / 2.0, self.view.height as f64 / 2.0);
            self.view.width = w.min(65535) as u16;
            self.view.height = h.min(65535) as u16;
            let sc = self.view.scale.max(1e-6);
            self.view.origin = Point::new(c.x - w as f64 / 2.0 / sc, c.y - h as f64 / 2.0 / sc);
            if fitted && !self.fit_pending {
                self.fit();
                self.sync(ui);
            } else if !self.fit_pending {
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
        // Cropping shows the whole picture; "Compare" shows the tone as captured.
        let shown_crop = self
            .crop
            .or_else(|| self.over.as_ref().map(|_| s.ed.doc.frame()));
        // A video (ZK-181): the frame the player delivered stands in for the poster.
        let frame = s.vid.as_ref().and_then(|v| v.raster.clone());
        let doc: std::borrow::Cow<Document> =
            if shown_crop.is_some() || self.compare || frame.is_some() {
                let mut d = s.ed.doc.clone();
                if let Some(r) = frame
                    && d.banks
                        .get(d.source as usize)
                        .is_some_and(|b| (b.width, b.height) == (r.width, r.height))
                {
                    d.banks[d.source as usize] = r;
                }
                if shown_crop.is_some() {
                    d.crop = None;
                }
                if self.compare {
                    d.recipe.exposure = 0.0;
                    d.recipe.gamma = 1.0;
                    d.recipe.contrast = 0;
                }
                std::borrow::Cow::Owned(d)
            } else {
                std::borrow::Cow::Borrowed(&s.ed.doc)
            };
        let frame_rect = if self.crop.is_some() {
            let (w, h) = s.ed.doc.image_size();
            IRect::new(0, 0, w as i32, h as i32)
        } else {
            s.ed.doc.frame()
        };
        let dark = ui.global::<crate::Theme>().get_dark();
        // ZK-130: only what changed is rendered and uploaded. A new texture, the crop frame
        // (it dims the whole canvas) and any change of view repaint everything.
        let fresh = size_changed
            || self.frame_changed
            || self.gpu.as_ref().is_none_or(|g| g.texture.is_none());
        self.frame_changed = false;
        let repaint = if fresh || shown_crop.is_some() {
            self.tracker.reset();
            Repaint::All
        } else {
            let extra = {
                use std::hash::{Hash, Hasher};
                let mut h = std::hash::DefaultHasher::new();
                (dark, self.dpr.to_bits(), self.over.is_some()).hash(&mut h);
                h.finish()
            };
            self.renderer
                .changes(&doc, self.view, extra, &mut self.tracker)
        };
        match &repaint {
            Repaint::All => {
                self.renderer.render(&doc, self.view, &mut self.base);
                if let Some(c) = shown_crop {
                    draw_crop(&mut self.base, &self.view, c, self.dpr, self.over.is_none());
                }
                if self.over.is_none() {
                    draw_frame_edge(&mut self.base, &self.view, frame_rect, self.dpr, dark, None);
                }
            }
            Repaint::Rects(rs) => {
                self.renderer
                    .render_rects(&doc, self.view, rs, &mut self.base);
                if self.over.is_none() {
                    draw_frame_edge(
                        &mut self.base,
                        &self.view,
                        frame_rect,
                        self.dpr,
                        dark,
                        Some(rs),
                    );
                }
            }
            Repaint::Nothing => {}
        }

        // The selection, caret and marquee: where they go this frame.
        let typing = self.editing.as_ref().and_then(|e| e.id);
        let line = self.text_size() as f64 * 1.25;
        let caret = match self.editing.as_ref() {
            None => None,
            Some(ed) => match ed.id.and_then(|id| s.ed.doc.get(id)) {
                Some(o) => self.renderer.text_caret(o, ed.cursor, ed.anchor),
                // Nothing typed yet: a caret one line high where the text will start.
                None => {
                    let (x, y) = (ed.at.0 as f64, ed.at.1 as f64);
                    Some((
                        znimok_render::vello_cpu::kurbo::Rect::new(x, y, x + 1.0, y + line),
                        Vec::new(),
                    ))
                }
            },
        };
        let selected: Vec<&Object> =
            s.ed.selection()
                .iter()
                .filter(|id| Some(**id) != typing)
                .filter_map(|id| s.ed.doc.get(*id))
                .collect();
        let mut overlay: Vec<IRect> = Vec::new();
        if let Some((c, sel)) = &caret {
            for r in sel.iter().chain(std::iter::once(c)) {
                overlay.push(out_box(&self.view, *r, (4.0 * self.dpr).ceil() as i32 + 2));
            }
        }
        for o in &selected {
            overlay.push(selection_box(&self.view, o, self.dpr));
        }
        let grip = if typing.is_none() {
            self.rotation_grip()
        } else {
            None
        };
        if let Some((g, _, base)) = grip {
            let r = znimok_render::vello_cpu::kurbo::Rect::from_points(
                Point::new(g.0, g.1),
                Point::new(base.0, base.1),
            );
            overlay.push(out_box(&self.view, r, (8.0 * self.dpr).ceil() as i32 + 2));
        }
        let text_boxes: Vec<IRect> = self
            .text
            .as_ref()
            .map(|t| t.lines.iter().map(|l| l.rect).collect())
            .unwrap_or_default();
        for r in &text_boxes {
            let k = znimok_render::vello_cpu::kurbo::Rect::new(
                r.x as f64,
                r.y as f64,
                r.right() as f64,
                r.bottom() as f64,
            );
            overlay.push(out_box(&self.view, k, 4));
        }
        if let Some(m) = self.marquee {
            let r = znimok_render::vello_cpu::kurbo::Rect::new(
                m.x as f64,
                m.y as f64,
                m.right() as f64,
                m.bottom() as f64,
            );
            overlay.push(out_box(&self.view, r, 2));
        }

        // `pixmap` = `base` again where the picture changed or an overlay was or will be.
        let canvas = IRect::new(0, 0, w as i32, h as i32);
        let restore: Option<Vec<IRect>> = match &repaint {
            Repaint::All => None,
            Repaint::Rects(rs) => Some(rs.clone()),
            Repaint::Nothing => Some(Vec::new()),
        }
        .map(|mut v| {
            v.extend(self.overlay_prev.iter().chain(&overlay).copied());
            znimok_render::merge_rects(v.into_iter().filter_map(|r| clip_rect(r, canvas)).collect())
        });
        match &restore {
            None => {
                if self.pixmap.width() != self.base.width()
                    || self.pixmap.height() != self.base.height()
                {
                    self.pixmap = self.base.clone();
                } else {
                    self.pixmap.data_mut().copy_from_slice(self.base.data());
                }
            }
            Some(rs) => {
                let pw = self.base.width() as usize;
                let (src, dst) = (self.base.data(), self.pixmap.data_mut());
                for r in rs {
                    for y in r.y as usize..r.bottom() as usize {
                        let o = y * pw;
                        dst[o + r.x as usize..o + r.right() as usize]
                            .copy_from_slice(&src[o + r.x as usize..o + r.right() as usize]);
                    }
                }
            }
        }
        if let Some((c, sel)) = &caret {
            draw_text_caret(
                &mut self.pixmap,
                &self.view,
                *c,
                sel,
                self.caret_on,
                self.dpr,
            );
        }
        for o in &selected {
            draw_selection(&mut self.pixmap, &self.view, o, self.dpr);
        }
        if let Some((g, _, base)) = grip {
            draw_rotation_grip(&mut self.pixmap, &self.view, base, g, self.dpr);
        }
        for r in &text_boxes {
            draw_text_box(&mut self.pixmap, &self.view, *r);
        }
        if let Some(m) = self.marquee {
            draw_marquee(&mut self.pixmap, &self.view, m);
        }
        self.overlay_prev = overlay;

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
        let whole = [canvas];
        let upload: &[IRect] = restore.as_deref().unwrap_or(&whole);
        for r in upload {
            gpu.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: r.x as u32,
                        y: r.y as u32,
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                self.pixmap.data_as_u8_slice(),
                wgpu::TexelCopyBufferLayout {
                    offset: (r.y as u64 * w as u64 + r.x as u64) * 4,
                    bytes_per_row: Some(w * 4),
                    rows_per_image: Some(r.h as u32),
                },
                wgpu::Extent3d {
                    width: r.w as u32,
                    height: r.h as u32,
                    depth_or_array_layers: 1,
                },
            );
        }
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

#[derive(Debug, PartialEq)]
pub enum KeyAction {
    None,
    Copy,
    Export,
    Open,
    /// The last Esc: back to the library (after the unsaved-changes question, if any).
    Back,
    /// Ctrl+S over the screen: to the library and close.
    Save,
    /// Ctrl+Shift+E, Enter after an export: the last export again, without the sheet (ZK-187).
    ExportRepeat,
    /// I: a picture from a file as a mark (ZK-163).
    InsertImage,
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

/// A document name as a file name: characters Windows and macOS refuse become "_".
pub fn file_safe(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_control() || r#"<>:"/\|?*"#.contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let s = s.trim().trim_end_matches('.').to_string();
    if s.is_empty() {
        "Znimok".to_string()
    } else {
        s
    }
}

/// Tells Slint the button is up after a system drag took the mouse, so the Copy button does
/// not stay pressed (and does not copy on the next move).
#[cfg(any(windows, target_os = "macos"))]
pub fn release_pointer(w: &slint::Window) {
    use slint::platform::{PointerEventButton, WindowEvent};
    let at = slint::LogicalPosition::new(-10.0, -10.0);
    w.dispatch_event(WindowEvent::PointerReleased {
        position: at,
        button: PointerEventButton::Left,
    });
    w.dispatch_event(WindowEvent::PointerExited);
}

/// The picture's edge on the canvas (owner, 29.09: a white or transparent picture melted into
/// the canvas): a checkerboard under transparent parts, a hairline rim and a soft shadow around.
fn draw_frame_edge(
    pix: &mut Pixmap,
    view: &View,
    f: IRect,
    dpr: f64,
    dark: bool,
    clip: Option<&[IRect]>,
) {
    use znimok_render::vello_cpu::color::PremulRgba8;
    let (w, h) = (pix.width() as i64, pix.height() as i64);
    let a = view.to_out(Point::new(f.x as f64, f.y as f64));
    let b = view.to_out(Point::new(f.right() as f64, f.bottom() as f64));
    let (x0, y0, x1, y1) = (
        a.x.round() as i64,
        a.y.round() as i64,
        b.x.round() as i64,
        b.y.round() as i64,
    );
    let data = pix.data_mut();
    let whole = [IRect::new(0, 0, w as i32, h as i32)];
    let clip = clip.unwrap_or(&whole);
    let inside = |x: i64, y: i64| {
        clip.iter().any(|r| {
            x >= r.x as i64 && x < r.right() as i64 && y >= r.y as i64 && y < r.bottom() as i64
        })
    };
    // Transparent parts over a checkerboard of 8-point squares.
    let sq = (8.0 * dpr).round().max(4.0) as i64;
    let (c1, c2) = if dark {
        (58u16, 44u16)
    } else {
        (255u16, 226u16)
    };
    for r in clip {
        for y in y0.max(0).max(r.y as i64)..y1.min(h).min(r.bottom() as i64) {
            for x in x0.max(0).max(r.x as i64)..x1.min(w).min(r.right() as i64) {
                let p = &mut data[(y * w + x) as usize];
                if p.a == 255 {
                    continue;
                }
                let c = if ((x - x0) / sq + (y - y0) / sq) % 2 == 0 {
                    c1
                } else {
                    c2
                };
                let k = 255 - p.a as u16;
                // premultiplied: result = src + checker·(1 − αsrc)
                let mix = |v: u8| (v as u16 + c * k / 255).min(255) as u8;
                *p = PremulRgba8 {
                    r: mix(p.r),
                    g: mix(p.g),
                    b: mix(p.b),
                    a: 255,
                };
            }
        }
    }
    // Shadow: a few pixels fading out around the frame; the rim: one pixel.
    let blur = (6.0 * dpr).round() as i64;
    let mut dim = |x: i64, y: i64, k: u16| {
        if x >= 0
            && y >= 0
            && x < w
            && y < h
            && !(x >= x0 && x < x1 && y >= y0 && y < y1)
            && inside(x, y)
        {
            let p = &mut data[(y * w + x) as usize];
            let m = |v: u8| (v as u16 * (255 - k) / 255) as u8;
            *p = PremulRgba8 {
                r: m(p.r),
                g: m(p.g),
                b: m(p.b),
                a: p.a.max(k as u8),
            };
        }
    };
    for d in 1..=blur {
        let k = (if dark { 90 } else { 40 }) * (blur - d + 1) as u16 / blur as u16;
        for x in x0 - d..x1 + d {
            dim(x, y0 - d, k / 2);
            dim(x, y1 - 1 + d, k);
        }
        for y in y0 - d..y1 + d {
            dim(x0 - d, y, k / 2);
            dim(x1 - 1 + d, y, k / 2);
        }
    }
    let rim = if dark { 110 } else { 60 };
    for x in x0 - 1..=x1 {
        dim(x, y0 - 1, rim);
        dim(x, y1, rim);
    }
    for y in y0 - 1..=y1 {
        dim(x0 - 1, y, rim);
        dim(x1, y, rim);
    }
}

/// Caret and selection of the text being typed, in screen pixels: the selection as a translucent
/// accent under a thin outline, the caret as a 2-point bar with a light rim (readable on dark and
/// light pictures), blinking.
fn draw_text_caret(
    pix: &mut Pixmap,
    view: &View,
    caret: znimok_render::vello_cpu::kurbo::Rect,
    sel: &[znimok_render::vello_cpu::kurbo::Rect],
    on: bool,
    dpr: f64,
) {
    use znimok_render::vello_cpu::color::PremulRgba8;
    let (w, h) = (pix.width() as i64, pix.height() as i64);
    let data = pix.data_mut();
    let mut blend = |x: i64, y: i64, c: [u8; 3], k: u16| {
        if x >= 0 && y >= 0 && x < w && y < h {
            let p = &mut data[(y * w + x) as usize];
            let mix = |d: u8, v: u8| ((d as u16 * (255 - k) + v as u16 * k) / 255) as u8;
            *p = PremulRgba8 {
                r: mix(p.r, c[0]),
                g: mix(p.g, c[1]),
                b: mix(p.b, c[2]),
                a: mix(p.a, 255).max(p.a),
            };
        }
    };
    let accent = [0x3D, 0x7B, 0xF5];
    for r in sel {
        let a = view.to_out(Point::new(r.x0, r.y0));
        let b = view.to_out(Point::new(r.x1, r.y1));
        for y in a.y.round() as i64..b.y.round() as i64 {
            for x in a.x.round() as i64..b.x.round() as i64 {
                blend(x, y, accent, 90);
            }
        }
    }
    if !on || !sel.is_empty() {
        return;
    }
    let a = view.to_out(Point::new(caret.x0, caret.y0));
    let b = view.to_out(Point::new(caret.x0, caret.y1));
    let x = a.x.round() as i64;
    let bar = (2.0 * dpr).round().max(2.0) as i64;
    for y in a.y.round() as i64..b.y.round() as i64 {
        blend(x - 1, y, [255, 255, 255], 160);
        blend(x + bar, y, [255, 255, 255], 160);
        for dx in 0..bar {
            blend(x + dx, y, accent, 255);
        }
    }
}

/// Crop frame over the whole picture (ZK-53): outside dimmed, the rule of thirds inside, and
/// corner brackets / edge bars as handles — white with a dark rim, readable on any picture.
fn draw_crop(pix: &mut Pixmap, view: &View, c: IRect, dpr: f64, thirds: bool) {
    use znimok_render::vello_cpu::color::PremulRgba8;
    let (w, h) = (pix.width() as i64, pix.height() as i64);
    let p0 = view.to_out(Point::new(c.x as f64, c.y as f64));
    let p1 = view.to_out(Point::new(c.right() as f64, c.bottom() as f64));
    let (x0, y0, x1, y1) = (
        p0.x.round() as i64,
        p0.y.round() as i64,
        p1.x.round() as i64,
        p1.y.round() as i64,
    );
    let data = pix.data_mut();
    for y in 0..h {
        for x in 0..w {
            if x >= x0 && x < x1 && y >= y0 && y < y1 {
                continue;
            }
            let p = &mut data[(y * w + x) as usize];
            p.r = (p.r as u16 * 2 / 5) as u8;
            p.g = (p.g as u16 * 2 / 5) as u8;
            p.b = (p.b as u16 * 2 / 5) as u8;
        }
    }
    let mut blend = |x: i64, y: i64, v: u8, k: u16| {
        if x >= 0 && y >= 0 && x < w && y < h {
            let p = &mut data[(y * w + x) as usize];
            let mix = |d: u8| ((d as u16 * (255 - k) + v as u16 * k) / 255) as u8;
            *p = PremulRgba8 {
                r: mix(p.r),
                g: mix(p.g),
                b: mix(p.b),
                a: mix(p.a).max(p.a),
            };
        }
    };
    // Thirds: faint white lines (while cropping in the window).
    for i in (1..3).filter(|_| thirds) {
        let gx = x0 + (x1 - x0) * i / 3;
        let gy = y0 + (y1 - y0) * i / 3;
        for y in y0..y1 {
            blend(gx, y, 255, 90);
        }
        for x in x0..x1 {
            blend(x, gy, 255, 90);
        }
    }
    // Frame: a dark rim just outside, a white line on the edge.
    for x in x0 - 1..=x1 {
        blend(x, y0 - 1, 0, 120);
        blend(x, y1, 0, 120);
        blend(x, y0, 255, 230);
        blend(x, y1 - 1, 255, 230);
    }
    for y in y0 - 1..=y1 {
        blend(x0 - 1, y, 0, 120);
        blend(x1, y, 0, 120);
        blend(x0, y, 255, 230);
        blend(x1 - 1, y, 255, 230);
    }
    // Handles: L-brackets at the corners, short bars on the edges, 3 px thick (logical).
    let t = (3.0 * dpr).round().max(2.0) as i64;
    let len = ((18.0 * dpr).round() as i64)
        .min((x1 - x0) / 2)
        .min((y1 - y0) / 2)
        .max(t);
    let mut bar = |ax: i64, ay: i64, bw: i64, bh: i64| {
        for y in ay - 1..ay + bh + 1 {
            for x in ax - 1..ax + bw + 1 {
                let edge = x < ax || y < ay || x >= ax + bw || y >= ay + bh;
                blend(
                    x,
                    y,
                    if edge { 0 } else { 255 },
                    if edge { 110 } else { 255 },
                );
            }
        }
    };
    let (mx, my) = ((x0 + x1) / 2, (y0 + y1) / 2);
    // corners: horizontal and vertical arms, drawn outside the frame line
    bar(x0 - t, y0 - t, len + t, t);
    bar(x0 - t, y0, t, len);
    bar(x1 - len, y0 - t, len + t, t);
    bar(x1, y0, t, len);
    bar(x0 - t, y1, len + t, t);
    bar(x0 - t, y1 - len, t, len);
    bar(x1 - len, y1, len + t, t);
    bar(x1, y1 - len, t, len);
    // edges
    bar(mx - len / 2, y0 - t, len, t);
    bar(mx - len / 2, y1, len, t);
    bar(x0 - t, my - len / 2, t, len);
    bar(x1, my - len / 2, t, len);
}

/// A document-space rectangle on the canvas, padded by `pad` pixels (ZK-130).
fn out_box(view: &View, r: znimok_render::vello_cpu::kurbo::Rect, pad: i32) -> IRect {
    let a = view.to_out(Point::new(r.x0.min(r.x1), r.y0.min(r.y1)));
    let b = view.to_out(Point::new(r.x0.max(r.x1), r.y0.max(r.y1)));
    let lim = |v: f64| v.clamp(-1e7, 1e7) as i32;
    let (x0, y0) = (lim(a.x.floor()) - pad, lim(a.y.floor()) - pad);
    let (x1, y1) = (lim(b.x.ceil()) + pad, lim(b.y.ceil()) + pad);
    IRect::new(x0, y0, x1 - x0, y1 - y0)
}

/// Where [`draw_selection`] draws: the dashed outline and every handle.
fn selection_box(view: &View, o: &Object, dpr: f64) -> IRect {
    let hs = (4.0 * dpr).round() as i32 + 2;
    let b = o.bounds();
    let mut r = znimok_render::vello_cpu::kurbo::Rect::new(
        b.x as f64,
        b.y as f64,
        b.right() as f64,
        b.bottom() as f64,
    );
    for (hx, hy) in hit::handles(o) {
        r = r.union_pt(Point::new(hx, hy));
    }
    if hit::turned(o) {
        let c = b.center();
        for p in [
            (b.x as f64, b.y as f64),
            (b.right() as f64, b.y as f64),
            (b.right() as f64, b.bottom() as f64),
            (b.x as f64, b.bottom() as f64),
        ] {
            let q = hit::turn(p, c, o.rot as f64);
            r = r.union_pt(Point::new(q.0, q.1));
        }
    }
    out_box(view, r, hs)
}

fn clip_rect(r: IRect, to: IRect) -> Option<IRect> {
    let x0 = r.x.max(to.x);
    let y0 = r.y.max(to.y);
    let x1 = r.right().min(to.right());
    let y1 = r.bottom().min(to.bottom());
    (x1 > x0 && y1 > y0).then(|| IRect::new(x0, y0, x1 - x0, y1 - y0))
}

/// This build (ZK-140): the version and, for builds made by CI, the commit — so a newly
/// installed build differs even while every preview is 0.0.0.
pub fn build_id() -> String {
    format!(
        "{}+{}",
        env!("CARGO_PKG_VERSION"),
        option_env!("GITHUB_SHA").unwrap_or("local")
    )
}

/// Counter sizes of the inspector's S / M / L / XL (ZK-166), as parts of the automatic size.
pub const COUNTER_SIZES: [f64; 4] = [0.75, 1.0, 1.4, 2.0];

/// A stamp's side at size step `i` (ZK-173): the counter's steps, a little larger (LH: stamps
/// 28 / 40 / 56 next to counters of 24 / 32 / 44).
pub fn stamp_size(badge: i32, i: usize) -> i32 {
    counter_size(badge + 8, i)
}

/// A counter's width at size step `i` for an automatic size `badge`.
pub fn counter_size(badge: i32, i: usize) -> i32 {
    ((badge as f64 * COUNTER_SIZES[i.min(COUNTER_SIZES.len() - 1)]).round() as i32).max(8)
}

/// The export sheet (ZK-187): the picture as drawn (full size), the choices, the size estimates.
struct ExportSheet {
    w: u32,
    h: u32,
    rgba: std::sync::Arc<Vec<u8>>,
    prefs: znimok_settings::ExportPrefs,
    /// Estimates in bytes for PNG, JPEG, WebP at the chosen size; `None` while counting.
    sizes: [Option<u64>; 3],
    /// Only the newest estimate is shown.
    serial: u64,
}

/// The picture's size at `prefs`' scale.
fn export_size(w: u32, h: u32, prefs: &znimok_settings::ExportPrefs) -> (u32, u32) {
    use znimok_settings::ExportScale as S;
    let k = match prefs.scale {
        S::Half => 0.5,
        S::Full => 1.0,
        S::Double => 2.0,
        S::Width => prefs.width.clamp(16, 16384) as f64 / w.max(1) as f64,
    };
    (
        ((w as f64 * k).round() as u32).max(1),
        ((h as f64 * k).round() as u32).max(1),
    )
}

fn export_ext(f: znimok_settings::ExportFormat) -> &'static str {
    match f {
        znimok_settings::ExportFormat::Png => "png",
        znimok_settings::ExportFormat::Jpeg => "jpg",
        znimok_settings::ExportFormat::Webp => "webp",
    }
}

/// How the sheet's choices write a picture in format `f` (ZK-197: WebP has its own quality
/// and may be lossless).
fn export_encode(p: &znimok_settings::ExportPrefs, f: znimok_settings::ExportFormat) -> io::Encode {
    use znimok_settings::ExportFormat as F;
    io::Encode {
        format: export_io(f),
        quality: if f == F::Webp {
            p.webp_quality
        } else {
            p.jpeg_quality
        },
        lossless: p.webp_lossless,
        white_bg: p.white_bg,
    }
}

fn export_io(f: znimok_settings::ExportFormat) -> io::Format {
    match f {
        znimok_settings::ExportFormat::Png => io::Format::Png,
        znimok_settings::ExportFormat::Jpeg => io::Format::Jpeg,
        znimok_settings::ExportFormat::Webp => io::Format::Webp,
    }
}

/// The text panel's state (ZK-184): the lines found and whether a reading is running.
struct TextFind {
    lines: Vec<crate::text::Found>,
    busy: bool,
}

/// A point in screenshot coordinates.
type Pt = (f64, f64);

/// A line of recognised text on the canvas (ZK-184): a light accent wash with an accent rim.
fn draw_text_box(pix: &mut Pixmap, view: &View, r: IRect) {
    let a = view.to_out(Point::new(r.x as f64, r.y as f64));
    let b = view.to_out(Point::new(r.right() as f64, r.bottom() as f64));
    let (x0, y0) = (a.x.floor() as i64 - 2, a.y.floor() as i64 - 2);
    let (x1, y1) = (b.x.ceil() as i64 + 2, b.y.ceil() as i64 + 2);
    let (w, h) = (pix.width() as i64, pix.height() as i64);
    let data = pix.data_mut();
    let accent = [0x3D as f32, 0x7B as f32, 0xF5 as f32];
    for y in y0.max(0)..=y1.min(h - 1) {
        for x in x0.max(0)..=x1.min(w - 1) {
            let rim = x == x0 || x == x1 || y == y0 || y == y1;
            let k = if rim { 0.9 } else { 0.16 };
            let px = &mut data[(y * w + x) as usize];
            let mix = |c: u8, t: f32| (c as f32 * (1.0 - k) + t * k).round() as u8;
            px.r = mix(px.r, accent[0]);
            px.g = mix(px.g, accent[1]);
            px.b = mix(px.b, accent[2]);
        }
    }
}

/// The id of the first mark a rotation drag turns (for the angle shown beside the pointer).
fn orig_first_id(d: &Option<Drag>) -> ObjectId {
    match d {
        Some(Drag::Rotate { orig, .. }) => orig.first().map_or(0, |o| o.0),
        _ => 0,
    }
}

/// The rotation handle (ZK-164): a thin stem from the top edge and a white disc with a blue
/// ring at its end, drawn straight into the pixmap like the resize handles.
fn draw_rotation_grip(pix: &mut Pixmap, view: &View, foot: (f64, f64), grip: (f64, f64), dpr: f64) {
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
    let a = view.to_out(Point::new(foot.0, foot.1));
    let b = view.to_out(Point::new(grip.0, grip.1));
    let len = (b - a).hypot().max(1.0);
    for i in 0..=len as i64 {
        let t = i as f64 / len;
        put(
            (a.x + (b.x - a.x) * t).round() as i64,
            (a.y + (b.y - a.y) * t).round() as i64,
            blue,
        );
    }
    let r = 5.5 * dpr;
    let ring = 1.8 * dpr;
    let (cx, cy) = (b.x, b.y);
    let n = r.ceil() as i64 + 1;
    for y in -n..=n {
        for x in -n..=n {
            let d = ((x as f64).powi(2) + (y as f64).powi(2)).sqrt();
            if d <= r {
                let px = (cx + x as f64).round() as i64;
                let py = (cy + y as f64).round() as i64;
                put(px, py, if d > r - ring { blue } else { white });
            }
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
    if !o.kind().is_segment() && hit::turned(o) {
        // The dashed outline turns with the mark (LH): it would otherwise promise a box that
        // is not there.
        let b = o.bounds();
        let c = b.center();
        let corners = [
            (b.x as f64, b.y as f64),
            (b.right() as f64, b.y as f64),
            (b.right() as f64, b.bottom() as f64),
            (b.x as f64, b.bottom() as f64),
        ]
        .map(|p| {
            let q = hit::turn(p, c, o.rot as f64);
            view.to_out(Point::new(q.0, q.1))
        });
        for i in 0..4 {
            let (p, q) = (corners[i], corners[(i + 1) % 4]);
            let len = (q - p).hypot().max(1.0);
            for k in 0..=len as i64 {
                if k % 6 < 3 {
                    let t = k as f64 / len;
                    put(
                        (p.x + (q.x - p.x) * t).round() as i64,
                        (p.y + (q.y - p.y) * t).round() as i64,
                        blue,
                    );
                }
            }
        }
    } else if !o.kind().is_segment() {
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

fn shape_index(s: CounterShape) -> i32 {
    match s {
        CounterShape::Circle => 0,
        CounterShape::RoundedBox => 1,
        CounterShape::Pin => 2,
    }
}

fn align_index(a: Align) -> i32 {
    match a {
        Align::Left => 0,
        Align::Center => 1,
        Align::Right => 2,
    }
}

fn effect_index(e: Effect) -> i32 {
    match e {
        Effect::None => 0,
        Effect::Light => 1,
        Effect::Strong => 2,
    }
}

/// Tools whose marks take shadow and glow (as `Kind::fx_allowed`).
fn fx_tool(t: usize) -> bool {
    !matches!(t, tool::PEN | tool::HIDE | tool::MARKER)
}

fn head_index(h: Head) -> i32 {
    match h {
        Head::None => 0,
        Head::Triangle => 1,
        Head::Chevron => 2,
        Head::Dot => 3,
    }
}

// ------------------------------------------------------------------ video (ZK-181)

impl App {
    pub fn is_video(&self) -> bool {
        self.s.as_ref().is_some_and(|s| s.vid.is_some())
    }

    /// A video document opened in an editor window: the player and the timeline come up.
    fn video_open(&mut self, ui: &AppWindow) {
        let me = self.me();
        let role = self.role;
        let Some(s) = self.s.as_mut() else { return };
        let Some(part) = s.video.as_ref() else {
            ui.set_vid_mode(false);
            return;
        };
        if role != crate::wins::Role::Editor {
            return;
        }
        let info = &part.video.info;
        let mut v =
            crate::video::Vid::new(info.fps(), info.frames as i64, s.ed.doc.timeline.as_ref());
        match crate::video::Player::open(part, move |d| {
            let _ = slint::invoke_from_event_loop(move || me.with(|a, ui| a.video_frame(ui, d)));
        }) {
            Ok(p) => {
                p.set_edit(v.edit().clone());
                p.seek(0);
                v.player = Some(p);
            }
            Err(e) => v.player_error = Some(e),
        }
        s.vid = Some(v);
        ui.set_vid_mode(true);
        ui.set_insp_tab(2);
        self.sync_video(ui);
    }

    /// The player delivered a frame.
    pub fn video_frame(&mut self, ui: &AppWindow, d: crate::video::Delivered) {
        let Some(v) = self.s.as_mut().and_then(|s| s.vid.as_mut()) else {
            return;
        };
        v.frame = d.frame;
        v.raster = Some(d.raster);
        v.playing = d.playing;
        self.frame_changed = true;
        self.dirty = true;
        self.sync_video(ui);
        ui.window().request_redraw();
    }

    /// Everything the video bars and the «Відео» tab show.
    fn sync_video(&mut self, ui: &AppWindow) {
        let Some(s) = self.s.as_mut() else { return };
        let Some(v) = s.vid.as_mut() else { return };
        if let Some(t) = s.ed.doc.timeline.as_ref() {
            v.adopt(t);
        }
        let e = v.edit();
        let edited = e.is_edited();
        let kept = v.kept();
        ui.set_vid_playing(v.playing);
        ui.set_vid_can_play(v.player.is_some());
        ui.set_vid_time(v.time_str(v.frame).into());
        ui.set_vid_total(crate::video::fmt_time(kept as f64 / v.fps.max(1e-6)).into());
        ui.set_vid_total_full(
            if edited {
                v.time_str(v.frames)
            } else {
                String::new()
            }
            .into(),
        );
        ui.set_vid_frame_label(
            self.tr
                .tr_args("vid-frame-n", &count_args("n", v.frame))
                .into(),
        );
        ui.set_vid_speed(format!("{}×", v.speed()).into());
        ui.set_vid_loop(v.looping);
        ui.set_vid_muted(v.muted);
        ui.set_tl_focus(v.focus);
        ui.set_vid_edited(edited);
        ui.set_vid_trim_in(v.time_str(e.in_point()).into());
        ui.set_vid_trim_out(v.time_str(e.out_point()).into());
        let off: Vec<_> = e.parts().iter().filter(|p| p.off).collect();
        let cut_frames: i64 = off.iter().map(|p| p.b - p.a).sum();
        ui.set_vid_trim_cut(
            if off.is_empty() {
                self.tr.tr("common-off")
            } else {
                let mut a = count_args("count", off.len() as i64);
                a.set(
                    "seconds",
                    format!("{:.1}", cut_frames as f64 / v.fps.max(1e-6)),
                );
                self.tr.tr_args("vid-cut-count", &a)
            }
            .into(),
        );
        ui.set_vid_trim_left(crate::video::fmt_time(kept as f64 / v.fps.max(1e-6)).into());
        ui.set_vid_note(
            match (&v.player, &v.player_error) {
                (None, Some(_)) | (None, None) => self.tr.tr("vid-poster-note"),
                _ => String::new(),
            }
            .into(),
        );
        // The timeline.
        v.view.auto_scroll((v.frame as f64 + 0.5) / v.fps.max(1e-6));
        let g = v.geometry();
        let ticks: Vec<crate::TlTick> = g
            .ticks
            .into_iter()
            .map(|(x, label)| crate::TlTick {
                x: x as f32,
                label: label.into(),
            })
            .collect();
        let pieces: Vec<crate::TlPiece> = g
            .pieces
            .into_iter()
            .map(|(x, w, kind)| crate::TlPiece {
                x: x as f32,
                w: w as f32,
                kind,
            })
            .collect();
        ui.set_tl_ticks(std::rc::Rc::new(VecModel::from(ticks)).into());
        ui.set_tl_pieces(std::rc::Rc::new(VecModel::from(pieces)).into());
        ui.set_tl_in(g.in_x as f32);
        ui.set_tl_out(g.out_x as f32);
        ui.set_tl_playhead(g.playhead_x as f32);
        ui.set_tl_has_range(v.selected_range().is_some());
        // Sound: the tracks of the recording, and the choice they make together (ZK-189).
        let tracks = s
            .video
            .as_ref()
            .map(|p| p.video.audio.clone())
            .unwrap_or_default();
        use znimok_format::video::AudioSource;
        let has = |src: AudioSource| tracks.iter().any(|t| t.source == src);
        let on = |src: AudioSource| tracks.iter().any(|t| t.source == src && !t.muted);
        ui.set_vid_has_system(has(AudioSource::System));
        ui.set_vid_has_mic(has(AudioSource::Microphone));
        ui.set_vid_sound_mode(
            match (on(AudioSource::System), on(AudioSource::Microphone)) {
                (false, false) => 0,
                (true, false) => 1,
                (false, true) => 2,
                (true, true) => 3,
            },
        );
        let rows: Vec<crate::VidTrack> = tracks
            .iter()
            .map(|t| crate::VidTrack {
                label: if t.label.is_empty() {
                    self.tr.tr(match t.source {
                        AudioSource::System => "vid-track-system",
                        AudioSource::Microphone => "vid-track-mic",
                    })
                } else {
                    t.label.clone()
                }
                .into(),
                colour: match t.source {
                    AudioSource::System => slint::Color::from_rgb_u8(0x3D, 0x7B, 0xF5),
                    AudioSource::Microphone => slint::Color::from_rgb_u8(0x34, 0xC4, 0x8A),
                },
                muted: t.muted,
                volume: t.volume as i32,
            })
            .collect();
        ui.set_vid_tracks(std::rc::Rc::new(VecModel::from(rows)).into());
    }

    /// The frame to show: the player decodes it; without one the timeline still moves.
    fn vid_seek(&mut self, ui: &AppWindow, f: i64) {
        let Some(v) = self.s.as_mut().and_then(|s| s.vid.as_mut()) else {
            return;
        };
        let f = f.clamp(0, (v.frames - 1).max(0));
        v.playing = false;
        match &v.player {
            Some(p) => p.seek(f),
            None => v.frame = f,
        }
        v.frame = f;
        self.sync_video(ui);
    }

    /// A finished edit of the timeline: one command of the core editor (one undo step with the
    /// marks, ZK-144); the player learns the new cuts.
    fn tl_commit(&mut self, ui: &AppWindow, merge: Option<MergeKey>) {
        let Some(t) = self
            .s
            .as_ref()
            .and_then(|s| s.vid.as_ref())
            .map(|v| v.edit().to_timeline())
        else {
            return;
        };
        self.apply(ui, Command::SetTimeline { timeline: t, merge });
        if let Some(v) = self.s.as_ref().and_then(|s| s.vid.as_ref())
            && let Some(p) = &v.player
        {
            p.set_edit(v.edit().clone());
        }
        self.sync(ui);
        ui.window().request_redraw();
    }

    /// The transport bar and its keys.
    pub fn vid_transport(&mut self, ui: &AppWindow, what: &str) {
        let Some(v) = self.s.as_mut().and_then(|s| s.vid.as_mut()) else {
            return;
        };
        match what {
            "play" => {
                if v.playing {
                    v.playing = false;
                    if let Some(p) = &v.player {
                        p.pause();
                    }
                } else if let Some(p) = &v.player {
                    // At the end: from the start again.
                    let from = if v.frame >= v.end() {
                        v.home()
                    } else {
                        v.frame
                    };
                    v.playing = true;
                    p.play(from, v.speed(), v.looping);
                }
            }
            "back" => {
                let f = v.step(-1);
                self.vid_seek(ui, f);
                return;
            }
            "fwd" => {
                let f = v.step(1);
                self.vid_seek(ui, f);
                return;
            }
            "sec-back" => {
                let f = v.step(-(v.fps.round() as i64).max(1));
                self.vid_seek(ui, f);
                return;
            }
            "sec-fwd" => {
                let f = v.step((v.fps.round() as i64).max(1));
                self.vid_seek(ui, f);
                return;
            }
            "home" => {
                let f = v.home();
                self.vid_seek(ui, f);
                return;
            }
            "end" => {
                let f = v.end();
                self.vid_seek(ui, f);
                return;
            }
            "loop" => v.looping = !v.looping,
            "mute" => v.muted = !v.muted,
            "speed" => v.speed_i = (v.speed_i + 1) % crate::video::SPEEDS.len(),
            "fit" => {
                v.view.zoom = 1.0;
                v.view.off = 0.0;
            }
            "zoom-in" | "zoom-out" => {
                let x = v.track_w / 2;
                let n = if what == "zoom-in" { 1.0 } else { -1.0 };
                v.view.zoom_at(x, n, znimok_video::edit::MAX_PX_PER_FRAME);
            }
            _ => {}
        }
        self.sync_video(ui);
    }

    /// The width of the timeline's track area, from the window.
    pub fn tl_layout(&mut self, ui: &AppWindow, w: i32) {
        if let Some(v) = self.s.as_mut().and_then(|s| s.vid.as_mut()) {
            v.set_track_width(w);
            self.sync_video(ui);
        }
    }

    /// The pointer over the timeline's tracks: `kind` 0 down, 1 move, 2 up; `x`, `y` in the
    /// track area. The ruler scrubs; the strip takes a trim handle, a range or a click.
    pub fn tl_pointer(&mut self, ui: &AppWindow, kind: i32, x: i32, y: i32, _shift: bool) {
        let Some(v) = self.s.as_mut().and_then(|s| s.vid.as_mut()) else {
            return;
        };
        const RULER: i32 = 22;
        const STRIP_TOP: i32 = 26;
        const STRIP_BOTTOM: i32 = 78;
        match kind {
            0 => {
                v.focus = true;
                if y < RULER {
                    v.scrubbing = true;
                    let f = v.frame_at_x(x);
                    self.vid_seek(ui, f);
                } else if (STRIP_TOP..=STRIP_BOTTOM).contains(&y) {
                    if v.strip_press(x) {
                        // A handle: the frame at it is shown while it moves.
                        if let Some(f) = v.strip_move(x) {
                            self.vid_seek(ui, f);
                        }
                    }
                } else {
                    v.tl.clear_selection();
                }
                self.sync_video(ui);
            }
            1 => {
                if v.scrubbing {
                    let f = v.frame_at_x(x);
                    self.vid_seek(ui, f);
                    return;
                }
                if let Some(f) = v.strip_move(x) {
                    self.vid_seek(ui, f);
                    return;
                }
                self.sync_video(ui);
            }
            _ => {
                if v.scrubbing {
                    v.scrubbing = false;
                    return;
                }
                match v.strip_release() {
                    crate::video::StripEnd::Edited => self.tl_commit(ui, None),
                    crate::video::StripEnd::Click(f) => self.vid_seek(ui, f),
                    crate::video::StripEnd::Nothing => self.sync_video(ui),
                }
            }
        }
    }

    /// The wheel over the timeline: Ctrl — zoom about the pointer, else pan.
    pub fn tl_wheel(&mut self, ui: &AppWindow, x: i32, dy: f32, ctrl: bool) {
        let Some(v) = self.s.as_mut().and_then(|s| s.vid.as_mut()) else {
            return;
        };
        let notches = (dy / 40.0) as f64;
        if ctrl {
            v.view
                .zoom_at(x, notches, znimok_video::edit::MAX_PX_PER_FRAME);
        } else {
            v.view.pan(-notches);
        }
        self.sync_video(ui);
    }

    /// The timeline's actions (buttons and keys): cut, keep only, split, in, out, restore,
    /// reset, the frame as a screenshot, the sound tracks.
    pub fn vid_action(&mut self, ui: &AppWindow, what: &str, arg: i32) {
        let Some(v) = self.s.as_mut().and_then(|s| s.vid.as_mut()) else {
            return;
        };
        let frame = v.frame;
        let changed = match what {
            "cut" => v.tl.cut_selection(),
            "keep" => v.keep_only(),
            "split" => v.tl.split(frame),
            "in" => v.tl.set_in(frame),
            "out" => v.tl.set_out(frame),
            // The «Повернути» pill sits on the piece: its x names the piece.
            "restore" => {
                let f = v.frame_at_x(arg);
                v.restore_at(f)
            }
            "reset" => {
                let n = v.frames;
                v.tl = znimok_video::edit::EditTimeline::with_edit(
                    znimok_video::edit::VideoEdit::new(n),
                );
                true
            }
            "deselect" => {
                v.tl.clear_selection();
                self.sync_video(ui);
                return;
            }
            "frame-shot" => {
                self.frame_as_shot(ui);
                return;
            }
            "sound" => {
                self.set_sound_mode(ui, arg);
                return;
            }
            "track-mute" => {
                self.set_track(ui, arg as usize, None, Some(true));
                return;
            }
            "track-unmute" => {
                self.set_track(ui, arg as usize, None, Some(false));
                return;
            }
            _ => false,
        };
        if changed {
            self.tl_commit(ui, None);
        } else {
            self.sync_video(ui);
        }
    }

    /// No sound / system / microphone / both (ZK-189): the tracks' `muted` flags.
    fn set_sound_mode(&mut self, ui: &AppWindow, mode: i32) {
        use znimok_format::video::AudioSource;
        let Some(s) = self.s.as_mut() else { return };
        let Some(part) = s.video.as_mut() else { return };
        for t in &mut part.video.audio {
            t.muted = match t.source {
                AudioSource::System => mode & 1 == 0,
                AudioSource::Microphone => mode & 2 == 0,
            };
        }
        s.changed_at = Instant::now();
        self.sync_video(ui);
    }

    fn set_track(&mut self, ui: &AppWindow, i: usize, volume: Option<u8>, muted: Option<bool>) {
        let Some(s) = self.s.as_mut() else { return };
        let Some(t) = s.video.as_mut().and_then(|p| p.video.audio.get_mut(i)) else {
            return;
        };
        if let Some(vol) = volume {
            t.volume = vol;
        }
        if let Some(m) = muted {
            t.muted = m;
        }
        s.changed_at = Instant::now();
        self.sync_video(ui);
    }

    /// «Кадр як знімок»: the frame on the canvas, full size, as a new document — in a window of
    /// its own (ZK-107). The marks come with ZK-94.
    fn frame_as_shot(&mut self, ui: &AppWindow) {
        let Some(s) = self.s.as_ref() else { return };
        let Some(v) = s.vid.as_ref() else { return };
        let raster = v
            .raster
            .as_ref()
            .map(|r| (**r).clone())
            .unwrap_or_else(|| s.ed.doc.source().clone());
        let mut a = count_args("n", v.frame);
        a.set("name", s.ed.doc.name.clone());
        let name = self.tr.tr_args("vid-frame-doc-name", &a);
        self.new_document(ui, raster, "video-frame", Some(name));
    }

    /// Keys of the video mode. Space and the transport work everywhere in the editor; I / O / S /
    /// Delete act on the timeline only while it has the keys (the last press was on it), so the
    /// tools keep their letters on the canvas.
    fn video_key(
        &mut self,
        ui: &AppWindow,
        text: &str,
        latin: Option<char>,
        shift: bool,
    ) -> Option<KeyAction> {
        let (focus, has_sel, has_range) = {
            let v = self.s.as_ref()?.vid.as_ref()?;
            (
                v.focus,
                !self.selection().is_empty(),
                v.selected_range().is_some(),
            )
        };
        let done = |a: &mut App| {
            a.sync(ui);
            ui.window().request_redraw();
            Some(KeyAction::None)
        };
        match text {
            " " => {
                self.vid_transport(ui, "play");
                return done(self);
            }
            "\u{F702}" if !has_sel => {
                self.vid_transport(ui, if shift { "sec-back" } else { "back" });
                return done(self);
            }
            "\u{F703}" if !has_sel => {
                self.vid_transport(ui, if shift { "sec-fwd" } else { "fwd" });
                return done(self);
            }
            "\u{F729}" => {
                self.vid_transport(ui, "home");
                return done(self);
            }
            "\u{F72B}" => {
                self.vid_transport(ui, "end");
                return done(self);
            }
            "\u{7f}" | "\u{8}" if focus && has_range => {
                self.vid_action(ui, if shift { "keep" } else { "cut" }, 0);
                return done(self);
            }
            "\u{1b}" if focus && has_range => {
                self.vid_action(ui, "deselect", 0);
                return done(self);
            }
            _ => {}
        }
        if focus {
            match latin {
                Some('i') => {
                    self.vid_action(ui, "in", 0);
                    return done(self);
                }
                Some('o') => {
                    self.vid_action(ui, "out", 0);
                    return done(self);
                }
                Some('s') => {
                    self.vid_action(ui, "split", 0);
                    return done(self);
                }
                _ => {}
            }
        }
        if latin == Some('m') {
            self.vid_transport(ui, "mute");
            return done(self);
        }
        None
    }
}
