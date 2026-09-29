//! The edit model of the video editor (ZK-93; LH `g_evEd`, CAPS-79/80,
//! `docs/discovery/inventory_video.md` §6.2, the data side of §6.5, §7 items 49, 52, 54).
//!
//! Everything is in **frame numbers**. The parts cover the whole video `[0, N)` without gaps
//! and are only *marked* cut (`off`); a split (`S`) divides a part, `Delete` cuts the selected
//! range or toggles the selected part. Trimming the start and the end is separate — two edges
//! `in`/`out` that are dragged by handles without breaking the splits, and each can be dragged
//! back. What is exported is [`VideoEdit::keep_segs`] = parts that are not `off` ∩ `[in, out)`,
//! adjacent ones merged — exactly the [`KeepSeg`] list [`crate::export`] consumes.
//!
//! Undo is by **snapshots of the whole [`VideoEdit`]** (it is tiny), like LH and like
//! `znimok-core::history`. The editor has ONE undo queue for two systems — timeline edits and
//! video marks (marks live in the screenshot editor, `znimok-core`): [`UndoOrder`] remembers
//! whose step is next (LH `g_evUndoOrder`, CAPS-80), the marks side is reached through the
//! [`MarksUndo`] trait so this crate does not depend on `znimok-core`.
//!
//! - [`VideoEdit`] — the state (`EvEdit`): parts, in/out, and pure queries over it
//!   (keep, kept frame, next/previous kept, the mapping source ↔ edited frame).
//! - [`EditTimeline`] — the state + selection + undo stacks + what was saved: the actions of
//!   §6.2 (split, cut, in/out, handle drags, part click), dirty check.
//! - [`UndoOrder`] — the shared undo/redo queue (`EvUndoAny`/`EvRedoAny`).
//! - [`Playback`] — how playback, steps, Home/End and reverse skip what is cut
//!   (`EvSkipCut`, `EvPlayedToEnd`, `EvStep`, `EvRevStep`, `EvApplyLoop`).
//! - [`TimelineView`] — timeline geometry without drawing: zoom, scroll, x ↔ time, auto-scroll,
//!   the trim handles hit from their current x (§6.5).

use std::collections::VecDeque;

use crate::export::{AudioCut, KeepSeg, kept_frames, map_frame, src_of_out, time_of};

/// Default depth of the timeline undo stack — the owner's "reasonably maximal" 500 of
/// `znimok-core::history` (LH had 200 for the timeline).
pub const DEFAULT_UNDO_DEPTH: usize = 500;

// ---- frames and time ---------------------------------------------------------------------------

/// Frames in a video of `dur` seconds (`EvFrames`: `N = round(dur·fps)`, §6.2); 0 when unknown.
pub fn frame_count(dur: f64, fps: f64) -> i64 {
    if dur > 0.0 && fps > 0.0 {
        (dur * fps + 0.5) as i64
    } else {
        0
    }
}

/// Frame shown at position `t` seconds (`EvFrameIdx`): a hair (1e-4) of tolerance so a position
/// computed as `f/fps` does not fall into the previous frame; clamped to `[0, n)` (0 for an empty
/// video).
pub fn frame_index(t: f64, fps: f64, n: i64) -> i64 {
    let mut i = (t * fps + 1e-4) as i64;
    if i >= n {
        i = n - 1;
    }
    i.max(0)
}

/// Seek position of frame `f` in seconds: a quarter frame inside, so the engine lands exactly on
/// the frame (§6.1, §7 item 49; the 100 ns twin is [`crate::traits::seek_time_for_frame`]).
pub fn seek_pos(f: i64, fps: f64) -> f64 {
    (f as f64 + 0.25) / fps
}

/// Reverse playback aims three quarters into a frame (`EvRevStep`): moving backwards from there
/// the next frame down is reached only after a whole frame of time.
pub fn reverse_pos(f: i64, fps: f64) -> f64 {
    (f as f64 + 0.75) / fps
}

// ---- the state ---------------------------------------------------------------------------------

/// One part of the video, frames `[a, b)`; `off` = cut (`EvPart`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Part {
    pub a: i64,
    pub b: i64,
    pub off: bool,
}

impl Part {
    pub const fn new(a: i64, b: i64, off: bool) -> Self {
        Self { a, b, off }
    }
    pub fn len(&self) -> i64 {
        self.b - self.a
    }
    pub fn is_empty(&self) -> bool {
        self.b <= self.a
    }
    pub fn contains(&self, f: i64) -> bool {
        f >= self.a && f < self.b
    }
}

/// The edits of one video (`EvEdit`). Invariant ([`VideoEdit::is_valid`]): the parts cover
/// `[0, N)` contiguously, none empty; `0 ≤ in < out ≤ N` (both 0 for an empty video).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct VideoEdit {
    parts: Vec<Part>,
    in_point: i64,
    out_point: i64,
}

