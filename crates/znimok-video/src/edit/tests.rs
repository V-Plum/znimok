use super::*;

fn parts(e: &VideoEdit) -> Vec<(i64, i64, bool)> {
    e.parts().iter().map(|p| (p.a, p.b, p.off)).collect()
}

fn keep(e: &VideoEdit) -> Vec<(i64, i64)> {
    e.keep_segs().iter().map(|s| (s.a, s.b)).collect()
}

/// §6.2: a fresh video — one part covering `[0, N)`, `in = 0`, `out = N`, nothing edited;
/// `N = round(dur·fps)`.
#[test]
fn fresh_video() {
    assert_eq!(frame_count(10.0, 30.0), 300);
    assert_eq!(frame_count(10.016, 30.0), 300);
    assert_eq!(frame_count(10.017, 30.0), 301);
    assert_eq!(frame_count(0.0, 30.0), 0);
    let e = VideoEdit::new(300);
    assert_eq!(parts(&e), [(0, 300, false)]);
    assert_eq!((e.in_point(), e.out_point(), e.frames()), (0, 300, 300));
    assert!(!e.is_edited());
    assert_eq!(keep(&e), [(0, 300)]);
    assert!(e.is_valid());
    let empty = VideoEdit::new(0);
    assert!(empty.is_empty() && empty.is_valid() && !empty.is_edited());
    assert!(empty.keep_segs().is_empty());
}

/// `EvFrameIdx`: 1e-4 of tolerance, clamped to `[0, n)`.
#[test]
fn frame_index_rules() {
    assert_eq!(frame_index(1.0 / 30.0 * 7.0, 30.0, 300), 7);
    assert_eq!(frame_index(0.0, 30.0, 300), 0);
    assert_eq!(frame_index(-1.0, 30.0, 300), 0);
    assert_eq!(frame_index(100.0, 30.0, 300), 299);
    assert_eq!(frame_index(1.0, 30.0, 0), 0);
    assert!((seek_pos(3, 30.0) - 3.25 / 30.0).abs() < 1e-12);
    assert!((reverse_pos(3, 30.0) - 3.75 / 30.0).abs() < 1e-12);
}

/// §6.2 `S`: the split is before the current frame, which becomes the first frame of the right
/// part; the right part is selected. At a part start — nothing, no undo step.
#[test]
fn split_selects_right_part() {
    let mut t = EditTimeline::new(100);
    assert!(t.split(40));
    assert_eq!(parts(t.edit()), [(0, 40, false), (40, 100, false)]);
    assert_eq!(t.selection(), Some(Selection::Part(1)));
    assert_eq!(t.history().undo_len(), 1);
    assert!(!t.split(40), "already a part start");
    assert!(!t.split(0), "video start");
    assert!(!t.split(100), "outside");
    assert!(!t.split(-1), "outside");
    assert_eq!(t.history().undo_len(), 1);
}

/// §6.2: splits alone change nothing that is kept — not edited, not dirty.
#[test]
fn splits_alone_keep_everything() {
    let mut t = EditTimeline::new(100);
    t.split(10);
    t.split(50);
    t.split(70);
    assert_eq!(keep(t.edit()), [(0, 100)], "adjacent kept parts are merged");
    assert!(!t.edit().is_edited());
    assert!(!t.cuts_dirty());
}

/// §6.2 `Delete` with a range: split on both sides, the range is cut, the selection cleared.
#[test]
fn delete_range() {
    let mut t = EditTimeline::new(100);
    t.select_range(30, 19); // a drag right to left: both ends and all between
    assert_eq!(t.selection(), Some(Selection::Range(19, 31)));
    assert!(t.cut_selection());
    assert_eq!(
        parts(t.edit()),
        [(0, 19, false), (19, 31, true), (31, 100, false)]
    );
    assert_eq!(t.selection(), None);
    assert_eq!(keep(t.edit()), [(0, 19), (31, 100)]);
    assert!(t.edit().is_edited());
    // a range over several parts cuts all of them; the range reaching N splits only once
    assert!(t.cut_range(50, 100));
    assert_eq!(
        parts(t.edit()),
        [
            (0, 19, false),
            (19, 31, true),
            (31, 50, false),
            (50, 100, true)
        ]
    );
    // a range already cut: nothing changes, no empty undo step
    let steps = t.history().undo_len();
    assert!(!t.cut_range(19, 31));
    assert_eq!(t.history().undo_len(), steps);
}

