//! The editor's «Відео» mode (ZK-181): a player that decodes the recording on its own thread
//! and hands frames to the canvas, the timeline's state (the edit list of `znimok-video`, the
//! selection, the drag in progress, the view's zoom and offset) and what the timeline shows.
//!
//! The frames come from `znimok-play` (ZK-92): decoded and converted on the GPU into a texture
//! the window shows under a transparent canvas — the marks are drawn over it, and the canvas is
//! not repainted for a new frame. A CPU copy of the paused frame comes a moment later (Hide marks
//! sample it, «Кадр як знімок» takes it). The film strip's thumbnails come from the same player
//! crate, averaged down on the GPU.
//!
//! The edits live in the document's timeline (ZK-144): every finished edit is one
//! `SetTimeline` command of the core editor, so the marks and the cuts share one undo history.
//! `EditTimeline` here only holds what is being dragged or selected.

use std::collections::HashMap;
use std::sync::Arc;

use std::collections::BTreeMap;

use znimok_core::{Object, ObjectId, Raster, Rgb, Timeline};
use znimok_video::edit::{EditTimeline, Selection, TimelineView, VideoEdit};

/// What the timeline draws, in pixels of its track area (Slint positions it).
#[derive(Clone, Debug, Default)]
pub struct Geometry {
    pub ticks: Vec<(i32, String)>,
    /// Pieces of the film strip: (x, w, kind) — 0 cut out, 1 outside in/out, 2 the selected
    /// part, 3 the selected range.
    pub pieces: Vec<(i32, i32, i32)>,
    pub in_x: i32,
    pub out_x: i32,
    pub playhead_x: i32,
    /// Frames per pixel at this zoom (for the wheel).
    pub px_per_frame: f64,
}

/// The state of one open video, next to the document's session.
pub struct Vid {
    pub fps: f64,
    pub frames: i64,
    /// The player (started once the window has its GPU device).
    pub player: Option<znimok_play::Player>,
    pub player_started: bool,
    /// Why there is no player.
    pub player_error: Option<String>,
    /// How the frames reach the screen ("gpu", "upload", "software").
    pub path: Option<&'static str>,
    /// The frame shown, and its texture on the GPU (None: the poster, drawn by the canvas).
    pub frame: i64,
    pub shown: Option<znimok_play::Shown>,
    /// A CPU copy of a paused frame: (frame, pixels).
    pub raster: Option<(i64, Arc<Raster>)>,
    /// «Кадр як знімок» waits for the CPU copy of the frame shown.
    pub shot_pending: bool,
    pub playing: bool,
    pub backward: bool,
    /// The tone table last sent to the player (None: not sent yet).
    pub tone_sent: Option<Option<[u8; 256]>>,
    /// The film strip: thumbnails by frame, the worker making the missing ones, and what the
    /// strip was last composed from (so it is composed again only when that changes).
    pub thumbs: HashMap<i64, Raster>,
    pub thumbs_job: Option<znimok_play::Thumbs>,
    pub strip_key: Option<(i32, i64, u64, usize)>,
    /// A press on a mark's bar (ZK-94): the mark, what is dragged, where it started, its span then.
    pub mark_press: Option<MarkPress>,
    /// The marks live on the frame last shown (a new set repaints the canvas).
    pub live_sig: u64,
    pub speed_i: usize,
    pub looping: bool,
    pub muted: bool,
    /// The edit list as the timeline works on it (the document's timeline is the truth).
    pub tl: EditTimeline,
    pub view: TimelineView,
    /// Width of the track area as Slint last reported it.
    pub track_w: i32,
    /// A press on the strip: the frame and the x it started at, and whether it moved.
    press: Option<(i64, i32, bool)>,
    pub scrubbing: bool,
    /// The last pointer press was on the timeline: its keys (I / O / S / Del) act on it.
    pub focus: bool,
}

pub const SPEEDS: [f64; 4] = [0.5, 1.0, 1.5, 2.0];