impl VideoEdit {
    /// A fresh video of `n` frames: one part, nothing cut, `in = 0`, `out = n` (`EvEditReset`).
    pub fn new(n: i64) -> Self {
        let n = n.max(0);
        Self {
            parts: if n > 0 {
                vec![Part::new(0, n, false)]
            } else {
                Vec::new()
            },
            in_point: 0,
            out_point: n,
        }
    }

    /// Restores edits saved in a project (`CUTS` block of `.lhvideo`, LH `LhvApply`) for a video
    /// that has `n` frames now. `None` — the edits are not about these frames (parts do not start
    /// at 0, leave a gap or are empty) and the caller keeps a fresh [`VideoEdit::new`].
    ///
    /// §7 item 49: the duration the engine reports can differ by a frame from the recorded one,
    /// so the tail is fitted: parts starting at or past `n` are dropped (the first one is always
    /// kept), the last part ends at `n`; `out` beyond `n` or not after `in` becomes `n`, a bad
    /// `in` becomes 0.
    pub fn from_saved(parts: &[Part], in_point: i64, out_point: i64, n: i64) -> Option<Self> {
        if parts.is_empty() || parts[0].a != 0 || n <= 0 {
            return None;
        }
        let contiguous = parts
            .windows(2)
            .all(|w| w[1].a == w[0].b && w[1].b > w[1].a);
        if !contiguous {
            return None;
        }
        let mut parts = parts.to_vec();
        while parts.len() > 1 && parts.last().is_some_and(|p| p.a >= n) {
            parts.pop();
        }
        if let Some(last) = parts.last_mut() {
            last.b = n;
        }
        let mut out = out_point;
        if out > n || out <= in_point {
            out = n;
        }
        let mut inp = in_point;
        if inp < 0 || inp >= out {
            inp = 0;
        }
        let e = Self {
            parts,
            in_point: inp,
            out_point: out,
        };
        e.is_valid().then_some(e)
    }

    /// As the document's timeline in `znimok-core`: the editor keeps the edits there, so cutting
    /// and marking share one undo history (ZK-144, variant «б»).
    pub fn to_timeline(&self) -> znimok_core::Timeline {
        znimok_core::Timeline {
            parts: self
                .parts
                .iter()
                .map(|p| znimok_core::TimelinePart {
                    a: p.a,
                    b: p.b,
                    off: p.off,
                })
                .collect(),
            in_point: self.in_point,
            out_point: self.out_point,
        }
    }

    /// From the document's timeline; `None` when it breaks the invariant.
    pub fn from_timeline(t: &znimok_core::Timeline) -> Option<Self> {
        let parts: Vec<Part> = t.parts.iter().map(|p| Part::new(p.a, p.b, p.off)).collect();
        Self::from_saved(&parts, t.in_point, t.out_point, t.frames())
    }

    pub fn parts(&self) -> &[Part] {
        &self.parts
    }

    /// First kept frame edge (`in`).
    pub fn in_point(&self) -> i64 {
        self.in_point
    }

    /// Edge after the last kept frame (`out`, exclusive).
    pub fn out_point(&self) -> i64 {
        self.out_point
    }