/// §6.2 `Delete` with a selected part: toggles cut ↔ kept, the part stays selected
/// (`EvSelIsOff` then reads "restore").
#[test]
fn delete_toggles_selected_part() {
    let mut t = EditTimeline::new(100);
    t.split(60);
    assert!(t.cut_selection());
    assert!(t.edit().parts()[1].off);
    assert!(t.selection_is_off());
    assert_eq!(keep(t.edit()), [(0, 60)]);
    assert!(t.cut_selection());
    assert!(!t.edit().parts()[1].off);
    assert!(!t.selection_is_off());
    assert_eq!(keep(t.edit()), [(0, 100)]);
    assert!(t.clear_selection());
    assert!(!t.cut_selection(), "nothing selected");
    assert!(
        !t.clear_selection(),
        "Esc: nothing to clear — the editor closes"
    );
}

/// `EvEditDragEnd`: a click selects its part, except the single untouched part.
#[test]
fn click_part_rule() {
    let mut t = EditTimeline::new(100);
    t.click_part(50);
    assert_eq!(
        t.selection(),
        None,
        "single part, no edits — nothing to select"
    );
    t.cut_range(0, 100);
    t.click_part(50);
    assert_eq!(
        t.selection(),
        Some(Selection::Part(0)),
        "a single cut part is selectable"
    );
    let mut t = EditTimeline::new(100);
    t.split(40);
    t.click_part(10);
    assert_eq!(t.selection(), Some(Selection::Part(0)));
    t.click_part(100);
    assert_eq!(t.selection(), None, "outside");
}

/// §6.2 `I`/`O`: the start is the current frame, the end is AFTER the current frame (it stays);
/// outside or unchanged — nothing.
#[test]
fn in_out_rules() {
    let mut t = EditTimeline::new(100);
    assert!(t.set_in(10));
    assert!(t.set_out(79));
    assert_eq!((t.edit().in_point(), t.edit().out_point()), (10, 80));
    assert!(t.edit().is_kept(79) && !t.edit().is_kept(80));
    assert!(t.edit().is_kept(10) && !t.edit().is_kept(9));
    assert!(!t.set_in(10), "unchanged");
    assert!(!t.set_in(80), "not before out");
    assert!(!t.set_in(-1));
    assert!(!t.set_out(79), "unchanged");
    assert!(!t.set_out(9), "out would not be after in");
    assert!(!t.set_out(100), "past N");
    assert!(t.set_out(99));
    assert_eq!(t.edit().out_point(), 100);
    assert_eq!(t.history().undo_len(), 3);
}

/// §6.2: in/out are separate edges and do not break the splits; keep = parts not cut ∩
/// `[in, out)`, adjacent ones merged.
#[test]
fn in_out_do_not_break_splits() {
    let mut t = EditTimeline::new(100);
    t.split(20);
    t.cut_range(40, 50);
    t.set_in(30);
    t.set_out(89);
    assert_eq!(
        parts(t.edit()),
        [
            (0, 20, false),
            (20, 40, false),
            (40, 50, true),
            (50, 100, false)
        ]
    );
    assert_eq!(keep(t.edit()), [(30, 40), (50, 90)]);
    assert_eq!(t.edit().kept_frames(), 50);
    // dragging the start back restores the earlier split untouched
    t.set_in(0);
    assert_eq!(keep(t.edit()), [(0, 40), (50, 90)]);
    // in inside a cut part: keep starts at the next kept part
    t.set_in(45);
    assert_eq!(keep(t.edit()), [(50, 90)]);
    assert!(t.edit().is_edited());
}

/// `EvKeptFrame` / `EvNextKept` / `EvPrevKept` / `EvPartAt`.
#[test]
fn kept_queries() {
    let mut t = EditTimeline::new(100);
    t.cut_range(10, 20);
    t.set_in(5);
    t.set_out(89);
    let e = t.edit();
    assert!(!e.is_kept(4) && e.is_kept(5) && e.is_kept(9) && !e.is_kept(10));
    assert!(!e.is_kept(19) && e.is_kept(20) && e.is_kept(89) && !e.is_kept(90));
    assert_eq!(e.next_kept(0), Some(5));
    assert_eq!(e.next_kept(12), Some(20));
    assert_eq!(e.next_kept(30), Some(30));
    assert_eq!(e.next_kept(90), None);
    assert_eq!(e.prev_kept(99), Some(89));
    assert_eq!(e.prev_kept(15), Some(9));
    assert_eq!(e.prev_kept(7), Some(7));
    assert_eq!(e.prev_kept(4), None);
    assert_eq!(e.part_at(15), Some(1));
    assert_eq!(e.part_at(100), None);
}

/// §6.2: the edited ↔ source mapping (`EvMapFrame`, `EvSrcOfOut`) and the edited duration.
#[test]
fn edited_source_mapping() {
    let mut t = EditTimeline::new(100);
    t.cut_range(10, 20);
    t.set_out(49);
    let e = t.edit();
    assert_eq!(e.kept_frames(), 40);
    assert_eq!(e.edited_frame(9), Some(9));
    assert_eq!(e.edited_frame(15), None);
    assert_eq!(e.edited_frame(20), Some(10));
    assert_eq!(e.edited_frame(50), None);
    assert_eq!(e.source_frame(10), 20);
    assert_eq!(e.source_frame(39), 49);
    assert_eq!(
        e.source_frame(1000),
        49,
        "past the end — the last kept frame"
    );
    assert_eq!(src_of_out(&[], 3), 0);
    assert_eq!(e.kept_duration_hns(30.0), time_of(40, 30.0));
}