impl Vid {
    pub fn new(fps: f64, frames: i64, timeline: Option<&Timeline>) -> Self {
        let edit = timeline
            .and_then(VideoEdit::from_timeline)
            .unwrap_or_else(|| VideoEdit::new(frames));
        let dur = frames as f64 / fps.max(1e-6);
        Self {
            fps,
            frames,
            player: None,
            player_started: false,
            player_error: None,
            path: None,
            frame: 0,
            shown: None,
            raster: None,
            shot_pending: false,
            playing: false,
            backward: false,
            tone_sent: None,
            thumbs: HashMap::new(),
            thumbs_job: None,
            strip_key: None,
            mark_press: None,
            live_sig: 0,
            speed_i: 1,
            looping: false,
            muted: false,
            tl: EditTimeline::with_edit(edit),
            view: TimelineView::new(dur, fps, 0, 900),
            track_w: 900,
            press: None,
            scrubbing: false,
            focus: false,
        }
    }

    pub fn speed(&self) -> f64 {
        SPEEDS[self.speed_i.min(SPEEDS.len() - 1)]
    }

    pub fn edit(&self) -> &VideoEdit {
        self.tl.edit()
    }

    /// The document's timeline changed under us (undo, redo, a load): the timeline follows.
    pub fn adopt(&mut self, t: &Timeline) {
        let mine = self.tl.edit().to_timeline();
        if (&mine.parts, mine.in_point, mine.out_point) != (&t.parts, t.in_point, t.out_point)
            && let Some(e) = VideoEdit::from_timeline(t)
        {
            self.tl = EditTimeline::with_edit(e);
            if let Some(p) = &self.player {
                p.set_edit(self.tl.edit().clone());
            }
        }
    }

    pub fn set_track_width(&mut self, w: i32) {
        let w = w.max(1);
        if w != self.track_w {
            self.track_w = w;
            self.view.width = w;
            self.view.left = 0;
        }
    }

    /// `m:ss.ff` of a frame.
    pub fn time_str(&self, frame: i64) -> String {
        fmt_time(frame as f64 / self.fps.max(1e-6))
    }

    /// Frames kept after the edits.
    pub fn kept(&self) -> i64 {
        self.edit().kept_frames()
    }

    // ------------------------------------------------------------------ the strip

    pub fn frame_at_x(&self, x: i32) -> i64 {
        (self.view.x_to_time(x) * self.fps)
            .floor()
            .clamp(0.0, (self.frames - 1).max(0) as f64) as i64
    }

    /// A press on the strip at `x`: a handle near it, else the start of a range or a click.
    /// Returns true when a trim handle was taken.
    pub fn strip_press(&mut self, x: i32) -> bool {
        let grab = znimok_video::edit::HANDLE_GRAB_PX;
        if let Some(h) = self.view.handle_at(x, self.tl.edit(), grab) {
            self.tl.begin_handle(h);
            return true;
        }
        self.press = Some((self.frame_at_x(x), x, false));
        false
    }

    /// The pointer moved with the button down over the strip. Returns the frame to show while a
    /// handle is dragged.
    pub fn strip_move(&mut self, x: i32) -> Option<i64> {
        if self.tl.dragging().is_some() {
            let edge = self.view.x_to_edge(x);
            return self.tl.drag_handle(edge);
        }
        let f = self.frame_at_x(x);
        if let Some((f0, x0, moved)) = self.press.as_mut() {
            if !*moved && (x - *x0).abs() < 4 {
                return None;
            }
            *moved = true;
            let a = *f0;
            self.tl.select_range(a, f);
        }
        None
    }

    /// The button went up over the strip: a finished handle drag (true = the edit changed), a
    /// range (kept selected), or a click (the part under it is selected, the frame shown).
    pub fn strip_release(&mut self) -> StripEnd {
        if self.tl.dragging().is_some() {
            return if self.tl.end_handle() {
                StripEnd::Edited
            } else {
                StripEnd::Nothing
            };
        }
        match self.press.take() {
            Some((_, _, true)) => StripEnd::Nothing,
            Some((f, _, false)) => {
                self.tl.click_part(f);
                StripEnd::Click(f)
            }
            None => StripEnd::Nothing,
        }
    }