    /// Frames the edits cover (`EvEditFrames`) — `N`.
    pub fn frames(&self) -> i64 {
        self.parts.last().map_or(0, |p| p.b)
    }

    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }

    /// The invariant of §6.2: parts cover `[0, N)` contiguously, no empty part, `0 ≤ in < out ≤ N`.
    pub fn is_valid(&self) -> bool {
        if self.parts.is_empty() {
            return self.in_point == 0 && self.out_point == 0;
        }
        self.parts[0].a == 0
            && self.parts.iter().all(|p| !p.is_empty())
            && self.parts.windows(2).all(|w| w[1].a == w[0].b)
            && self.in_point >= 0
            && self.in_point < self.out_point
            && self.out_point <= self.frames()
    }

    /// What is kept, as contiguous segments (`EvKeepSegs`): parts that are not cut ∩ `[in, out)`,
    /// adjacent ones merged. A split alone therefore changes nothing here.
    pub fn keep_segs(&self) -> Vec<KeepSeg> {
        let mut v: Vec<KeepSeg> = Vec::new();
        for p in self.parts.iter().filter(|p| !p.off) {
            let a = p.a.max(self.in_point);
            let b = p.b.min(self.out_point);
            if b <= a {
                continue;
            }
            match v.last_mut() {
                Some(last) if last.b == a => last.b = b,
                _ => v.push(KeepSeg::new(a, b)),
            }
        }
        v
    }

    /// Something is cut or trimmed (`EvEdited`); splits alone do not count.
    pub fn is_edited(&self) -> bool {
        if self.parts.is_empty() {
            return false;
        }
        let k = self.keep_segs();
        !(k.len() == 1 && k[0].a == 0 && k[0].b == self.frames())
    }

    /// Frames of the edited video (`EvKeptFrames`).
    pub fn kept_frames(&self) -> i64 {
        kept_frames(&self.keep_segs())
    }

    /// Duration of the edited video, 100 ns (output frame times are [`time_of`]).
    pub fn kept_duration_hns(&self, fps: f64) -> i64 {
        time_of(self.kept_frames(), fps)
    }

    /// Frame `f` stays in the edited video (`EvKeptFrame`).
    pub fn is_kept(&self, f: i64) -> bool {
        if f < self.in_point || f >= self.out_point {
            return false;
        }
        self.part_at(f).is_some_and(|i| !self.parts[i].off)
    }

    /// First kept frame ≥ `f` (`EvNextKept`).
    pub fn next_kept(&self, f: i64) -> Option<i64> {
        for s in self.keep_segs() {
            if f < s.a {
                return Some(s.a);
            }
            if f < s.b {
                return Some(f);
            }
        }
        None
    }

    /// Last kept frame ≤ `f` (`EvPrevKept`).
    pub fn prev_kept(&self, f: i64) -> Option<i64> {
        for s in self.keep_segs().iter().rev() {
            if f >= s.b {
                return Some(s.b - 1);
            }
            if f >= s.a {
                return Some(f);
            }
        }
        None
    }

    /// Index of the part holding frame `f` (`EvPartAt`).
    pub fn part_at(&self, f: i64) -> Option<usize> {
        self.parts.iter().position(|p| p.contains(f))
    }

    /// Position of source frame `f` in the edited video, `None` when cut (`EvMapFrame`).
    pub fn edited_frame(&self, f: i64) -> Option<i64> {
        map_frame(&self.keep_segs(), f)
    }

    /// Source frame shown at edited frame `out` (`EvSrcOfOut`); past the end — the last kept
    /// frame, nothing kept — 0.
    pub fn source_frame(&self, out: i64) -> i64 {
        src_of_out(&self.keep_segs(), out)
    }

    /// Cutting the audio of an export of these edits by time (§6.3, §7 item 17).
    pub fn audio_cut(&self, fps: f64, rate: i64, channels: usize) -> AudioCut {
        AudioCut::new(&self.keep_segs(), fps, rate, channels, self.frames())
    }

    /// Split before frame `f` — it becomes the first frame of the right part (`EvSplitRaw`).
    /// Returns the index of the right part; `None` — `f` is outside or already a part start.
    fn split_raw(&mut self, f: i64) -> Option<usize> {
        let i = self.part_at(f)?;
        if self.parts[i].a == f {
            return None;
        }
        let mut right = self.parts[i];
        right.a = f;
        self.parts[i].b = f;
        self.parts.insert(i + 1, right);
        Some(i + 1)
    }
}

// ---- undo stack --------------------------------------------------------------------------------

/// Snapshot undo of the timeline (`g_evUndo`/`g_evRedo`): the state *before* a change that did
/// happen is recorded; a new record drops the redo stack; the oldest steps beyond the depth go.
#[derive(Clone, Debug)]
pub struct EditHistory {
    undo: VecDeque<VideoEdit>,
    redo: Vec<VideoEdit>,
    pub max_steps: usize,
}

impl Default for EditHistory {
    fn default() -> Self {
        Self::new(DEFAULT_UNDO_DEPTH)
    }
}

impl EditHistory {
    pub fn new(max_steps: usize) -> Self {
        Self {
            undo: VecDeque::new(),
            redo: Vec::new(),
            max_steps: max_steps.max(1),
        }
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }
    pub fn redo_len(&self) -> usize {
        self.redo.len()
    }
    pub fn record(&mut self, before: VideoEdit) {
        self.undo.push_back(before);
        while self.undo.len() > self.max_steps {
            self.undo.pop_front();
        }
        self.redo.clear();
    }
    pub fn undo(&mut self, current: VideoEdit) -> Option<VideoEdit> {
        let prev = self.undo.pop_back()?;
        self.redo.push(current);
        Some(prev)
    }
    pub fn redo(&mut self, current: VideoEdit) -> Option<VideoEdit> {
        let next = self.redo.pop()?;
        self.undo.push_back(current);
        Some(next)
    }
    pub fn clear_redo(&mut self) {
        self.redo.clear();
    }
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}

// ---- the editing timeline ----------------------------------------------------------------------

/// One of the two trim handles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    In,
    Out,
}

/// What is selected on the film strip: a part (click) or a range (drag), never both.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Selection {
    /// Part index (`g_evSelPart`).
    Part(usize),
    /// Frames `[a, b)` (`g_evSelA/B`).
    Range(i64, i64),
}

