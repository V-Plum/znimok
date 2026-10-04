//! Editing a recording (ZK-239): what is kept (trim, cut, restore), the sound tracks (mute), the
//! size on export, and when marks show. All on the file, in the MCP process, as the editor does:
//! the cuts and the marks' times are the poster document's timeline (one undo history in the
//! app, ZK-144), the size and the tracks belong to the video part. The frame (crop) is the
//! document's crop, so `transform.crop` already works on a recording; turning and mirroring a
//! recording is not something the app does, nor a speed: those are refused.

use crate::library;
use crate::permissions::Scope;
use crate::tools::{Agent, Output, Tool, doc_arg, image, obj, summary};
use serde_json::{Value, json};
use std::path::Path;
use znimok_core::command::Command;
use znimok_core::{Document, Editor, ObjectId, Timeline, TimelinePart};
use znimok_format::VideoPart;

fn span_arg(what: &str) -> Value {
    json!({
        "type": "object",
        "description": what,
        "properties": {
            "from_ms": {"type": "integer", "minimum": 0, "description": "Start, ms of the recording as recorded"},
            "to_ms": {"type": "integer", "minimum": 0, "description": "End (not included), ms of the recording as recorded"}
        },
        "required": ["from_ms", "to_ms"],
        "additionalProperties": false
    })
}

pub(crate) const TOOLS: &[Tool] = &[Tool {
    name: "video_edit",
    title: "Edit a recording",
    description: "What a recording keeps and how it is exported, in this order: restore (cut stretches back, or restore_all), cut (stretches left out), trim (where it starts and ends, or reset), mute (sound tracks, by their index in video_info), size (the size on export: width / height / percent, or reset). Times are ms of the recording as recorded (as video_info and devlog give them). The file's video is untouched: an export applies the edits.",
    scope: Some(Scope::LibraryWrite),
    read_only: false,
    schema: || {
        obj(
            json!({
                "document": doc_arg(),
                "restore": {"type": "array", "items": span_arg("A cut stretch to bring back"), "minItems": 1},
                "restore_all": {"type": "boolean", "description": "Bring back everything cut"},
                "cut": {"type": "array", "items": span_arg("A stretch to leave out"), "minItems": 1},
                "trim": {
                    "type": "object",
                    "description": "Where the recording starts and ends; what is given changes",
                    "properties": {
                        "from_ms": {"type": "integer", "minimum": 0},
                        "to_ms": {"type": "integer", "minimum": 1},
                        "reset": {"type": "boolean", "description": "The whole recording again"}
                    },
                    "additionalProperties": false
                },
                "mute": {
                    "type": "array",
                    "minItems": 1,
                    "items": {
                        "type": "object",
                        "properties": {
                            "track": {"type": "integer", "minimum": 0, "description": "Index of the sound track (video_info)"},
                            "muted": {"type": "boolean", "description": "true: left out of the export; false: back in"}
                        },
                        "required": ["track", "muted"],
                        "additionalProperties": false
                    }
                },
                "size": {
                    "type": "object",
                    "description": "The size on export; the other side follows the frame's proportions",
                    "properties": {
                        "width": {"type": "integer", "minimum": 16},
                        "height": {"type": "integer", "minimum": 16},
                        "percent": {"type": "number", "exclusiveMinimum": 0, "maximum": 400},
                        "reset": {"type": "boolean", "description": "The size of the frame"}
                    },
                    "additionalProperties": false
                }
            }),
            &["document"],
        )
    },
}];