    /// The range selected on the strip, frames `[a, b)`.
    pub fn selected_range(&self) -> Option<(i64, i64)> {
        match self.tl.selection()? {
            Selection::Range(a, b) => Some((a, b)),
            Selection::Part(i) => self.edit().parts().get(i).map(|p| (p.a, p.b)),
        }
    }

    /// «Лишити лише це»: everything outside the selected range goes.
    pub fn keep_only(&mut self) -> bool {
        let Some((a, b)) = self.selected_range() else {
            return false;
        };
        let n = self.frames;
        let mut changed = false;
        if a > 0 {
            changed |= self.tl.cut_range(0, a);
        }
        if b < n {
            changed |= self.tl.cut_range(b, n);
        }
        self.tl.clear_selection();
        changed
    }

    /// The cut-out part that contains `f` comes back.
    pub fn restore_at(&mut self, f: i64) -> bool {
        let Some(i) = self.edit().part_at(f) else {
            return false;
        };
        if !self.edit().parts()[i].off {
            return false;
        }
        self.tl.click_part(f);
        let done = self.tl.cut_selection();
        self.tl.clear_selection();
        done
    }

    // ------------------------------------------------------------------ playback helpers

    /// One frame (or `n`) forward or back, never onto a cut (`Playback::step`).
    pub fn step(&self, n: i64) -> i64 {
        let p = znimok_video::edit::Playback {
            edit: self.tl.edit(),
            fps: self.fps,
            frames: self.frames,
            looping: self.looping,
        };
        p.step(self.frame, n)
    }

    pub fn home(&self) -> i64 {
        self.edit().in_point()
    }

    pub fn end(&self) -> i64 {
        (self.edit().out_point() - 1).max(0)
    }

    // ------------------------------------------------------------------ what to draw

    pub fn geometry(&mut self) -> Geometry {
        let mut g = Geometry::default();
        let n = self.frames.max(1);
        let vis = self.view.visible().max(1e-6);
        g.px_per_frame = self.view.width as f64 / (vis * self.fps.max(1e-6));
        // Ticks: a label at least 64 px apart, on a round number of seconds.
        let px_per_s = self.view.width as f64 / vis;
        let step = [1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0]
            .into_iter()
            .find(|s| s * px_per_s >= 64.0)
            .unwrap_or(600.0);
        let first = (self.view.off / step).floor() * step;
        let mut t = first;
        while t <= self.view.off + vis + step {
            if t >= 0.0 && t <= self.view.dur + 1e-9 {
                g.ticks.push((self.view.time_to_x(t), fmt_time_short(t)));
            }
            t += step;
        }
        let e = self.tl.edit();
        let x_of = |f: i64| self.view.edge_to_x(f);
        // Outside in / out: dimmed.
        if e.in_point() > 0 {
            g.pieces.push((x_of(0), x_of(e.in_point()) - x_of(0), 1));
        }
        if e.out_point() < n {
            g.pieces
                .push((x_of(e.out_point()), x_of(n) - x_of(e.out_point()), 1));
        }
        for (i, p) in e.parts().iter().enumerate() {
            if p.off {
                g.pieces.push((x_of(p.a), x_of(p.b) - x_of(p.a), 0));
            }
            if self.tl.selection() == Some(Selection::Part(i)) {
                g.pieces.push((x_of(p.a), x_of(p.b) - x_of(p.a), 2));
            }
        }
        if let Some(Selection::Range(a, b)) = self.tl.selection() {
            g.pieces.push((x_of(a), x_of(b) - x_of(a), 3));
        }
        g.in_x = x_of(e.in_point());
        g.out_x = x_of(e.out_point());
        g.playhead_x = self
            .view
            .time_to_x((self.frame as f64 + 0.5) / self.fps.max(1e-6));
        g
    }
}

pub enum StripEnd {
    Nothing,
    Edited,
    Click(i64),
}

/// `m:ss.ff`.
pub fn fmt_time(secs: f64) -> String {
    let secs = secs.max(0.0);
    let m = (secs / 60.0).floor() as i64;
    let s = secs - m as f64 * 60.0;
    format!("{m}:{s:05.2}")
}