/// The edits of the open video with selection, undo and what was saved. The actions return
/// `true` when they recorded an undo step — the caller then notes it in the shared queue with
/// [`UndoOrder::note_cut`].
#[derive(Clone, Debug, Default)]
pub struct EditTimeline {
    edit: VideoEdit,
    history: EditHistory,
    selection: Option<Selection>,
    /// What lies in the saved file (`g_evSavedKeep`).
    saved_keep: Vec<KeepSeg>,
    /// A handle being dragged and the state before the drag.
    drag: Option<(Handle, VideoEdit)>,
}

impl EditTimeline {
    /// A freshly opened video of `n` frames: nothing cut, the saved file keeps everything
    /// (`EvEditReset`).
    pub fn new(n: i64) -> Self {
        let edit = VideoEdit::new(n);
        Self {
            saved_keep: edit.keep_segs(),
            edit,
            ..Self::default()
        }
    }

    /// A project reopened with saved edits (`LhvApply`): they are the saved state too.
    pub fn with_edit(edit: VideoEdit) -> Self {
        Self {
            saved_keep: edit.keep_segs(),
            edit,
            ..Self::default()
        }
    }

    pub fn with_depth(mut self, max_steps: usize) -> Self {
        self.history.max_steps = max_steps.max(1);
        self
    }

    pub fn edit(&self) -> &VideoEdit {
        &self.edit
    }

    pub fn history(&self) -> &EditHistory {
        &self.history
    }

    pub fn selection(&self) -> Option<Selection> {
        self.selection
    }

    /// The selected part is cut — the Delete button reads "restore" (`EvSelIsOff`).
    pub fn selection_is_off(&self) -> bool {
        matches!(self.selection, Some(Selection::Part(i)) if self.edit.parts.get(i).is_some_and(|p| p.off))
    }

    /// The engine learnt the real frame count (`MF_MEDIA_ENGINE_EVENT_LOADEDDATA`): when it
    /// differs from the edits and nothing can be undone yet (`any_undo` — of the whole shared
    /// queue), the edits start afresh. Returns `true` when reset.
    pub fn adopt_frame_count(&mut self, n: i64, any_undo: bool) -> bool {
        if any_undo || self.edit.frames() == n {
            return false;
        }
        *self = Self::new(n).with_depth(self.history.max_steps);
        true
    }

    // -- dirty --

    /// The kept frames differ from the saved file (`EvDirty` without marks and geometry, which
    /// are [`UndoOrder::marks_dirty`] and the geometry's own). Splits alone are not dirty.
    pub fn cuts_dirty(&self) -> bool {
        !self.edit.is_empty() && self.edit.keep_segs() != self.saved_keep
    }

    /// The current edits were saved (`g_evSavedKeep = EvKeepSegs()`).
    pub fn mark_saved(&mut self) {
        self.saved_keep = self.edit.keep_segs();
    }

    /// The saved file keeps `keep` (a save job finished with the edits it started with — the
    /// user may have edited further meanwhile).
    pub fn mark_saved_keep(&mut self, keep: Vec<KeepSeg>) {
        self.saved_keep = keep;
    }

    pub fn saved_keep(&self) -> &[KeepSeg] {
        &self.saved_keep
    }

    // -- actions --

    fn commit(&mut self, before: VideoEdit) -> bool {
        debug_assert!(self.edit.is_valid(), "{:?}", self.edit);
        if before == self.edit {
            return false;
        }
        self.history.record(before);
        true
    }

    /// `S`: split before frame `f` (the current one) — it becomes the first frame of the right
    /// part, and the right part is selected: that is the one usually cut next (`EvSplitHere`).
    pub fn split(&mut self, f: i64) -> bool {
        let before = self.edit.clone();
        let Some(right) = self.edit.split_raw(f) else {
            return false;
        };
        self.selection = Some(Selection::Part(right));
        self.commit(before)
    }

    /// `Delete` (`EvCutSel`): a selected range is split on both sides and cut, the selection is
    /// cleared; a selected part toggles cut ↔ kept and stays selected.
    ///
    /// Unlike LH a range already cut does not leave an empty undo step (as everywhere in
    /// `znimok-core::history`).
    pub fn cut_selection(&mut self) -> bool {
        match self.selection {
            Some(Selection::Range(a, b)) if b > a => {
                let before = self.edit.clone();
                self.edit.split_raw(a);
                self.edit.split_raw(b);
                for p in &mut self.edit.parts {
                    if p.a >= a && p.b <= b {
                        p.off = true;
                    }
                }
                self.selection = None;
                self.commit(before)
            }
            Some(Selection::Part(i)) if i < self.edit.parts.len() => {
                let before = self.edit.clone();
                self.edit.parts[i].off = !self.edit.parts[i].off;
                self.commit(before)
            }
            _ => false,
        }
    }