/// §6.2 undo by snapshots: undo restores the exact state, redo re-applies, a new action drops
/// redo; the selection is cleared.
#[test]
fn undo_redo_snapshots() {
    let mut t = EditTimeline::new(100);
    let s0 = t.edit().clone();
    t.split(30);
    let s1 = t.edit().clone();
    t.cut_selection();
    let s2 = t.edit().clone();
    assert!(t.undo());
    assert_eq!(t.edit(), &s1);
    assert_eq!(t.selection(), None);
    assert!(t.undo());
    assert_eq!(t.edit(), &s0);
    assert!(!t.undo());
    assert!(t.redo());
    assert!(t.redo());
    assert_eq!(t.edit(), &s2);
    assert!(!t.redo());
    t.undo();
    t.set_in(5);
    assert!(!t.history().can_redo(), "a new step drops redo");
}

/// Undo depth: the oldest steps go.
#[test]
fn undo_depth() {
    let mut t = EditTimeline::new(100).with_depth(3);
    for f in 1..=5 {
        t.split(f * 10);
    }
    assert_eq!(t.history().undo_len(), 3);
    while t.undo() {}
    assert_eq!(
        t.edit().parts().len(),
        3,
        "the two oldest splits cannot be undone"
    );
}

/// §6.5 / §7 item 52: trim handles clamp (`in ∈ [0, out−1]`, `out ∈ [in+1, N]`), show the new
/// first / last frame; an empty move leaves no undo step; a real one leaves exactly one.
#[test]
fn handle_drag() {
    let mut t = EditTimeline::new(100);
    t.begin_handle(Handle::In);
    assert_eq!(t.dragging(), Some(Handle::In));
    assert_eq!(t.drag_handle(30), Some(30));
    assert_eq!(t.drag_handle(500), Some(99), "in stays before out");
    assert_eq!(t.drag_handle(-5), Some(0));
    assert!(!t.end_handle(), "back where it was — no step (§7 item 52)");
    assert_eq!(t.history().undo_len(), 0);

    t.begin_handle(Handle::Out);
    assert_eq!(
        t.drag_handle(50),
        Some(49),
        "shows the last frame before the end"
    );
    assert_eq!(t.drag_handle(500), Some(99));
    assert_eq!(t.drag_handle(60), Some(59));
    assert!(t.end_handle());
    assert_eq!(t.edit().out_point(), 60);
    assert_eq!(t.history().undo_len(), 1);

    t.begin_handle(Handle::In);
    assert_eq!(t.drag_handle(80), Some(59), "in stays before out");
    t.end_handle();
    t.begin_handle(Handle::Out);
    assert_eq!(t.drag_handle(0), Some(59), "out stays after in");
    t.end_handle();
    assert_eq!((t.edit().in_point(), t.edit().out_point()), (59, 60));
    assert_eq!(t.drag_handle(10), None, "not dragging");
    t.undo();
    t.undo();
    assert_eq!((t.edit().in_point(), t.edit().out_point()), (0, 100));
}

/// §6.2 dirty: keep ≠ saved; splits are not dirty; after a save, undo makes it dirty again and
/// undoing back to the saved keep makes it clean.
#[test]
fn cuts_dirty() {
    let mut t = EditTimeline::new(100);
    assert!(!t.cuts_dirty());
    t.cut_range(10, 20);
    assert!(t.cuts_dirty());
    t.mark_saved();
    assert!(!t.cuts_dirty());
    t.undo();
    assert!(t.cuts_dirty());
    t.redo();
    assert!(!t.cuts_dirty());
    let job_keep = t.edit().keep_segs();
    t.set_in(5); // edited while the save job ran
    t.mark_saved_keep(job_keep);
    assert!(t.cuts_dirty());
}