/// `m:ss` for the ruler.
pub fn fmt_time_short(secs: f64) -> String {
    let secs = secs.max(0.0).round() as i64;
    format!("{}:{:02}", secs / 60, secs % 60)
}

// ------------------------------------------------------------------ marks in time (ZK-94)

/// A new mark shows this long from the frame it was made on (LH `kEvMarkSec`).
pub const MARK_SECONDS: f64 = 3.0;
/// Lanes of the marks track; more overlapping marks share the last one (LH: up to 4 visible).
pub const LANES: usize = 4;
/// Where the lanes are in the track area, pixels: the first one's top, a lane's step and height.
pub const LANE_TOP: i32 = 106;
pub const LANE_STEP: i32 = 12;
pub const LANE_H: i32 = 10;
/// How close to a bar's end a press takes that end, pixels.
const GRIP_PX: i32 = 6;

/// A mark's bar on the marks track.
#[derive(Clone, Debug, PartialEq)]
pub struct MarkBar {
    pub id: ObjectId,
    pub x: i32,
    pub w: i32,
    pub lane: usize,
    pub colour: Rgb,
    pub selected: bool,
}

/// What a press on a bar drags.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grip {
    Move,
    Start,
    End,
}

#[derive(Clone, Copy, Debug)]
pub struct MarkPress {
    pub id: ObjectId,
    pub grip: Grip,
    pub x0: i32,
    pub span0: (i64, i64),
    pub moved: bool,
    /// The drag's undo step: every move of one drag merges into it.
    pub step: u64,
}

impl Vid {
    /// The span a new mark gets: from the frame shown, three seconds (inside the video).
    pub fn new_span(&self) -> (i64, i64) {
        let n = self.frames.max(1);
        let a = self.frame.clamp(0, n - 1);
        let b = (a + (MARK_SECONDS * self.fps).round() as i64).clamp(a + 1, n);
        (a, b)
    }

    /// The bars of the marks with a span, in lanes that do not overlap (the first free lane by
    /// start; the last lane takes what does not fit).
    pub fn mark_bars(
        &self,
        spans: &BTreeMap<ObjectId, (i64, i64)>,
        objects: &[Object],
        selected: &[ObjectId],
    ) -> Vec<MarkBar> {
        let mut items: Vec<(i64, i64, &Object)> = objects
            .iter()
            .filter_map(|o| spans.get(&o.id).map(|&(a, b)| (a, b, o)))
            .collect();
        items.sort_by_key(|(a, b, o)| (*a, *b, o.id));
        let mut ends = [i64::MIN; LANES];
        items
            .into_iter()
            .map(|(a, b, o)| {
                let lane = (0..LANES).find(|l| ends[*l] <= a).unwrap_or(LANES - 1);
                ends[lane] = ends[lane].max(b);
                let x = self.view.edge_to_x(a);
                MarkBar {
                    id: o.id,
                    x,
                    w: (self.view.edge_to_x(b) - x).max(2),
                    lane,
                    colour: o.style.color,
                    selected: selected.contains(&o.id),
                }
            })
            .collect()
    }

    /// The bar under a point of the track area and what a press there drags.
    pub fn bar_at(bars: &[MarkBar], x: i32, y: i32) -> Option<(ObjectId, Grip)> {
        let lane = ((y - LANE_TOP + (LANE_STEP - LANE_H) / 2) / LANE_STEP).clamp(0, LANES as i32 - 1)
            as usize;
        // The top one first (drawn last).
        bars.iter()
            .rev()
            .find(|b| b.lane == lane && x >= b.x - GRIP_PX / 2 && x <= b.x + b.w + GRIP_PX / 2)
            .map(|b| {
                let grip = if b.w > 3 * GRIP_PX && x <= b.x + GRIP_PX {
                    Grip::Start
                } else if b.w > 3 * GRIP_PX && x >= b.x + b.w - GRIP_PX {
                    Grip::End
                } else {
                    Grip::Move
                };
                (b.id, grip)
            })
    }