    /// Cut frames `[a, b)` directly (select the range + `Delete`).
    pub fn cut_range(&mut self, a: i64, b: i64) -> bool {
        self.selection = Some(Selection::Range(a, b));
        let done = self.cut_selection();
        self.selection = None;
        done
    }

    /// `I`: the start is the current frame `f` (`EvSetIn`); outside `[0, out)` or unchanged —
    /// nothing.
    pub fn set_in(&mut self, f: i64) -> bool {
        if f < 0 || f >= self.edit.out_point || f == self.edit.in_point {
            return false;
        }
        let before = self.edit.clone();
        self.edit.in_point = f;
        self.commit(before)
    }

    /// `O`: the end is AFTER the current frame `f` — the current frame stays (`EvSetOut`).
    pub fn set_out(&mut self, f: i64) -> bool {
        let o = f + 1;
        if o <= self.edit.in_point || o > self.edit.frames() || o == self.edit.out_point {
            return false;
        }
        let before = self.edit.clone();
        self.edit.out_point = o;
        self.commit(before)
    }

    /// `Esc`: clears the selection first; `false` — nothing to clear, the editor closes as usual
    /// (`EvClearSel`).
    pub fn clear_selection(&mut self) -> bool {
        self.selection.take().is_some()
    }

    /// A click on the strip at frame `f` selects its part — except the single untouched part:
    /// there is nothing to select then (`EvEditDragEnd`).
    pub fn click_part(&mut self, f: i64) {
        self.selection = self
            .edit
            .part_at(f)
            .filter(|&i| self.edit.parts.len() > 1 || self.edit.parts[i].off)
            .map(Selection::Part);
    }

    /// A drag on the strip from frame `from` to frame `to` selects both of them and everything
    /// between: `[min, max + 1)` (`EvEditDrag`).
    pub fn select_range(&mut self, from: i64, to: i64) {
        self.selection = Some(Selection::Range(from.min(to), from.max(to) + 1));
    }

    /// A trim handle is grabbed (`EvEditClick`, kinds 3/4).
    pub fn begin_handle(&mut self, h: Handle) {
        if !self.edit.is_empty() {
            self.drag = Some((h, self.edit.clone()));
        }
    }

    pub fn dragging(&self) -> Option<Handle> {
        self.drag.as_ref().map(|(h, _)| *h)
    }

    /// The handle moved to frame edge `edge` (`EvXToEdge`, a boundary between frames). `in` stays
    /// in `[0, out − 1]`, `out` in `[in + 1, N]`. Returns the frame to show: the new first frame,
    /// or the last one before the new end.
    pub fn drag_handle(&mut self, edge: i64) -> Option<i64> {
        let (h, _) = self.drag.as_ref()?;
        match h {
            Handle::In => {
                let v = edge.min(self.edit.out_point - 1).max(0);
                self.edit.in_point = v;
                Some(v)
            }
            Handle::Out => {
                let v = edge.max(self.edit.in_point + 1).min(self.edit.frames());
                self.edit.out_point = v;
                Some(v - 1)
            }
        }
    }

    /// The handle is released: a step is recorded only if the edge really moved — an empty move
    /// leaves no undo record (§7 item 52).
    ///
    /// LH pushed the step on grab and popped it on an empty release but left its entry in the
    /// shared order queue (unlike the marks drag, which popped both), so a later undo could take
    /// a cut step out of turn; recording on release avoids the orphan entry altogether.
    pub fn end_handle(&mut self) -> bool {
        match self.drag.take() {
            Some((_, before)) => self.commit(before),
            None => false,
        }
    }

    // -- undo --

    /// Undo one timeline step (`EvUndo`); the selection is cleared. Call through
    /// [`UndoOrder::undo_any`] to respect the shared queue.
    pub fn undo(&mut self) -> bool {
        self.drag = None;
        let Some(prev) = self.history.undo(self.edit.clone()) else {
            return false;
        };
        self.edit = prev;
        self.selection = None;
        true
    }

    /// Redo one timeline step (`EvRedo`).
    pub fn redo(&mut self) -> bool {
        self.drag = None;
        let Some(next) = self.history.redo(self.edit.clone()) else {
            return false;
        };
        self.edit = next;
        self.selection = None;
        true
    }
}

// ---- the shared undo queue ---------------------------------------------------------------------

/// Whose step it is in the shared queue.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Track {
    /// Timeline edits ([`EditTimeline`]).
    Cuts,
    /// Video marks (the screenshot editor's history in `znimok-core`).
    Marks,
}

/// The marks' own undo stack as the shared queue needs it — implemented by the app over
/// `znimok-core`'s editor (its `Undo`/`Redo` commands and its history).
pub trait MarksUndo {
    fn can_undo(&self) -> bool;
    fn can_redo(&self) -> bool;
    fn undo(&mut self) -> bool;
    fn redo(&mut self) -> bool;
    /// A new timeline step makes the marks' redo invalid (`EvNoteCutUndo` clears `g_edRedo`).
    fn clear_redo(&mut self);
}