/// `LhvApply` / §7 item 49: saved edits are taken only if they cover `[0, N)`; the tail is
/// fitted to the frame count the engine reports (it can be a frame off).
#[test]
fn restore_from_project() {
    let p = [
        Part::new(0, 40, false),
        Part::new(40, 60, true),
        Part::new(60, 100, false),
    ];
    let e = VideoEdit::from_saved(&p, 5, 90, 100).unwrap();
    assert_eq!(
        parts(&e),
        parts(&VideoEdit::from_saved(&p, 5, 90, 100).unwrap())
    );
    assert_eq!(keep(&e), [(5, 40), (60, 90)]);
    // one frame fewer: the last part shrinks, out beyond N becomes N
    let e = VideoEdit::from_saved(&p, 5, 100, 99).unwrap();
    assert_eq!(e.parts()[2], Part::new(60, 99, false));
    assert_eq!(e.out_point(), 99);
    // one frame more: the last part grows
    let e = VideoEdit::from_saved(&p, 0, 100, 101).unwrap();
    assert_eq!(e.parts()[2], Part::new(60, 101, false));
    assert_eq!(e.out_point(), 100, "an out inside N stays");
    // many frames fewer: parts starting at or past N are dropped
    let e = VideoEdit::from_saved(&p, 0, 100, 50).unwrap();
    assert_eq!(parts(&e), [(0, 40, false), (40, 50, true)]);
    assert_eq!((e.in_point(), e.out_point()), (0, 50));
    // a bad out becomes N (out is fixed first, then in is checked against it), a bad in 0
    let e = VideoEdit::from_saved(&p, 95, 90, 100).unwrap();
    assert_eq!((e.in_point(), e.out_point()), (95, 100));
    let e = VideoEdit::from_saved(&p, -3, 50, 100).unwrap();
    assert_eq!((e.in_point(), e.out_point()), (0, 50));
    let e = VideoEdit::from_saved(&p, 60, 100, 50).unwrap();
    assert_eq!((e.in_point(), e.out_point()), (0, 50));
    // not about these frames
    assert!(VideoEdit::from_saved(&[], 0, 0, 100).is_none());
    assert!(VideoEdit::from_saved(&[Part::new(1, 100, false)], 0, 100, 100).is_none());
    assert!(
        VideoEdit::from_saved(
            &[Part::new(0, 40, false), Part::new(41, 100, false)],
            0,
            100,
            100
        )
        .is_none(),
        "gap"
    );
    assert!(
        VideoEdit::from_saved(
            &[Part::new(0, 40, false), Part::new(40, 40, false)],
            0,
            40,
            40
        )
        .is_none(),
        "empty part"
    );
    // a reopened project is its own saved state
    let t = EditTimeline::with_edit(VideoEdit::from_saved(&p, 0, 100, 100).unwrap());
    assert!(!t.cuts_dirty() && t.edit().is_edited());
}

/// `EvOnEvent LOADEDDATA`: the engine's frame count differs → fresh edits, but only while
/// nothing can be undone (the user's work is never thrown away).
#[test]
fn adopt_frame_count() {
    let mut t = EditTimeline::new(100);
    assert!(!t.adopt_frame_count(100, false));
    assert!(t.adopt_frame_count(101, false));
    assert_eq!(t.edit().frames(), 101);
    t.split(50);
    assert!(!t.adopt_frame_count(99, true));
    assert_eq!(t.edit().frames(), 101);
}

// ---- shared undo queue ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
struct FakeMarks {
    state: i32,
    undo: Vec<i32>,
    redo: Vec<i32>,
}

impl FakeMarks {
    fn change(&mut self, v: i32, order: &mut UndoOrder, cuts: &mut EditTimeline) {
        self.undo.push(self.state);
        self.redo.clear();
        self.state = v;
        order.note_mark(cuts);
    }
}

impl MarksUndo for FakeMarks {
    fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    fn undo(&mut self) -> bool {
        let Some(v) = self.undo.pop() else {
            return false;
        };
        self.redo.push(self.state);
        self.state = v;
        true
    }
    fn redo(&mut self) -> bool {
        let Some(v) = self.redo.pop() else {
            return false;
        };
        self.undo.push(self.state);
        self.state = v;
        true
    }
    fn clear_redo(&mut self) {
        self.redo.clear();
    }
}

/// CAPS-80: one queue for cuts and marks — undo goes back in the order the steps were made.
#[test]
fn shared_queue_order() {
    let mut o = UndoOrder::new();
    let mut c = EditTimeline::new(100);
    let mut m = FakeMarks::default();
    if c.split(50) {
        o.note_cut(&mut m);
    }
    m.change(1, &mut o, &mut c);
    if c.set_in(10) {
        o.note_cut(&mut m);
    }
    m.change(2, &mut o, &mut c);
    assert_eq!(o.undo_any(&mut c, &mut m), Some(Track::Marks));
    assert_eq!(m.state, 1);
    assert_eq!(o.undo_any(&mut c, &mut m), Some(Track::Cuts));
    assert_eq!(c.edit().in_point(), 0);
    assert_eq!(o.undo_any(&mut c, &mut m), Some(Track::Marks));
    assert_eq!(m.state, 0);
    assert_eq!(o.undo_any(&mut c, &mut m), Some(Track::Cuts));
    assert_eq!(c.edit().parts().len(), 1);
    assert_eq!(o.undo_any(&mut c, &mut m), None);
    assert_eq!(o.redo_any(&mut c, &mut m), Some(Track::Cuts));
    assert_eq!(o.redo_any(&mut c, &mut m), Some(Track::Marks));
    assert_eq!((c.edit().parts().len(), m.state), (2, 1));
}