    /// The span while a bar is dragged to `x`.
    pub fn dragged_span(&self, p: &MarkPress, x: i32) -> (i64, i64) {
        let n = self.frames.max(1);
        let f0 = self.view.x_to_edge(p.x0);
        let df = self.view.x_to_edge(x) - f0;
        let (a, b) = p.span0;
        match p.grip {
            Grip::Move => {
                let len = b - a;
                let a = (a + df).clamp(0, n - len);
                (a, a + len)
            }
            Grip::Start => ((a + df).clamp(0, b - 1), b),
            Grip::End => (a, (b + df).clamp(a + 1, n)),
        }
    }
}

// ------------------------------------------------------------------ the film strip

impl Vid {
    /// Frames between two key frames — the thumbnails are taken at key frames (fast to decode).
    pub fn key_step(&self) -> i64 {
        ((self.fps / 4.0).round() as i64).max(1)
    }

    /// The tiles of the film strip at this zoom: (x in track pixels, the frame shown there).
    /// `tile_w` is a thumbnail's width in track pixels.
    pub fn strip_tiles(&self, tile_w: i32) -> Vec<(i32, i64)> {
        let tile_w = tile_w.max(8);
        let k = self.key_step();
        let mut out = Vec::new();
        let mut x = 0;
        while x < self.view.width {
            let f = self.frame_at_x(x + tile_w / 2);
            out.push((x, (f / k) * k));
            x += tile_w;
        }
        out
    }
}

/// NV12 (BT.709, limited range) → RGBA, on the CPU — for a recording's poster frame (the player
/// converts on the GPU).
#[cfg(windows)]
pub fn nv12_to_rgba(f: &znimok_video_win::Nv12Frame) -> Raster {
    let (w, h) = (f.width as usize, f.height as usize);
    let mut rgba = vec![255u8; w * h * 4];
    for y in 0..h {
        let yrow = &f.y[y * w..y * w + w];
        let crow = &f.uv[(y / 2) * w..(y / 2) * w + w];
        let out = &mut rgba[y * w * 4..y * w * 4 + w * 4];
        for x in 0..w {
            let yy = (yrow[x] as i32 - 16).max(0) * 298;
            let cb = crow[x & !1] as i32 - 128;
            let cr = crow[(x & !1) + 1] as i32 - 128;
            let r = (yy + 459 * cr + 128) >> 8;
            let g = (yy - 55 * cb - 136 * cr + 128) >> 8;
            let b = (yy + 541 * cb + 128) >> 8;
            let o = x * 4;
            out[o] = r.clamp(0, 255) as u8;
            out[o + 1] = g.clamp(0, 255) as u8;
            out[o + 2] = b.clamp(0, 255) as u8;
        }
    }
    Raster::new(f.width, f.height, rgba)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_format_as_minutes_seconds() {
        assert_eq!(fmt_time(0.0), "0:00.00");
        assert_eq!(fmt_time(12.4), "0:12.40");
        assert_eq!(fmt_time(65.0), "1:05.00");
        assert_eq!(fmt_time_short(48.0), "0:48");
        assert_eq!(fmt_time_short(125.0), "2:05");
    }

    #[test]
    fn keep_only_cuts_both_sides_and_restore_brings_a_part_back() {
        let mut v = Vid::new(30.0, 300, None);
        v.tl.select_range(100, 199);
        assert!(v.keep_only());
        let e = v.edit();
        assert_eq!(e.kept_frames(), 100);
        assert!(!e.is_kept(50) && e.is_kept(150) && !e.is_kept(250));
        assert!(v.restore_at(50));
        assert_eq!(v.edit().kept_frames(), 200);
    }

    #[test]
    fn geometry_places_handles_and_ticks() {
        let mut v = Vid::new(30.0, 30 * 48, None);
        v.set_track_width(924);
        let g = v.geometry();
        assert_eq!(g.in_x, 0);
        assert_eq!(g.out_x, 924);
        assert!(g.ticks.iter().any(|(_, l)| l == "0:00"));
        assert!(g.ticks.len() >= 6, "{:?}", g.ticks);
        assert!(g.pieces.is_empty());
    }
}