/// Default depth of the order queue: both stacks at full depth.
pub const DEFAULT_ORDER_DEPTH: usize = 2 * DEFAULT_UNDO_DEPTH;

/// One undo queue for two systems (CAPS-80, `g_evUndoOrder`/`g_evRedoOrder`): the order of the
/// steps says whose stack undoes next. An entry whose stack is already empty (trimmed by its
/// depth) is skipped. Also counts changes of the marks for the dirty check: an undo or redo of
/// marks after a save leaves the project dirty on purpose (§7 item 54).
#[derive(Clone, Debug)]
pub struct UndoOrder {
    undo: VecDeque<Track>,
    redo: Vec<Track>,
    pub max_steps: usize,
    marks_gen: u64,
    marks_saved_gen: u64,
}

impl Default for UndoOrder {
    fn default() -> Self {
        Self {
            undo: VecDeque::new(),
            redo: Vec::new(),
            max_steps: DEFAULT_ORDER_DEPTH,
            marks_gen: 0,
            marks_saved_gen: 0,
        }
    }
}

impl UndoOrder {
    pub fn new() -> Self {
        Self::default()
    }

    fn push(&mut self, t: Track) {
        self.undo.push_back(t);
        while self.undo.len() > self.max_steps {
            self.undo.pop_front();
        }
        self.redo.clear();
    }

    /// A timeline step was recorded (`EvNoteCutUndo`): the marks' redo is dropped.
    pub fn note_cut(&mut self, marks: &mut impl MarksUndo) {
        self.push(Track::Cuts);
        marks.clear_redo();
    }

    /// A marks step was recorded (`EvNoteMarkUndo`): the timeline's redo is dropped and the marks
    /// changed.
    pub fn note_mark(&mut self, cuts: &mut EditTimeline) {
        self.push(Track::Marks);
        cuts.history.clear_redo();
        self.marks_gen += 1;
    }

    /// A marks drag that moved nothing took its step back (`EvMarksDragEnd`): the last entry
    /// goes too, and the marks did not change.
    pub fn forget_last_mark(&mut self) {
        if self.undo.back() == Some(&Track::Marks) {
            self.undo.pop_back();
            self.marks_gen = self.marks_gen.saturating_sub(1);
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Undo the newest step of either system (`EvUndoAny`).
    pub fn undo_any(
        &mut self,
        cuts: &mut EditTimeline,
        marks: &mut impl MarksUndo,
    ) -> Option<Track> {
        while let Some(t) = self.undo.pop_back() {
            let done = match t {
                Track::Cuts => cuts.history.can_undo() && cuts.undo(),
                Track::Marks => marks.can_undo() && marks.undo(),
            };
            if done {
                if t == Track::Marks {
                    self.marks_gen += 1;
                }
                self.redo.push(t);
                return Some(t);
            }
        }
        None
    }

    /// Redo the newest undone step of either system (`EvRedoAny`).
    pub fn redo_any(
        &mut self,
        cuts: &mut EditTimeline,
        marks: &mut impl MarksUndo,
    ) -> Option<Track> {
        while let Some(t) = self.redo.pop() {
            let done = match t {
                Track::Cuts => cuts.history.can_redo() && cuts.redo(),
                Track::Marks => marks.can_redo() && marks.redo(),
            };
            if done {
                if t == Track::Marks {
                    self.marks_gen += 1;
                }
                self.undo.push_back(t);
                return Some(t);
            }
        }
        None
    }

    /// The marks differ from the saved file (`g_evMarksGen != g_evMarksSavedGen`).
    pub fn marks_dirty(&self) -> bool {
        self.marks_gen != self.marks_saved_gen
    }

    /// Marks changed outside undo (a property edited without a step): dirty.
    pub fn touch_marks(&mut self) {
        self.marks_gen += 1;
    }

    pub fn mark_saved(&mut self) {
        self.marks_saved_gen = self.marks_gen;
    }

    pub fn clear(&mut self) {
        *self = Self {
            max_steps: self.max_steps,
            ..Self::default()
        };
    }
}

// ---- playback over the edits -------------------------------------------------------------------

/// What playback must do next.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PlayAction {
    /// Nothing — the frame is kept.
    Continue,
    /// Seek to this position (seconds) and keep playing.
    Seek(f64),
    /// The kept video is over and the loop is on: seek here and play again.
    Restart(f64),
    /// The kept video is over: pause at this position (the last kept frame), `None` — nothing is
    /// kept at all.
    Stop(Option<f64>),
}

/// Playback, steps and reverse over the edits: what is cut is never played (§6.2 `EvSkipCut`,
/// `EvNextKept/EvPrevKept`); with edits the editor, not the engine, runs the loop.
#[derive(Clone, Copy, Debug)]
pub struct Playback<'a> {
    pub edit: &'a VideoEdit,
    pub fps: f64,
    /// Frames of the video (`EvFrames`, from the engine's duration).
    pub frames: i64,
    pub looping: bool,
}