/// `EvNoteCutUndo` / `EvNoteMarkUndo`: a new step of one system drops the redo of BOTH.
#[test]
fn shared_queue_new_step_drops_both_redos() {
    let mut o = UndoOrder::new();
    let mut c = EditTimeline::new(100);
    let mut m = FakeMarks::default();
    if c.split(50) {
        o.note_cut(&mut m);
    }
    m.change(1, &mut o, &mut c);
    o.undo_any(&mut c, &mut m);
    o.undo_any(&mut c, &mut m);
    assert!(c.history().can_redo() && m.can_redo() && o.can_redo());
    if c.set_in(3) {
        o.note_cut(&mut m);
    }
    assert!(!c.history().can_redo() && !m.can_redo() && !o.can_redo());

    o.undo_any(&mut c, &mut m);
    assert!(c.history().can_redo());
    m.change(5, &mut o, &mut c);
    assert!(
        !c.history().can_redo(),
        "a mark step drops the timeline redo"
    );
}

/// Entries whose stack was trimmed by depth are skipped (the `while` of `EvUndoAny`).
#[test]
fn shared_queue_skips_exhausted_stacks() {
    let mut o = UndoOrder::new();
    let mut c = EditTimeline::new(100).with_depth(1);
    let mut m = FakeMarks::default();
    m.change(1, &mut o, &mut c);
    for f in [10, 20] {
        if c.split(f) {
            o.note_cut(&mut m);
        }
    }
    assert_eq!(o.undo_any(&mut c, &mut m), Some(Track::Cuts));
    assert_eq!(
        o.undo_any(&mut c, &mut m),
        Some(Track::Marks),
        "the trimmed cut step is skipped"
    );
    assert_eq!(m.state, 0);
    assert!(!o.can_undo());
}

/// §7 item 54: undo or redo of marks after a save leaves the project dirty on purpose (the
/// generation only grows); a marks drag that moved nothing leaves no trace (`EvMarksDragEnd`).
#[test]
fn marks_dirty_after_undo() {
    let mut o = UndoOrder::new();
    let mut c = EditTimeline::new(100);
    let mut m = FakeMarks::default();
    m.change(1, &mut o, &mut c);
    o.mark_saved();
    assert!(!o.marks_dirty());
    o.undo_any(&mut c, &mut m);
    assert!(o.marks_dirty());
    o.redo_any(&mut c, &mut m);
    assert!(
        o.marks_dirty(),
        "back at the saved marks — still dirty (§7 item 54)"
    );
    o.mark_saved();
    // an empty drag: the marks side pops its step, the queue forgets the entry
    m.change(1, &mut o, &mut c);
    m.undo.pop();
    o.forget_last_mark();
    assert!(!o.marks_dirty());
    assert_eq!(m.state, 1);
    assert_eq!(o.undo_any(&mut c, &mut m), Some(Track::Marks));
    assert_eq!(m.state, 0, "the undone entry is the earlier real change");
    assert_eq!(o.undo_any(&mut c, &mut m), None);
    o.touch_marks();
    assert!(o.marks_dirty());
}

// ---- playback ----------------------------------------------------------------------------------

fn pb(e: &VideoEdit, looping: bool) -> Playback<'_> {
    Playback {
        edit: e,
        fps: 10.0,
        frames: e.frames(),
        looping,
    }
}

fn edited() -> VideoEdit {
    let mut t = EditTimeline::new(100);
    t.cut_range(20, 30);
    t.set_in(5);
    t.set_out(89);
    t.edit().clone()
}

/// `EvApplyLoop`: with edits the editor runs the loop, not the engine.
#[test]
fn engine_loop_only_without_edits() {
    let fresh = VideoEdit::new(100);
    assert!(pb(&fresh, true).engine_loop());
    assert!(!pb(&fresh, false).engine_loop());
    let e = edited();
    assert!(!pb(&e, true).engine_loop());
}

/// §6.2 `EvSkipCut` / `EvPlayedToEnd`: a cut frame is jumped over; after the last kept frame —
/// restart from the first kept one (loop) or stop on the last kept one.
#[test]
fn playback_skips_cuts() {
    let e = edited();
    let p = pb(&e, false);
    assert_eq!(p.skip_cut(1.05), PlayAction::Continue);
    assert_eq!(p.skip_cut(2.05), PlayAction::Seek(seek_pos(30, 10.0)));
    assert_eq!(p.skip_cut(9.05), PlayAction::Stop(Some(seek_pos(89, 10.0))));
    assert_eq!(
        pb(&e, true).skip_cut(9.05),
        PlayAction::Restart(seek_pos(5, 10.0))
    );
    assert_eq!(
        pb(&VideoEdit::new(100), false).skip_cut(9.05),
        PlayAction::Continue,
        "no edits — the engine plays everything"
    );
    let mut t = EditTimeline::new(100);
    t.cut_range(0, 100);
    assert_eq!(pb(t.edit(), true).played_to_end(), PlayAction::Stop(None));
}