/// The recording behind a library document; an error for a screenshot.
pub(crate) fn open(path: &Path) -> Result<(Document, VideoPart), String> {
    let (doc, v) =
        znimok_format::open_parts(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let v = v.ok_or_else(|| format!("«{}» is a screenshot, not a recording", doc.name))?;
    Ok((doc, v))
}

fn fps(v: &VideoPart) -> f64 {
    (f64::from(v.video.info.fps_milli) / 1000.0).max(1e-3)
}

/// A time in ms of the recording → a frame edge in `[0, n]`.
fn frame_at(ms: i64, fps: f64, n: i64) -> i64 {
    ((ms as f64 * fps / 1000.0).round() as i64).clamp(0, n)
}

fn ms_at(f: i64, fps: f64) -> i64 {
    (f as f64 * 1000.0 / fps).round() as i64
}

fn span(v: &Value, fps: f64, n: i64) -> Result<(i64, i64), String> {
    let (a, b) = (v["from_ms"].as_i64(), v["to_ms"].as_i64());
    let (Some(a), Some(b)) = (a, b) else {
        return Err("a stretch needs «from_ms» and «to_ms»".into());
    };
    let (a, b) = (frame_at(a, fps, n), frame_at(b, fps, n));
    if b <= a {
        return Err("a stretch must end after it starts (and inside the recording)".into());
    }
    Ok((a, b))
}

/// A part boundary at frame `f` (nothing when there is one already).
fn split(t: &mut Timeline, f: i64) {
    if let Some(i) = t.parts.iter().position(|p| p.a < f && f < p.b) {
        let p = t.parts[i];
        t.parts[i].b = f;
        t.parts.insert(
            i + 1,
            TimelinePart {
                a: f,
                b: p.b,
                off: p.off,
            },
        );
    }
}

/// Frames `[a, b)` cut (`off`) or kept.
fn set_off(t: &mut Timeline, a: i64, b: i64, off: bool) {
    split(t, a);
    split(t, b);
    for p in &mut t.parts {
        if p.a >= a && p.b <= b {
            p.off = off;
        }
    }
    // Neighbours alike merge back, so restoring a cut leaves no needless splits.
    let mut merged: Vec<TimelinePart> = Vec::with_capacity(t.parts.len());
    for p in t.parts.drain(..) {
        match merged.last_mut() {
            Some(last) if last.off == p.off && last.b == p.a => last.b = p.b,
            _ => merged.push(p),
        }
    }
    t.parts = merged;
}

/// What a recording keeps and how it is exported, for an answer.
pub(crate) fn state(doc: &Document, v: &VideoPart) -> Value {
    let fps = fps(v);
    let n = i64::from(v.video.info.frames);
    let t = doc.timeline.clone().unwrap_or_else(|| Timeline::whole(n));
    let cuts: Vec<Value> = t
        .parts
        .iter()
        .filter(|p| p.off)
        .map(|p| json!({"from_ms": ms_at(p.a, fps), "to_ms": ms_at(p.b, fps)}))
        .collect();
    let kept: i64 = t
        .parts
        .iter()
        .filter(|p| !p.off)
        .map(|p| (p.b.min(t.out_point) - p.a.max(t.in_point)).max(0))
        .sum();
    let f = doc.frame();
    json!({
        "duration_ms": ms_at(n, fps),
        "kept_ms": ms_at(kept, fps),
        "trim": {"from_ms": ms_at(t.in_point, fps), "to_ms": ms_at(t.out_point, fps)},
        "cuts": cuts,
        "frame": {"x": f.x, "y": f.y, "width": f.w, "height": f.h},
        "size": v.video.out_size.map_or_else(
            || json!({"width": f.w, "height": f.h}),
            |(w, h)| json!({"width": w, "height": h}),
        ),
        "audio": v.video.audio.iter().enumerate().map(|(i, a)| json!({"track": i, "label": a.label, "muted": a.muted})).collect::<Vec<_>>(),
    })
}

fn even(x: f64) -> u32 {
    ((x / 2.0).round() as u32 * 2).clamp(16, 16384)
}

fn edit(agent: &Agent, args: &Value) -> Result<Output, String> {
    let (_, path) = agent.doc(args)?;
    let (doc, mut v) = open(&path)?;
    let (fps, n) = (fps(&v), i64::from(v.video.info.frames));
    let before = doc.timeline.clone().unwrap_or_else(|| Timeline::whole(n));
    let mut t = before.clone();
    let mut changed = false;

    if args["restore_all"].as_bool().unwrap_or(false) {
        t.parts = Timeline::whole(n).parts;
    }
    for s in args["restore"].as_array().into_iter().flatten() {
        let (a, b) = span(s, fps, n)?;
        set_off(&mut t, a, b, false);
    }
    for s in args["cut"].as_array().into_iter().flatten() {
        let (a, b) = span(s, fps, n)?;
        set_off(&mut t, a, b, true);
    }
    if let Some(tr) = args.get("trim").filter(|x| x.is_object()) {
        if tr["reset"].as_bool().unwrap_or(false) {
            (t.in_point, t.out_point) = (0, n);
        }
        if let Some(ms) = tr["from_ms"].as_i64() {
            t.in_point = frame_at(ms, fps, n);
        }
        if let Some(ms) = tr["to_ms"].as_i64() {
            t.out_point = frame_at(ms, fps, n);
        }
        if t.out_point <= t.in_point {
            return Err("trim: the end must come after the start".into());
        }
    }
    if !t.is_valid() {
        return Err("these edits leave the recording without a valid timeline".into());
    }
    if !t
        .parts
        .iter()
        .any(|p| !p.off && p.b > t.in_point && p.a < t.out_point)
    {
        return Err("these edits would leave nothing of the recording".into());
    }

    for m in args["mute"].as_array().into_iter().flatten() {
        let i = m["track"].as_u64().ok_or("mute: «track» is an index")? as usize;
        let tracks = v.video.audio.len();
        let a = v.video.audio.get_mut(i).ok_or_else(|| {
            format!("mute: no sound track {i} (the recording has {tracks}; see video_info)")
        })?;
        let muted = m["muted"]
            .as_bool()
            .ok_or("mute: «muted» is true or false")?;
        changed |= a.muted != muted;
        a.muted = muted;
    }

    if let Some(sz) = args.get("size").filter(|x| x.is_object()) {
        let f = doc.frame();
        let (fw, fh) = (f64::from(f.w.max(1)), f64::from(f.h.max(1)));
        let size = if sz["reset"].as_bool().unwrap_or(false) {
            None
        } else {
            let (w, h) = match (
                sz["width"].as_f64(),
                sz["height"].as_f64(),
                sz["percent"].as_f64(),
            ) {
                (Some(w), Some(h), _) => (w, h),
                (Some(w), None, _) => (w, w * fh / fw),
                (None, Some(h), _) => (h * fw / fh, h),
                (None, None, Some(p)) if p > 0.0 => (fw * p / 100.0, fh * p / 100.0),
                _ => return Err("size: give «width» and/or «height», «percent», or «reset»".into()),
            };
            let s = (even(w), even(h));
            (s != (f.w as u32, f.h as u32)).then_some(s)
        };
        changed |= v.video.out_size != size;
        v.video.out_size = size;
    }

    let mut ed = Editor::new(doc);
    if t != before {
        ed.apply(Command::SetTimeline {
            timeline: t,
            merge: None,
        })
        .map_err(|e| e.to_string())?;
        changed = true;
    }
    if !changed {
        return Err(
            "nothing to change: give restore, cut, trim, mute or size (or it is so already)".into(),
        );
    }
    library::save_parts(&path, &ed.doc, Some(&v))?;
    let (doc, v) = open(&path)?;
    let mut s = summary(&doc, &path);
    s["video"] = state(&doc, &v);
    let r = library::render(&doc, 1.0);
    Ok(Output::ok(s, image(&r).into_iter().collect()))
}

/// The marks' times on a recording (ZK-239): `from_ms` / `to_ms` of a mark, or `always` to show
/// it the whole time. `marks` — (id, the mark's arguments). Nothing for a screenshot when no
/// mark asks for a time; a time on a screenshot is an error.
pub(crate) fn set_times(
    ed: &mut Editor,
    path: &Path,
    marks: &[(ObjectId, Value)],
) -> Result<(), String> {
    let wants = |m: &Value| {
        !m["from_ms"].is_null() || !m["to_ms"].is_null() || m["always"].as_bool().is_some()
    };
    if !marks.iter().any(|(_, m)| wants(m)) {
        return Ok(());
    }
    let (_, v) = open(path).map_err(|_| "from_ms / to_ms are for recordings".to_string())?;
    let (fps, n) = (fps(&v), i64::from(v.video.info.frames));
    let mut t = ed
        .doc
        .timeline
        .clone()
        .unwrap_or_else(|| Timeline::whole(n));
    for (id, m) in marks.iter().filter(|(_, m)| wants(m)) {
        if m["always"].as_bool() == Some(true) {
            t.marks.remove(id);
            continue;
        }
        let (a0, b0) = t.marks.get(id).copied().unwrap_or((0, n));
        let a = m["from_ms"].as_i64().map_or(a0, |ms| frame_at(ms, fps, n));
        let b = m["to_ms"].as_i64().map_or(b0, |ms| frame_at(ms, fps, n));
        if b <= a {
            return Err(format!(
                "mark {id}: «to_ms» must come after «from_ms», inside the recording"
            ));
        }
        t.marks.insert(*id, (a, b));
    }
    ed.apply(Command::SetTimeline {
        timeline: t,
        merge: None,
    })
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// `None`: not this module's tool.
pub(crate) fn run(agent: &Agent, name: &str, args: &Value) -> Option<Result<Output, String>> {
    Some(match name {
        "video_edit" => edit(agent, args),
        _ => return None,
    })
}