impl Playback<'_> {
    /// Loop in the engine only without edits: the engine would loop at the end of the file,
    /// together with what is cut (`EvApplyLoop`).
    pub fn engine_loop(&self) -> bool {
        self.looping && !self.edit.is_edited()
    }

    fn last(&self) -> i64 {
        self.frames - 1
    }

    /// Called on every tick while playing (`EvSkipCut`): a cut frame is jumped over to the next
    /// kept one; none left — [`Playback::played_to_end`].
    pub fn skip_cut(&self, pos: f64) -> PlayAction {
        if !self.edit.is_edited() {
            return PlayAction::Continue;
        }
        let f = frame_index(pos, self.fps, self.frames);
        if self.edit.is_kept(f) {
            return PlayAction::Continue;
        }
        match self.edit.next_kept(f) {
            Some(nf) => PlayAction::Seek(seek_pos(nf, self.fps)),
            None => self.played_to_end(),
        }
    }

    /// The end of what is kept (`EvPlayedToEnd`): loop → from the first kept frame, otherwise
    /// pause on the last kept one.
    pub fn played_to_end(&self) -> PlayAction {
        if self.looping
            && let Some(first) = self.edit.next_kept(0)
        {
            return PlayAction::Restart(seek_pos(first, self.fps));
        }
        PlayAction::Stop(
            self.edit
                .prev_kept(self.last())
                .map(|f| seek_pos(f, self.fps)),
        )
    }

    /// `n` frame steps from frame `idx` (`EvStep`): with edits a step jumps over cut frames;
    /// nowhere further to go — stays. Returns the frame to seek to.
    pub fn step(&self, idx: i64, n: i64) -> i64 {
        let last = self.last();
        let edited = self.edit.is_edited();
        let dir = if n < 0 { -1 } else { 1 };
        let mut idx = idx;
        for _ in 0..n.abs() {
            let mut j = idx + dir;
            while j >= 0 && j <= last && edited && !self.edit.is_kept(j) {
                j += dir;
            }
            if j < 0 || j > last {
                break;
            }
            idx = j;
        }
        idx.clamp(0, last.max(0))
    }

    /// `Home` (`EvHome`): the first kept frame.
    pub fn home(&self) -> i64 {
        if self.edit.is_edited() {
            self.edit.next_kept(0).unwrap_or(0)
        } else {
            0
        }
    }

    /// `End` (`EvEnd`): the last kept frame.
    pub fn end(&self) -> i64 {
        let last = self.last();
        if self.edit.is_edited() {
            self.edit.prev_kept(last).unwrap_or(last)
        } else {
            last
        }
    }

    /// Where reverse playback starts from `pos` (`EvToggleReverse`): at the very start — from the
    /// end (`dur`); with edits, at or before the first kept frame — from the last kept one.
    /// `None` — nothing is kept, reverse does not start.
    pub fn reverse_start(&self, pos: f64, dur: f64) -> Option<f64> {
        let mut pos = pos;
        if pos <= 1.0 / self.fps {
            pos = dur;
        }
        if self.edit.is_edited() {
            let first = self.edit.next_kept(0);
            let last = self.edit.prev_kept(self.last())?;
            if first.is_some_and(|first| frame_index(pos, self.fps, self.frames) <= first) {
                pos = reverse_pos(last, self.fps);
            }
        }
        Some(pos)
    }

    /// A reverse tick reached position `t` (`EvRevStep`, after the loop wrap): a cut frame is
    /// jumped over backwards. Returns the new position and whether reverse goes on;
    /// `None` — `t` is fine as it is. When the jump happens the caller restarts its reverse
    /// clock from the new position.
    pub fn reverse_skip(&self, t: f64) -> Option<(f64, bool)> {
        if !self.edit.is_edited() {
            return None;
        }
        let f = frame_index(t, self.fps, self.frames);
        if self.edit.is_kept(f) {
            return None;
        }
        let mut pf = self.edit.prev_kept(f);
        if pf.is_none() && self.looping {
            pf = self.edit.prev_kept(self.last());
        }
        Some(match pf {
            Some(pf) => (reverse_pos(pf, self.fps), true),
            None => (
                seek_pos(self.edit.next_kept(0).unwrap_or(0), self.fps),
                false,
            ),
        })
    }
}

// ---- timeline geometry (§6.5) ------------------------------------------------------------------