/// `EvStep`: steps jump over what is cut; nowhere further — stay.
#[test]
fn steps_skip_cuts() {
    let e = edited();
    let p = pb(&e, false);
    assert_eq!(p.step(19, 1), 30);
    assert_eq!(p.step(30, -1), 19);
    assert_eq!(p.step(18, 3), 31);
    assert_eq!(p.step(89, 1), 89, "past out — nowhere to go");
    assert_eq!(p.step(5, -1), 5);
    let f = VideoEdit::new(100);
    assert_eq!(pb(&f, false).step(99, 1), 99);
    assert_eq!(pb(&f, false).step(0, -1), 0);
    assert_eq!(pb(&f, false).step(50, -10), 40);
}

/// `EvHome` / `EvEnd`: the first / last kept frame.
#[test]
fn home_end() {
    let e = edited();
    assert_eq!((pb(&e, false).home(), pb(&e, false).end()), (5, 89));
    let f = VideoEdit::new(100);
    assert_eq!((pb(&f, false).home(), pb(&f, false).end()), (0, 99));
}

/// `EvToggleReverse` / `EvRevStep`: reverse starts from the end when at the start (with edits —
/// from the last kept frame, ¾ inside); cut frames are jumped over backwards; with the loop it
/// wraps to the last kept frame, without — stops at the first kept one.
#[test]
fn reverse_skips_cuts() {
    let e = edited();
    let p = pb(&e, false);
    assert_eq!(p.reverse_start(0.0, 10.0), Some(10.0));
    assert_eq!(p.reverse_start(0.4, 10.0), Some(reverse_pos(89, 10.0)));
    assert_eq!(p.reverse_start(5.0, 10.0), Some(5.0));
    assert_eq!(p.reverse_skip(5.0), None);
    assert_eq!(p.reverse_skip(2.55), Some((reverse_pos(19, 10.0), true)));
    assert_eq!(p.reverse_skip(0.2), Some((seek_pos(5, 10.0), false)));
    assert_eq!(
        pb(&e, true).reverse_skip(0.2),
        Some((reverse_pos(89, 10.0), true))
    );
    let mut t = EditTimeline::new(100);
    t.cut_range(0, 100);
    assert_eq!(pb(t.edit(), false).reverse_start(3.0, 10.0), None);
}

// ---- timeline geometry -------------------------------------------------------------------------

/// §6.5: x ↔ time, the zoom limit of 12 px per frame, zoom around the cursor, scroll clamps.
#[test]
fn timeline_geometry() {
    let mut v = TimelineView::new(10.0, 30.0, 100, 600);
    assert_eq!(v.time_to_x(0.0), 100);
    assert_eq!(v.time_to_x(10.0), 700);
    assert!((v.x_to_time(400) - 5.0).abs() < 1e-9);
    assert_eq!(v.x_to_time(0), 0.0, "clamped");
    assert_eq!(v.x_to_time(10_000), 10.0, "clamped");
    assert_eq!(v.x_to_edge(400), 150);
    // zoom in around x = 400 (t = 5 s): the time under the cursor stays there
    v.zoom_at(400, 4.0, MAX_PX_PER_FRAME);
    assert!((v.zoom - 1.25f64.powi(4)).abs() < 1e-9);
    assert!((v.x_to_time(400) - 5.0).abs() < 1e-9);
    // the closest zoom: 12 px per frame
    v.zoom_at(400, 100.0, MAX_PX_PER_FRAME);
    assert!((v.zoom - 300.0 * 12.0 / 600.0).abs() < 1e-9);
    assert_eq!(v.edge_to_x(151) - v.edge_to_x(150), 12);
    v.zoom_at(400, -1000.0, MAX_PX_PER_FRAME);
    assert_eq!((v.zoom, v.off), (1.0, 0.0));
    // pan clamps to [0, dur − visible]
    v.zoom = 2.0;
    v.pan(-100.0);
    assert!((v.off - 5.0).abs() < 1e-9);
    v.pan(100.0);
    assert_eq!(v.off, 0.0);
}

