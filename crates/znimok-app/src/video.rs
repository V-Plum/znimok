//! The editor's «Відео» mode (ZK-181): a player that decodes the recording on its own thread
//! and hands frames to the canvas, the timeline's state (the edit list of `znimok-video`, the
//! selection, the drag in progress, the view's zoom and offset) and what the timeline shows.
//!
//! The frames come from the CPU decoder of `znimok-video-win` for now (NV12 → RGBA here, then
//! the canvas draws them like a screenshot's picture); the GPU path without copies is ZK-92.
//! On macOS there is no player yet: the poster frame stands for the video, the timeline works.
//!
//! The edits live in the document's timeline (ZK-144): every finished edit is one
//! `SetTimeline` command of the core editor, so the marks and the cuts share one undo history.
//! `EditTimeline` here only holds what is being dragged or selected.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender};

use znimok_core::{Raster, Timeline};
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
    /// The player, when this machine can decode (Windows now).
    pub player: Option<Player>,
    /// Why there is no player (shown once).
    pub player_error: Option<String>,
    /// The frame on the canvas and its pixels (None: the poster).
    pub frame: i64,
    pub raster: Option<Arc<Raster>>,
    pub playing: bool,
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
            player_error: None,
            frame: 0,
            raster: None,
            playing: false,
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
        if self.tl.edit().to_timeline() != *t
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

// ------------------------------------------------------------------ the player

/// A frame the player delivered.
pub struct Delivered {
    pub frame: i64,
    pub raster: Arc<Raster>,
    /// Still playing after this frame (false: the end was reached, or a pause was asked).
    pub playing: bool,
}

// The player runs on Windows only for now (ZK-92 brings macOS): elsewhere nothing reads these.
#[cfg_attr(not(windows), allow(dead_code))]
enum Cmd {
    Seek(i64),
    Play {
        from: i64,
        speed: f64,
        looping: bool,
    },
    Pause,
    Edit(VideoEdit),
    Quit,
}

/// Decodes on its own thread; frames come back on the UI thread through `on_frame`.
pub struct Player {
    tx: Sender<Cmd>,
}

impl Player {
    /// Opens the recording of a document: the stream is copied out of the `.znimok` once, into
    /// the cache (ZK-92 will read it in place), and decoded from there.
    pub fn open(
        part: &znimok_format::VideoPart,
        on_frame: impl Fn(Delivered) + Send + 'static,
    ) -> Result<Player, String> {
        let mp4 = extract_stream(part)?;
        let (tx, rx) = std::sync::mpsc::channel::<Cmd>();
        let fps = part.video.info.fps();
        let frames = part.video.info.frames as i64;
        std::thread::Builder::new()
            .name("znimok-player".into())
            .spawn(move || run(rx, &mp4, fps, frames, on_frame))
            .map_err(|e| e.to_string())?;
        Ok(Player { tx })
    }

    pub fn seek(&self, frame: i64) {
        let _ = self.tx.send(Cmd::Seek(frame));
    }

    pub fn play(&self, from: i64, speed: f64, looping: bool) {
        let _ = self.tx.send(Cmd::Play {
            from,
            speed,
            looping,
        });
    }

    pub fn pause(&self) {
        let _ = self.tx.send(Cmd::Pause);
    }

    pub fn set_edit(&self, e: VideoEdit) {
        let _ = self.tx.send(Cmd::Edit(e));
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Quit);
    }
}

/// The MP4 inside the document, copied to the cache under a name from the file's size and
/// position (a re-saved document gets a new copy; old copies go with the cache).
fn extract_stream(part: &znimok_format::VideoPart) -> Result<PathBuf, String> {
    use std::io::{Read, Seek, Write};
    let dir = crate::library::cache_dir().join("video");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let stem = part
        .source
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "video".into());
    let first = part.payload.ranges.first().map_or(0, |r| r.start);
    let out = dir.join(format!("{stem}-{first}-{}.mp4", part.payload.len()));
    if out.is_file() && std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0) == part.payload.len()
    {
        return Ok(out);
    }
    let f = std::fs::File::open(&part.source).map_err(|e| e.to_string())?;
    let mut reader =
        znimok_format::video::PayloadReader::new(std::io::BufReader::new(f), &part.payload);
    reader
        .seek(std::io::SeekFrom::Start(0))
        .map_err(|e| e.to_string())?;
    let tmp = out.with_extension("part");
    {
        let mut w =
            std::io::BufWriter::new(std::fs::File::create(&tmp).map_err(|e| e.to_string())?);
        let mut buf = vec![0u8; 1 << 20];
        loop {
            let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            w.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        }
        w.flush().map_err(|e| e.to_string())?;
    }
    std::fs::rename(&tmp, &out).map_err(|e| e.to_string())?;
    Ok(out)
}