/// Timeline geometry without drawing (§6.5): the film strip spans `[left, left + width)` pixels,
/// shows `dur / zoom` seconds from `off`. Zoom is `1 … dur·fps·px_per_frame/width` — the closest
/// is 12 points per frame, beyond that frames cannot be told apart (`EvWheel`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimelineView {
    pub dur: f64,
    pub fps: f64,
    pub left: i32,
    pub width: i32,
    pub zoom: f64,
    pub off: f64,
}

/// Pixels per frame at the closest zoom, at scale 1 (`EdPx(12)`).
pub const MAX_PX_PER_FRAME: f64 = 12.0;
/// Handle grab distance at scale 1 (`EdPx(9)`).
pub const HANDLE_GRAB_PX: i32 = 9;

impl TimelineView {
    pub fn new(dur: f64, fps: f64, left: i32, width: i32) -> Self {
        Self {
            dur,
            fps,
            left,
            width,
            zoom: 1.0,
            off: 0.0,
        }
    }

    /// Seconds visible (`EvVisDur`).
    pub fn visible(&self) -> f64 {
        if self.zoom > 0.0 {
            self.dur / self.zoom
        } else {
            self.dur
        }
    }

    /// Time under `x`, clamped to `[0, dur]` (`EvXToTime`).
    pub fn x_to_time(&self, x: i32) -> f64 {
        if self.width <= 0 {
            return 0.0;
        }
        let t = self.off + (x - self.left) as f64 * self.visible() / self.width as f64;
        t.clamp(0.0, self.dur.max(0.0))
    }

    /// `x` of time `t` (`EvTimeToX`), rounded.
    pub fn time_to_x(&self, t: f64) -> i32 {
        let vis = self.visible();
        // `(int)(v + 0.5)` as in LH: truncation toward zero, so left of the strip it rounds
        // differently — irrelevant, nothing is drawn there.
        self.left
            + if vis > 0.0 {
                ((t - self.off) * self.width as f64 / vis + 0.5) as i32
            } else {
                0
            }
    }

    /// `x` of frame edge `f` (a boundary between frames).
    pub fn edge_to_x(&self, f: i64) -> i32 {
        self.time_to_x(f as f64 / self.fps)
    }

    /// The frame edge nearest to `x` (`EvXToEdge`) — what a trim handle snaps to.
    pub fn x_to_edge(&self, x: i32) -> i64 {
        (self.x_to_time(x) * self.fps + 0.5) as i64
    }

    fn clamp_off(&mut self) {
        let vis = self.visible();
        if self.off > self.dur - vis {
            self.off = self.dur - vis;
        }
        if self.off < 0.0 {
            self.off = 0.0;
        }
    }

    /// Zoom by wheel `notches` (120 = one notch; `×1.25` per notch) around the time under `x`
    /// (`EvWheel`); `px_per_frame` is [`MAX_PX_PER_FRAME`] × the UI scale.
    pub fn zoom_at(&mut self, x: i32, notches: f64, px_per_frame: f64) {
        if self.dur <= 0.0 || self.width <= 0 {
            return;
        }
        let anchor = self.x_to_time(x);
        let zmax = (self.dur * self.fps * px_per_frame / self.width as f64).max(1.0);
        self.zoom = (self.zoom * 1.25f64.powf(notches)).clamp(1.0, zmax);
        self.off = anchor - (x - self.left) as f64 * self.visible() / self.width as f64;
        self.clamp_off();
    }

    /// Scroll by wheel `notches` (Shift or a horizontal wheel): a tenth of the visible span each.
    pub fn pan(&mut self, notches: f64) {
        if self.dur <= 0.0 || self.width <= 0 {
            return;
        }
        self.off -= self.visible() * 0.1 * notches;
        self.clamp_off();
    }

    /// Keep the playhead in view (`EvAutoScroll`): leaving the visible span (or its last 5 %) puts
    /// it a tenth from the left edge.
    pub fn auto_scroll(&mut self, pos: f64) {
        let vis = self.visible();
        if self.zoom <= 1.0 || vis <= 0.0 {
            self.off = 0.0;
            return;
        }
        if pos < self.off || pos > self.off + vis * 0.95 {
            self.off = pos - vis * 0.1;
        }
        self.clamp_off();
    }

    /// The trim handles are not separate hit areas: they move with zoom and scroll, so a press is
    /// tested against their x now (`EvEditClick`). `in` wins a tie. `None` — a plain press on
    /// the strip (seek / range selection / part click).
    pub fn handle_at(&self, x: i32, edit: &VideoEdit, grab_px: i32) -> Option<Handle> {
        if edit.is_empty() {
            return None;
        }
        let di = (x - self.edge_to_x(edit.in_point())).abs();
        let d_out = (x - self.edge_to_x(edit.out_point())).abs();
        if di <= grab_px && di <= d_out {
            Some(Handle::In)
        } else if d_out <= grab_px {
            Some(Handle::Out)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests;