/// `EvAutoScroll`: the playhead leaving the view (or its last 5 %) puts it a tenth from the left.
#[test]
fn timeline_auto_scroll() {
    let mut v = TimelineView::new(10.0, 30.0, 0, 600);
    v.auto_scroll(9.0);
    assert_eq!(v.off, 0.0, "not zoomed — never scrolls");
    v.zoom = 4.0; // 2.5 s visible
    v.auto_scroll(1.0);
    assert_eq!(v.off, 0.0);
    v.auto_scroll(2.4); // > 0.95 · 2.5
    assert!((v.off - 2.15).abs() < 1e-9);
    v.auto_scroll(1.0); // before the view
    assert!((v.off - 0.75).abs() < 1e-9);
    v.auto_scroll(9.99);
    assert!((v.off - 7.5).abs() < 1e-9, "clamped to dur − visible");
}

/// §6.5: the trim handles are hit from their current x (they move with zoom and scroll); `in`
/// wins a tie.
#[test]
fn timeline_handles() {
    let mut t = EditTimeline::new(300);
    t.set_in(30);
    t.set_out(269);
    let v = TimelineView::new(10.0, 30.0, 0, 600);
    let e = t.edit();
    assert_eq!(v.edge_to_x(30), 60);
    assert_eq!(v.handle_at(60, e, HANDLE_GRAB_PX), Some(Handle::In));
    assert_eq!(v.handle_at(69, e, HANDLE_GRAB_PX), Some(Handle::In));
    assert_eq!(v.handle_at(70, e, HANDLE_GRAB_PX), None);
    assert_eq!(v.handle_at(545, e, HANDLE_GRAB_PX), Some(Handle::Out));
    let mut z = v;
    z.zoom = 10.0;
    z.off = 0.5;
    assert_eq!(z.handle_at(60, e, HANDLE_GRAB_PX), None, "moved with zoom");
    assert_eq!(z.handle_at(300, e, HANDLE_GRAB_PX), Some(Handle::In));
    let mut close = EditTimeline::new(300);
    close.set_in(10);
    close.set_out(10); // out = 11: both handles 2 px apart
    let c = close.edit();
    assert_eq!(
        v.handle_at(21, c, HANDLE_GRAB_PX),
        Some(Handle::In),
        "a tie → in"
    );
    assert_eq!(v.handle_at(23, c, HANDLE_GRAB_PX), Some(Handle::Out));
    assert_eq!(v.handle_at(0, &VideoEdit::new(0), HANDLE_GRAB_PX), None);
}

// ---- export wiring -----------------------------------------------------------------------------

/// §6.3: the export consumes exactly `keep_segs`; a stretched sample across a cut is split,
/// audio totals match the kept frames by time (§7 items 17, 18).
#[test]
fn export_takes_the_edits() {
    let mut t = EditTimeline::new(90);
    t.cut_range(12, 14);
    let e = t.edit();
    let pieces = crate::export::split_video_sample(&e.keep_segs(), 10, 6, 30.0);
    assert_eq!(pieces.len(), 2);
    assert_eq!((pieces[1].src_frame, pieces[1].out_frame), (14, 12));
    let a = e.audio_cut(30.0, 48_000, 2);
    assert_eq!(a.total_out(), e.kept_frames() * 1600);
}

// ---- properties ----------------------------------------------------------------------------------

/// xorshift64* — a small deterministic generator (no proptest in the workspace).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + (self.next() % (hi - lo) as u64) as i64
    }
}

/// One random user action; `true` when it recorded an undo step.
fn random_action(r: &mut Rng, t: &mut EditTimeline) -> bool {
    let n = t.edit().frames();
    let f = r.range(-2, n + 3);
    match r.range(0, 8) {
        0 => t.split(f),
        1 => {
            let g = r.range(0, n.max(1));
            t.select_range(f.clamp(0, n), g);
            t.cut_selection()
        }
        2 => {
            t.click_part(f);
            t.cut_selection()
        }
        3 => t.set_in(f),
        4 => t.set_out(f),
        5 => {
            let h = if r.next().is_multiple_of(2) {
                Handle::In
            } else {
                Handle::Out
            };
            t.begin_handle(h);
            for _ in 0..r.range(0, 4) {
                t.drag_handle(r.range(-5, n + 5));
            }
            t.end_handle()
        }
        6 => {
            t.clear_selection();
            false
        }
        _ => t.cut_range(f, r.range(-2, n + 3)),
    }
}

/// Brute-force reference: kept frames from the definition of §6.2.
fn brute_kept(e: &VideoEdit) -> Vec<i64> {
    (0..e.frames())
        .filter(|&f| {
            f >= e.in_point()
                && f < e.out_point()
                && e.parts().iter().any(|p| p.contains(f) && !p.off)
        })
        .collect()
}