#[cfg(windows)]
fn run(rx: Receiver<Cmd>, mp4: &Path, fps: f64, frames: i64, on_frame: impl Fn(Delivered)) {
    use std::sync::mpsc::TryRecvError;
    use std::time::{Duration, Instant};
    use znimok_video::traits::{Decoded, VideoDecoder, frames_of_sample, seek_time_for_frame};
    let mut dec = match znimok_video_win::MfDecoder::open(mp4) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("player: {e}");
            return;
        }
    };
    let fps = if fps > 0.0 {
        fps
    } else {
        dec.info().fps.max(1.0)
    };
    let mut edit = VideoEdit::new(frames.max(1));
    let mut playing = false;
    let mut speed = 1.0f64;
    let mut looping = false;
    // The last frame decoded (the reader stands after it), and the one wanted next.
    let mut cur: i64 = -1;
    let mut want: Option<i64> = None;
    let mut due = Instant::now();
    // Decodes forward to frame `f` (seeking first when it is behind or far ahead); the frame's
    // pixels, or None at the end of the stream.
    let decode_to =
        |dec: &mut znimok_video_win::MfDecoder, cur: &mut i64, f: i64| -> Option<Arc<Raster>> {
            if f < *cur || f > *cur + 60 || *cur < 0 {
                if dec.seek(seek_time_for_frame(f, fps)).is_err() {
                    return None;
                }
                *cur = -1;
            }
            loop {
                match dec.next() {
                    Ok(Some(Decoded::Video {
                        time_hns,
                        duration_hns,
                        frame,
                    })) => {
                        let (idx, n) = frames_of_sample(time_hns, duration_hns, fps);
                        *cur = idx + n - 1;
                        if idx + n > f {
                            return Some(Arc::new(nv12_to_rgba(&frame)));
                        }
                    }
                    Ok(Some(Decoded::Audio { .. })) => continue,
                    Ok(None) => return None,
                    Err(e) => {
                        eprintln!("player: {e}");
                        return None;
                    }
                }
            }
        };
    loop {
        // Commands: all that are waiting; when idle, wait for one.
        let first = if playing || want.is_some() {
            match rx.try_recv() {
                Ok(c) => Some(c),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => return,
            }
        } else {
            match rx.recv() {
                Ok(c) => Some(c),
                Err(_) => return,
            }
        };
        let mut cmds: Vec<Cmd> = first.into_iter().collect();
        while let Ok(c) = rx.try_recv() {
            cmds.push(c);
        }
        for c in cmds {
            match c {
                Cmd::Seek(f) => {
                    playing = false;
                    want = Some(f.clamp(0, (frames - 1).max(0)));
                }
                Cmd::Play {
                    from,
                    speed: s,
                    looping: l,
                } => {
                    speed = s;
                    looping = l;
                    playing = true;
                    want = Some(from.clamp(0, (frames - 1).max(0)));
                    due = Instant::now();
                }
                Cmd::Pause => playing = false,
                Cmd::Edit(e) => edit = e,
                Cmd::Quit => return,
            }
        }
        if let Some(f) = want.take() {
            match decode_to(&mut dec, &mut cur, f) {
                Some(r) => on_frame(Delivered {
                    frame: f,
                    raster: r,
                    playing,
                }),
                None => playing = false,
            }
            if playing {
                due = Instant::now() + Duration::from_secs_f64(1.0 / (fps * speed));
            }
            continue;
        }
        if !playing {
            continue;
        }
        // The next frame to play: the one after the last shown, skipping what is cut.
        let shown = cur;
        let mut next = shown + 1;
        if !edit.is_kept(next) {
            match edit.next_kept(next) {
                Some(k) => next = k,
                None => next = edit.out_point(),
            }
        }
        if next >= edit.out_point() {
            if looping {
                want = Some(edit.next_kept(edit.in_point()).unwrap_or(edit.in_point()));
                continue;
            }
            playing = false;
            let last = (edit.out_point() - 1).max(0);
            if let Some(r) = decode_to(&mut dec, &mut cur, last) {
                on_frame(Delivered {
                    frame: last,
                    raster: r,
                    playing: false,
                });
            }
            continue;
        }
        let now = Instant::now();
        if due > now {
            std::thread::sleep(due - now);
        }
        // Far behind (a slow decode): the frames in between are skipped, not shown late.
        let late = now.saturating_duration_since(due).as_secs_f64() * fps * speed;
        if late > 2.0 {
            let skip = late as i64;
            next = (next + skip).min(edit.out_point() - 1);
            due = Instant::now();
        }
        match decode_to(&mut dec, &mut cur, next) {
            Some(r) => {
                on_frame(Delivered {
                    frame: next,
                    raster: r,
                    playing: true,
                });
                due += Duration::from_secs_f64(1.0 / (fps * speed));
            }
            None => playing = false,
        }
    }
}

#[cfg(not(windows))]
fn run(_rx: Receiver<Cmd>, _mp4: &Path, _fps: f64, _frames: i64, _on_frame: impl Fn(Delivered)) {
    // ZK-92: AVFoundation on macOS. Until then the poster frame stands for the video.
}

/// NV12 (BT.709, limited range) → RGBA, on the CPU (ZK-92 moves this to the GPU).
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