fn check_invariants(e: &VideoEdit, n: i64) {
    assert!(e.is_valid(), "{e:?}");
    assert_eq!(e.frames(), n, "the parts always cover [0, N)");
    let segs = e.keep_segs();
    assert!(
        segs.windows(2).all(|w| w[0].b < w[1].a),
        "sorted, merged: {segs:?}"
    );
    let kept = brute_kept(e);
    assert_eq!(
        e.kept_frames(),
        kept.len() as i64,
        "total = sum of segments"
    );
    for f in 0..n {
        assert_eq!(e.is_kept(f), kept.binary_search(&f).is_ok());
        let next = kept.iter().copied().find(|&k| k >= f);
        let prev = kept.iter().rev().copied().find(|&k| k <= f);
        assert_eq!(e.next_kept(f), next);
        assert_eq!(e.prev_kept(f), prev);
    }
    for (out, &src) in kept.iter().enumerate() {
        assert_eq!(e.edited_frame(src), Some(out as i64), "source → edited");
        assert_eq!(e.source_frame(out as i64), src, "edited → source");
    }
    assert_eq!(e.is_edited(), kept.len() as i64 != n);
}

/// Invariants after any sequence of actions: coverage of `[0, N)`, total = sum of segments,
/// kept set = definition, source ↔ edited round trip, next/prev by brute force.
#[test]
fn property_invariants() {
    let mut r = Rng(0x9E37_79B9_7F4A_7C15);
    for _ in 0..200 {
        let n = r.range(1, 80);
        let mut t = EditTimeline::new(n);
        for _ in 0..40 {
            random_action(&mut r, &mut t);
            check_invariants(t.edit(), n);
        }
    }
}

/// Undo after any sequence restores every earlier state exactly, redo replays it.
#[test]
fn property_undo_redo_exact() {
    let mut r = Rng(0xD1B5_4A32_D192_ED03);
    for _ in 0..200 {
        let n = r.range(1, 60);
        let mut t = EditTimeline::new(n);
        let mut states = vec![t.edit().clone()];
        for _ in 0..30 {
            let before = t.edit().clone();
            let steps = t.history().undo_len();
            random_action(&mut r, &mut t);
            if t.history().undo_len() > steps {
                assert_ne!(&before, t.edit(), "a step only for a real change");
                states.push(t.edit().clone());
            } else {
                assert_eq!(&before, t.edit(), "no step — no change");
            }
        }
        for s in states.iter().rev().skip(1) {
            assert!(t.undo());
            assert_eq!(t.edit(), s);
        }
        assert!(!t.undo());
        for s in states.iter().skip(1) {
            assert!(t.redo());
            assert_eq!(t.edit(), s);
        }
        assert!(!t.redo());
    }
}

/// The shared queue after any interleaving of cut and mark steps: undo walks back through every
/// combined state in order, redo forward.
#[test]
fn property_shared_queue() {
    let mut r = Rng(0x0123_4567_89AB_CDEF);
    for _ in 0..100 {
        let n = r.range(1, 50);
        let mut o = UndoOrder::new();
        let mut c = EditTimeline::new(n);
        let mut m = FakeMarks::default();
        let mut states = vec![(c.edit().clone(), m.state)];
        for i in 0..30 {
            if r.next().is_multiple_of(3) {
                m.change(i + 1, &mut o, &mut c);
                states.push((c.edit().clone(), m.state));
            } else {
                let steps = c.history().undo_len();
                random_action(&mut r, &mut c);
                if c.history().undo_len() > steps {
                    o.note_cut(&mut m);
                    states.push((c.edit().clone(), m.state));
                }
            }
            // sometimes undo a few in the middle and continue from there
            if r.next().is_multiple_of(7) {
                for _ in 0..r.range(1, 3) {
                    if o.undo_any(&mut c, &mut m).is_some() {
                        states.pop();
                    }
                    assert_eq!(states.last().unwrap(), &(c.edit().clone(), m.state));
                }
            }
        }
        let snapshot = states.clone();
        for s in snapshot.iter().rev().skip(1) {
            assert!(o.undo_any(&mut c, &mut m).is_some());
            assert_eq!(&(c.edit().clone(), m.state), s);
        }
        assert_eq!(o.undo_any(&mut c, &mut m), None);
        for s in snapshot.iter().skip(1) {
            assert!(o.redo_any(&mut c, &mut m).is_some());
            assert_eq!(&(c.edit().clone(), m.state), s);
        }
    }
}

#[test]
fn to_and_from_the_document_timeline() {
    let e = VideoEdit::from_saved(
        &[
            Part::new(0, 30, false),
            Part::new(30, 60, true),
            Part::new(60, 90, false),
        ],
        5,
        80,
        90,
    )
    .expect("valid");
    let t = e.to_timeline();
    assert_eq!((t.frames(), t.in_point, t.out_point), (90, 5, 80));
    assert_eq!(VideoEdit::from_timeline(&t), Some(e));
    let mut bad = t;
    bad.parts[1].a = 31;
    assert_eq!(VideoEdit::from_timeline(&bad), None);
}
