//! The editing tools (ZK-233): marks, crop, rotation, size, tone — plain arguments over the same
//! editor commands `annotate` takes raw. An agent finds what Znimok can do in the list of tools
//! instead of in the command schema; each call is saved and answers with the picture.

use crate::library;
use crate::permissions::Scope;
use crate::tools::{Agent, Output, Tool, arg_int, arg_str, doc_arg, image, obj, summary};
use serde_json::{Value, json};
use znimok_core::command::ObjectPatch;
use znimok_core::{
    Align, Command, CounterShape, Data, Document, Editor, Head, HideMode, IRect, Object, ObjectId,
    Rgb,
};
use znimok_models::ocr::Rect as OcrRect;

fn colour_arg(what: &str) -> Value {
    json!({"type": "string", "description": format!("{what}: #RRGGBB, or red, orange, yellow, green, blue, violet, black, white, grey")})
}

fn mark_schema() -> Value {
    json!({
        "type": "object",
        "description": "One mark. Boxes (rect, ellipse, hide, highlighter) take x, y, width, height; arrow and line take from and to; pen takes points; text, counter and stamp take x, y (text: its top-left corner; counter and stamp: their centre).",
        "properties": {
            "kind": {"type": "string", "enum": ["rect", "ellipse", "arrow", "line", "pen", "text", "counter", "hide", "highlighter", "stamp"]},
            "x": {"type": "integer"}, "y": {"type": "integer"},
            "width": {"type": "integer", "minimum": 1}, "height": {"type": "integer", "minimum": 1},
            "from": {"type": "array", "items": {"type": "integer"}, "minItems": 2, "maxItems": 2, "description": "arrow / line: where it starts, [x, y]"},
            "to": {"type": "array", "items": {"type": "integer"}, "minItems": 2, "maxItems": 2, "description": "arrow / line: where it ends (the arrow's head), [x, y]"},
            "heads": {"type": "string", "enum": ["none", "end", "both"], "description": "arrow / line heads; arrow: end, line: none by default"},
            "points": {"type": "array", "items": {"type": "array", "items": {"type": "integer"}, "minItems": 2, "maxItems": 2}, "description": "pen: the trail, [[x, y], …]"},
            "text": {"type": "string"},
            "size": {"type": "integer", "minimum": 6, "description": "text: font size in pixels (24); counter / stamp: diameter (36)"},
            "bold": {"type": "boolean"}, "italic": {"type": "boolean"},
            "align": {"type": "string", "enum": ["left", "center", "right"]},
            "box_width": {"type": "integer", "minimum": 0, "description": "text: wrap at this width; 0 = one line as long as the text"},
            "color": colour_arg("Line, outline or text colour (red by default; the highlighter is yellow)"),
            "fill": colour_arg("rect / ellipse: fill; text: outline; counter: the digit"),
            "line_width": {"type": "integer", "minimum": 1, "description": "Thickness of lines and outlines in pixels (4)"},
            "opacity": {"type": "integer", "minimum": 10, "maximum": 100},
            "mode": {"type": "string", "enum": ["blur", "pixelate", "plate"], "description": "hide: how (pixelate)"},
            "strength": {"type": "integer", "minimum": 1, "maximum": 100, "description": "hide: how strongly (50)"},
            "shape": {"type": "string", "enum": ["circle", "rounded_box", "pin"], "description": "counter"},
            "stamp": {"type": "integer", "minimum": 0, "description": "stamp: 0–5 are the vector stamps, 100+ emoji"},
            "name": {"type": "string", "description": "A name for the layers list"},
            "around_text": {"type": "string", "description": "Place the mark by this text on the picture (read on this device, as ocr does) instead of giving coordinates: a box around it (rect, ellipse, hide, highlighter), an arrow's head at its left, a counter or a stamp beside it, a text below it. The first line that contains the text, top to bottom; on a recording the frame at from_ms."},
            "padding": {"type": "integer", "minimum": 0, "description": "around_text: room around the text in pixels (6)"},
            "from_ms": {"type": "integer", "minimum": 0, "description": "Recording only: when the mark appears, ms as recorded (the whole recording without times)"},
            "to_ms": {"type": "integer", "minimum": 1, "description": "Recording only: when it goes (not included)"}
        },
        "required": ["kind"]
    })
}

pub(crate) const TOOLS: &[Tool] = &[
    Tool {
        name: "list_marks",
        title: "List the marks",
        description: "The marks of a library document: id, kind, box in screenshot pixels, text, colour, hidden — and the picture's size and crop. Use the ids with update_marks and delete_marks.",
        scope: Some(Scope::LibraryRead),
        read_only: true,
        schema: || obj(json!({"document": doc_arg()}), &["document"]),
    },
    Tool {
        name: "add_marks",
        title: "Add marks",
        description: "Draws marks on a library document — frames, ellipses, arrows, lines, pen trails, text, numbered counters, hidden areas (blur, pixelate, plate), highlighter, stamps — and saves it. Coordinates are screenshot pixels, origin top-left. Returns the new marks' ids and the picture.",
        scope: Some(Scope::LibraryWrite),
        read_only: false,
        schema: || {
            obj(
                json!({"document": doc_arg(), "marks": {"type": "array", "minItems": 1, "items": mark_schema()}}),
                &["document", "marks"],
            )
        },
    },
    Tool {
        name: "update_marks",
        title: "Change marks",
        description: "Changes marks of a library document by their ids (from list_marks or add_marks): move them, set a new box, colour, fill, thickness, opacity, text, font size, show or hide them. Only what is given changes.",
        scope: Some(Scope::LibraryWrite),
        read_only: false,
        schema: || {
            obj(
                json!({
                    "document": doc_arg(),
                    "ids": {"type": "array", "minItems": 1, "items": {"type": "integer"}},
                    "dx": {"type": "integer", "description": "Move right by this many pixels"},
                    "dy": {"type": "integer", "description": "Move down by this many pixels"},
                    "x": {"type": "integer"}, "y": {"type": "integer"},
                    "width": {"type": "integer"}, "height": {"type": "integer"},
                    "color": colour_arg("New colour"),
                    "fill": colour_arg("New fill (\"none\" removes it)"),
                    "line_width": {"type": "integer", "minimum": 1},
                    "opacity": {"type": "integer", "minimum": 10, "maximum": 100},
                    "text": {"type": "string"},
                    "size": {"type": "integer", "minimum": 6},
                    "bold": {"type": "boolean"}, "italic": {"type": "boolean"},
                    "hidden": {"type": "boolean"},
                    "name": {"type": "string"},
                    "from_ms": {"type": "integer", "minimum": 0, "description": "Recording only: when the marks appear, ms as recorded"},
                    "to_ms": {"type": "integer", "minimum": 1, "description": "Recording only: when they go (not included)"},
                    "always": {"type": "boolean", "description": "Recording only: true shows the marks the whole time"}
                }),
                &["document", "ids"],
            )
        },
    },
    Tool {
        name: "delete_marks",
        title: "Delete marks",
        description: "Removes marks of a library document by their ids, or all of them with all=true. The picture under them stays as it was.",
        scope: Some(Scope::LibraryWrite),
        read_only: false,
        schema: || {
            obj(
                json!({
                    "document": doc_arg(),
                    "ids": {"type": "array", "items": {"type": "integer"}},
                    "all": {"type": "boolean"}
                }),
                &["document"],
            )
        },
    },
    Tool {
        name: "crop",
        title: "Crop",
        description: "Sets the frame of a library document: the part that is shown and exported (in pixels of the whole picture). Nothing is thrown away — reset=true shows the whole picture again.",
        scope: Some(Scope::LibraryWrite),
        read_only: false,
        schema: || {
            obj(
                json!({
                    "document": doc_arg(),
                    "x": {"type": "integer"}, "y": {"type": "integer"},
                    "width": {"type": "integer", "minimum": 1}, "height": {"type": "integer", "minimum": 1},
                    "reset": {"type": "boolean"}
                }),
                &["document"],
            )
        },
    },
    Tool {
        name: "rotate",
        title: "Rotate or mirror",
        description: "Turns a library document by a quarter (right, left) or a half turn, or mirrors it (horizontal: left and right swap; vertical: top and bottom). The marks move with the picture.",
        scope: Some(Scope::LibraryWrite),
        read_only: false,
        schema: || {
            obj(
                json!({
                    "document": doc_arg(),
                    "turn": {"type": "string", "enum": ["right", "left", "half"]},
                    "mirror": {"type": "string", "enum": ["horizontal", "vertical"]}
                }),
                &["document"],
            )
        },
    },
    Tool {
        name: "resize",
        title: "Resize the picture or the canvas",
        description: "Scales the picture of a library document (width and/or height in pixels — the other side follows the proportions — or percent), or changes its canvas without scaling (canvas: a box in pixels of the picture; beyond the picture it is filled, inside it is cut).",
        scope: Some(Scope::LibraryWrite),
        read_only: false,
        schema: || {
            obj(
                json!({
                    "document": doc_arg(),
                    "width": {"type": "integer", "minimum": 1}, "height": {"type": "integer", "minimum": 1},
                    "percent": {"type": "number", "exclusiveMinimum": 0},
                    "canvas": {"type": "object", "properties": {
                        "x": {"type": "integer"}, "y": {"type": "integer"},
                        "width": {"type": "integer", "minimum": 1}, "height": {"type": "integer", "minimum": 1}
                    }, "required": ["x", "y", "width", "height"]},
                    "fill": colour_arg("canvas: the colour of what is added (transparent when left out)")
                }),
                &["document"],
            )
        },
    },
    Tool {
        name: "tone",
        title: "Exposure, gamma, contrast",
        description: "Corrects the picture of a library document: exposure in stops (−3…3), gamma (0.3…3, 1 = as it is), contrast (−100…100). reset=true takes the correction off.",
        scope: Some(Scope::LibraryWrite),
        read_only: false,
        schema: || {
            obj(
                json!({
                    "document": doc_arg(),
                    "exposure": {"type": "number"}, "gamma": {"type": "number"},
                    "contrast": {"type": "integer"},
                    "reset": {"type": "boolean"}
                }),
                &["document"],
            )
        },
    },
];

/// Tools that take something away for good (the client may ask before it calls them).
pub(crate) fn destructive(name: &str) -> bool {
    name == "delete_marks"
}

fn colour(v: &Value, k: &str) -> Result<Option<Rgb>, String> {
    let Some(s) = arg_str(v, k) else {
        return Ok(None);
    };
    let named = match s.trim().to_ascii_lowercase().as_str() {
        "red" => Some(Rgb::new(255, 59, 48)),
        "orange" => Some(Rgb::new(255, 149, 0)),
        "yellow" => Some(Rgb::new(255, 214, 10)),
        "green" => Some(Rgb::new(52, 199, 89)),
        "blue" => Some(Rgb::new(61, 123, 245)),
        "violet" | "purple" => Some(Rgb::new(155, 92, 245)),
        "black" => Some(Rgb::new(0, 0, 0)),
        "white" => Some(Rgb::new(255, 255, 255)),
        "grey" | "gray" => Some(Rgb::new(142, 142, 147)),
        _ => None,
    };
    if named.is_some() {
        return Ok(named);
    }
    let h = s.trim().trim_start_matches('#');
    let n = (h.len() == 6)
        .then(|| u32::from_str_radix(h, 16).ok())
        .flatten()
        .ok_or_else(|| format!("«{k}»: «{s}» is not a colour (#RRGGBB or a name)"))?;
    Ok(Some(Rgb::new((n >> 16) as u8, (n >> 8) as u8, n as u8)))
}

fn int(v: &Value, k: &str) -> Result<i32, String> {
    arg_int(v, k)
        .map(|n| znimok_core::clamp_coord(n.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32))
}

fn opt_int(v: &Value, k: &str) -> Result<Option<i32>, String> {
    match v.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(_) => int(v, k).map(Some),
    }
}

fn point(v: &Value, k: &str) -> Result<(i32, i32), String> {
    let a = v
        .get(k)
        .and_then(Value::as_array)
        .filter(|a| a.len() == 2)
        .ok_or_else(|| format!("«{k}» must be [x, y]"))?;
    let n = |i: usize| {
        a[i].as_i64()
            .or_else(|| a[i].as_f64().map(|f| f.round() as i64))
            .map(|n| znimok_core::clamp_coord(n.clamp(-(1 << 30), 1 << 30) as i32))
            .ok_or_else(|| format!("«{k}» must be [x, y]"))
    };
    Ok((n(0)?, n(1)?))
}

fn boxed(m: &Value) -> Result<IRect, String> {
    let (w, h) = (int(m, "width")?, int(m, "height")?);
    if w < 1 || h < 1 {
        return Err("width and height must be positive".into());
    }
    Ok(IRect::new(int(m, "x")?, int(m, "y")?, w, h))
}

/// A mark from its plain description.
fn mark(m: &Value) -> Result<Object, String> {
    let kind = arg_str(m, "kind").ok_or("a mark needs «kind»")?;
    let size = opt_int(m, "size")?;
    let mut o = match kind {
        "rect" => Object::new(boxed(m)?, Data::Rect),
        "ellipse" => Object::new(boxed(m)?, Data::Ellipse),
        "hide" => {
            let mode = match arg_str(m, "mode").unwrap_or("pixelate") {
                "blur" => HideMode::Blur,
                "plate" => HideMode::Plate,
                "pixelate" => HideMode::Pixelate,
                other => return Err(format!("unknown hide mode «{other}»")),
            };
            let strength = opt_int(m, "strength")?.unwrap_or(50).clamp(1, 100) as u8;
            Object::new(boxed(m)?, Data::Hide { mode, strength })
        }
        "highlighter" => {
            let r = boxed(m)?;
            let mut o = Object::new(r, Data::Mark);
            o.style.color = Rgb::new(255, 214, 10);
            o.style.thick = r.h;
            o
        }
        "arrow" | "line" => {
            let (a, b) = (point(m, "from")?, point(m, "to")?);
            let heads = arg_str(m, "heads").unwrap_or(if kind == "arrow" { "end" } else { "none" });
            let (front, back) = match heads {
                "none" => (Head::None, Head::None),
                "end" => (Head::Triangle, Head::None),
                "both" => (Head::Triangle, Head::Triangle),
                other => return Err(format!("unknown heads «{other}»")),
            };
            Object::new(
                IRect::new(a.0, a.1, b.0 - a.0, b.1 - a.1),
                Data::Line {
                    head_front: front,
                    head_back: back,
                    head_size: 1,
                },
            )
        }
        "pen" => {
            let pts = m
                .get("points")
                .and_then(Value::as_array)
                .ok_or("a pen mark needs «points»")?;
            let mut points = Vec::with_capacity(pts.len());
            for p in pts {
                points.push(point(&json!({"p": p}), "p")?);
            }
            if points.len() < 2 {
                return Err("a pen mark needs at least two points".into());
            }
            let (x0, y0) = points
                .iter()
                .fold((i32::MAX, i32::MAX), |a, p| (a.0.min(p.0), a.1.min(p.1)));
            let (x1, y1) = points
                .iter()
                .fold((i32::MIN, i32::MIN), |a, p| (a.0.max(p.0), a.1.max(p.1)));
            Object::new(
                IRect::new(x0, y0, (x1 - x0).max(1), (y1 - y0).max(1)),
                Data::pen(points),
            )
        }
        "text" => {
            let text = arg_str(m, "text").ok_or("a text mark needs «text»")?;
            let align = match arg_str(m, "align").unwrap_or("left") {
                "left" => Align::Left,
                "center" => Align::Center,
                "right" => Align::Right,
                other => return Err(format!("unknown align «{other}»")),
            };
            Object::new(
                IRect::new(int(m, "x")?, int(m, "y")?, 1, 1),
                Data::Text {
                    text: text.to_string(),
                    size: size.unwrap_or(24).clamp(6, 400),
                    bold: m["bold"].as_bool().unwrap_or(false),
                    italic: m["italic"].as_bool().unwrap_or(false),
                    align,
                    box_w: opt_int(m, "box_width")?.unwrap_or(0).max(0),
                },
            )
        }
        "counter" | "stamp" => {
            let d = size.unwrap_or(36).clamp(12, 400);
            let (x, y) = (int(m, "x")?, int(m, "y")?);
            let (data, h) = if kind == "counter" {
                let shape = match arg_str(m, "shape").unwrap_or("circle") {
                    "circle" => CounterShape::Circle,
                    "rounded_box" => CounterShape::RoundedBox,
                    "pin" => CounterShape::Pin,
                    other => return Err(format!("unknown counter shape «{other}»")),
                };
                // A pin is taller than wide, as the app draws it.
                let h = if shape == CounterShape::Pin {
                    d * 13 / 10
                } else {
                    d
                };
                (
                    Data::Counter {
                        seq: 0,
                        group: 1,
                        start: 1,
                        shape,
                    },
                    h,
                )
            } else {
                (
                    Data::Stamp {
                        id: opt_int(m, "stamp")?.unwrap_or(0).max(0) as u32,
                    },
                    d,
                )
            };
            let mut o = Object::new(IRect::new(x - d / 2, y - h / 2, d, h), data);
            o.style.thick = d;
            o
        }
        other => return Err(format!("unknown mark kind «{other}»")),
    };
    if let Some(c) = colour(m, "color")? {
        o.style.color = c;
    }
    if let Some(c) = colour(m, "fill")? {
        o.style.color2 = Some(c);
    }
    if let Some(t) = opt_int(m, "line_width")? {
        o.style.thick = t.clamp(1, 400);
    }
    if let Some(a) = opt_int(m, "opacity")? {
        o.style.alpha = a.clamp(10, 100) as u8;
    }
    if let Some(n) = arg_str(m, "name") {
        o.name = Some(n.to_string());
    }
    Ok(o)
}

fn hex(c: Rgb) -> String {
    format!("#{:02X}{:02X}{:02X}", c.r, c.g, c.b)
}

fn mark_json(o: &Object) -> Value {
    let kind = serde_json::to_value(&o.data)
        .ok()
        .and_then(|v| v["kind"].as_str().map(str::to_string))
        .unwrap_or_default();
    // The highlighter is «mark» inside; agents know it by the name they draw it with.
    let kind = if kind == "mark" {
        "highlighter".to_string()
    } else {
        kind
    };
    let mut v = json!({
        "id": o.id, "kind": kind,
        "x": o.rect.x, "y": o.rect.y, "width": o.rect.w, "height": o.rect.h,
        "color": hex(o.style.color), "hidden": o.hidden,
    });
    if let Data::Text { text, size, .. } = &o.data {
        v["text"] = json!(text);
        v["size"] = json!(size);
    }
    if let Some(c) = o.style.color2 {
        v["fill"] = json!(hex(c));
    }
    if o.rot != 0 {
        v["rotation"] = json!(o.rot);
    }
    if o.group != 0 {
        v["group"] = json!(o.group);
    }
    if let Some(n) = &o.name {
        v["name"] = json!(n);
    }
    v
}

fn ids(args: &Value) -> Result<Vec<ObjectId>, String> {
    let a = args["ids"]
        .as_array()
        .ok_or("«ids» must be an array of mark ids")?;
    a.iter()
        .map(|v| {
            v.as_u64()
                .and_then(|n| ObjectId::try_from(n).ok())
                .ok_or_else(|| format!("«{v}» is not a mark id"))
        })
        .collect()
}

fn known(doc: &Document, ids: &[ObjectId]) -> Result<(), String> {
    match ids.iter().find(|id| doc.get(**id).is_none()) {
        Some(id) => Err(format!("no mark {id} in this document (see list_marks)")),
        None => Ok(()),
    }
}

/// Applies the commands, saves, answers with the document and its picture.
pub(crate) fn apply(
    agent: &Agent,
    args: &Value,
    cmds: impl FnOnce(&Document) -> Result<Vec<Command>, String>,
) -> Result<Output, String> {
    apply_timed(agent, args, cmds, |_| Vec::new())
}

/// Where a phrase stands among the lines read (ZK-309): the first line, top to bottom, that
/// contains it (case folded); the union of the words' boxes that cover it when the engine gives
/// words, else the line's box cut by the phrase's share of the characters.
pub(crate) fn phrase_box(lines: &[znimok_models::ocr::Line], needle: &str) -> Option<OcrRect> {
    let needle = znimok_models::ocr::fold_for_find(needle.trim());
    if needle.is_empty() {
        return None;
    }
    let n_chars = needle.chars().count();
    for line in lines {
        // Twins and case folded one character for one (ZK-314): positions still match.
        let text = znimok_models::ocr::fold_for_find(&line.text);
        let Some(byte) = text.find(&needle) else {
            continue;
        };
        let start = text[..byte].chars().count();
        let end = start + n_chars;
        if !line.words.is_empty() {
            // The words joined by one space are the line's text; a word is in when its characters
            // overlap the phrase's.
            let mut at = 0;
            let mut found: Option<OcrRect> = None;
            for w in &line.words {
                let len = w.text.chars().count();
                if at < end && at + len > start {
                    found = Some(match found {
                        Some(f) => f.union(w.rect),
                        None => w.rect,
                    });
                }
                at += len + 1;
            }
            if let Some(f) = found {
                return Some(f);
            }
        }
        let total = text.chars().count().max(1) as f32;
        let r = line.rect;
        return Some(OcrRect {
            x: r.x + r.w * start as f32 / total,
            y: r.y,
            w: (r.w * n_chars as f32 / total).max(1.0),
            h: r.h,
        });
    }
    None
}

/// The marks with `around_text` get their place (ZK-309): the text of the picture (the frame at
/// the mark's from_ms on a recording) read once per frame, the phrase found, the mark put around
/// it as its kind wants. The others pass as they are.
fn around_text(agent: &Agent, args: &Value) -> Result<Value, String> {
    let mut out = args.clone();
    let Some(marks) = out["marks"].as_array_mut() else {
        return Ok(out);
    };
    if !marks.iter().any(|m| m.get("around_text").is_some()) {
        return Ok(out);
    }
    let (doc, path) = agent.doc(args)?;
    let engine = znimok_models::ocr::system().ok_or("no text recognition on this system")?;
    let mut read: std::collections::HashMap<i64, znimok_models::ocr::OcrResult> =
        Default::default();
    for (i, m) in marks.iter_mut().enumerate() {
        let Some(needle) = m["around_text"]
            .as_str()
            .map(str::trim)
            .filter(|t| !t.is_empty())
        else {
            continue;
        };
        let needle = needle.to_string();
        let ms = m["from_ms"].as_i64().unwrap_or(0);
        if let std::collections::hash_map::Entry::Vacant(e) = read.entry(ms) {
            let r = crate::vexport::picture(&doc, &path, &json!({"at_ms": ms}))?;
            let img = znimok_models::Rgba::new(r.width, r.height, r.rgba).ok_or("empty picture")?;
            e.insert(engine.recognize(&img, &[]).map_err(|e| e.to_string())?);
        }
        let res = &read[&ms];
        let Some(b) = phrase_box(&res.lines, &needle) else {
            return Err(format!(
                "mark {i}: «{needle}» is not on the picture ({} lines read; ocr shows them)",
                res.lines.len()
            ));
        };
        let pad = opt_int(m, "padding")?.unwrap_or(6).max(0) as f32;
        let (x0, y0) = ((b.x - pad).floor() as i64, (b.y - pad).floor() as i64);
        let (x1, y1) = (
            (b.x + b.w + pad).ceil() as i64,
            (b.y + b.h + pad).ceil() as i64,
        );
        let cy = (b.y + b.h / 2.0).round() as i64;
        let size = opt_int(m, "size")?.unwrap_or(36).max(6) as i64;
        match arg_str(m, "kind").unwrap_or("") {
            "rect" | "ellipse" | "hide" | "highlighter" => {
                m["x"] = json!(x0);
                m["y"] = json!(y0);
                m["width"] = json!((x1 - x0).max(1));
                m["height"] = json!((y1 - y0).max(1));
            }
            "arrow" | "line" => {
                m["to"] = json!([x0, cy]);
                if m.get("from").is_none() {
                    m["from"] = json!([(x0 - 120).max(0), (cy + 80).max(0)]);
                }
            }
            "counter" | "stamp" => {
                m["x"] = json!((x0 - size / 2).max(size / 2));
                m["y"] = json!(cy);
            }
            "text" => {
                m["x"] = json!(x0.max(0));
                m["y"] = json!(y1);
            }
            other => {
                return Err(format!(
                    "mark {i}: around_text places rect, ellipse, hide, highlighter, arrow, line, counter, stamp and text, not «{other}»"
                ));
            }
        }
    }
    Ok(out)
}

/// [`apply`], then the times of marks on a recording (ZK-239): `times` gets the ids of the
/// marks just made and answers which marks take which time arguments.
fn apply_timed(
    agent: &Agent,
    args: &Value,
    cmds: impl FnOnce(&Document) -> Result<Vec<Command>, String>,
    times: impl FnOnce(&[ObjectId]) -> Vec<(ObjectId, Value)>,
) -> Result<Output, String> {
    let (doc, path) = agent.doc(args)?;
    let cmds = cmds(&doc)?;
    let mut ed = Editor::new(doc);
    let mut created = Vec::new();
    for c in cmds {
        let a = ed.apply(c).map_err(|e| e.to_string())?;
        created.extend(a.created);
    }
    crate::videdit::set_times(&mut ed, &path, &times(&created))?;
    library::save(&path, &ed.doc)?;
    let mut s = summary(&ed.doc, &path);
    if !created.is_empty() {
        s["created"] = json!(created);
    }
    let r = library::render(&ed.doc, 1.0);
    Ok(Output::ok(s, image(&r).into_iter().collect()))
}

/// `None`: not an editing tool.
pub(crate) fn run(agent: &Agent, name: &str, args: &Value) -> Option<Result<Output, String>> {
    Some(match name {
        "list_marks" => agent.doc(args).map(|(doc, path)| {
            let mut s = summary(&doc, &path);
            let (w, h) = doc.image_size();
            s["picture"] = json!({"width": w, "height": h});
            s["crop"] = match doc.crop {
                Some(c) => json!({"x": c.x, "y": c.y, "width": c.w, "height": c.h}),
                None => Value::Null,
            };
            s["marks"] = json!(doc.objects.iter().map(mark_json).collect::<Vec<_>>());
            Output::ok(s, vec![])
        }),
        "add_marks" => (|| {
            let args = &around_text(agent, args)?;
            apply_timed(
                agent,
                args,
                |_| {
                    let marks = args["marks"].as_array().ok_or("«marks» must be an array")?;
                    marks
                        .iter()
                        .enumerate()
                        .map(|(i, m)| {
                            mark(m)
                                .map(|object| Command::AddObject {
                                    object,
                                    select: false,
                                    merge: None,
                                })
                                .map_err(|e| format!("mark {i}: {e}"))
                        })
                        .collect()
                },
                // One new mark per mark given, in order: each takes its own from_ms / to_ms.
                |created| {
                    created
                        .iter()
                        .copied()
                        .zip(args["marks"].as_array().into_iter().flatten().cloned())
                        .collect()
                },
            )
        })(),
        "update_marks" => apply_timed(
            agent,
            args,
            |doc| {
                let ids = ids(args)?;
                known(doc, &ids)?;
                let timed = !args["from_ms"].is_null()
                    || !args["to_ms"].is_null()
                    || args["always"].as_bool().is_some();
                let mut out = Vec::new();
                let (dx, dy) = (
                    opt_int(args, "dx")?.unwrap_or(0),
                    opt_int(args, "dy")?.unwrap_or(0),
                );
                if dx != 0 || dy != 0 {
                    out.push(Command::MoveObjects {
                        ids: ids.clone(),
                        dx,
                        dy,
                        merge: None,
                    });
                }
                // A new box: each side given replaces that side of every listed mark.
                let side = |k: &str| opt_int(args, k);
                let (x, y, w, h) = (side("x")?, side("y")?, side("width")?, side("height")?);
                if x.is_some() || y.is_some() || w.is_some() || h.is_some() {
                    for id in &ids {
                        let r = doc.get(*id).map(|o| o.rect).unwrap_or_default();
                        out.push(Command::UpdateObjects {
                            ids: vec![*id],
                            patch: ObjectPatch {
                                rect: Some(IRect::new(
                                    x.unwrap_or(r.x),
                                    y.unwrap_or(r.y),
                                    w.unwrap_or(r.w),
                                    h.unwrap_or(r.h),
                                )),
                                ..Default::default()
                            },
                            merge: None,
                        });
                    }
                }
                let mut patch = ObjectPatch::default();
                let mut style = znimok_core::command::StylePatch::default();
                let mut styled = false;
                if let Some(c) = colour(args, "color")? {
                    style.color = Some(c);
                    styled = true;
                }
                if arg_str(args, "fill").is_some_and(|f| f.eq_ignore_ascii_case("none")) {
                    style.color2 = Some(None);
                    styled = true;
                } else if let Some(c) = colour(args, "fill")? {
                    style.color2 = Some(Some(c));
                    styled = true;
                }
                if let Some(t) = opt_int(args, "line_width")? {
                    style.thick = Some(t.clamp(1, 400));
                    styled = true;
                }
                if let Some(a) = opt_int(args, "opacity")? {
                    style.alpha = Some(a.clamp(10, 100) as u8);
                    styled = true;
                }
                if styled {
                    patch.style = Some(style);
                }
                patch.text = arg_str(args, "text").map(str::to_string);
                patch.size = opt_int(args, "size")?.map(|s| s.clamp(6, 400));
                patch.bold = args["bold"].as_bool();
                patch.italic = args["italic"].as_bool();
                patch.hidden = args["hidden"].as_bool();
                if let Some(n) = arg_str(args, "name") {
                    patch.name = Some(Some(n.to_string()));
                }
                if patch != ObjectPatch::default() {
                    out.push(Command::UpdateObjects {
                        ids,
                        patch,
                        merge: None,
                    });
                }
                if out.is_empty() && !timed {
                    return Err("nothing to change: give dx / dy, a box, a colour, text…".into());
                }
                Ok(out)
            },
            |_| {
                // The times apply to every listed mark (ids were checked above).
                ids(args)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|id| (id, args.clone()))
                    .collect()
            },
        ),
        "delete_marks" => apply(agent, args, |doc| {
            let ids = if args["all"].as_bool().unwrap_or(false) {
                doc.objects.iter().map(|o| o.id).collect()
            } else {
                let ids = ids(args)?;
                known(doc, &ids)?;
                ids
            };
            Ok(vec![Command::DeleteObjects { ids }])
        }),
        "crop" => apply(agent, args, |_| {
            let rect = if args["reset"].as_bool().unwrap_or(false) {
                None
            } else {
                Some(boxed(args)?)
            };
            Ok(vec![Command::SetCrop { rect }])
        }),
        "rotate" => apply(agent, args, |_| {
            let mut out = Vec::new();
            match arg_str(args, "turn") {
                Some("right") => out.push(Command::Rotate { quarters: 1 }),
                Some("left") => out.push(Command::Rotate { quarters: 3 }),
                Some("half") => out.push(Command::Rotate { quarters: 2 }),
                Some(other) => return Err(format!("unknown turn «{other}»")),
                None => {}
            }
            match arg_str(args, "mirror") {
                Some("horizontal") => out.push(Command::Mirror),
                Some("vertical") => out.push(Command::MirrorVertical),
                Some(other) => return Err(format!("unknown mirror «{other}»")),
                None => {}
            }
            if out.is_empty() {
                return Err(
                    "give «turn» (right, left, half) or «mirror» (horizontal, vertical)".into(),
                );
            }
            Ok(out)
        }),
        "resize" => apply(agent, args, |doc| {
            if let Some(c) = args.get("canvas").filter(|c| c.is_object()) {
                return Ok(vec![Command::ResizeCanvas {
                    rect: boxed(c)?,
                    fill: colour(args, "fill")?,
                }]);
            }
            let (w0, h0) = doc.image_size();
            let (w0, h0) = (f64::from(w0.max(1)), f64::from(h0.max(1)));
            let (w, h) = (opt_int(args, "width")?, opt_int(args, "height")?);
            let (w, h) = match (w, h, args["percent"].as_f64()) {
                (Some(w), Some(h), _) => (f64::from(w), f64::from(h)),
                (Some(w), None, _) => (f64::from(w), (f64::from(w) * h0 / w0).round()),
                (None, Some(h), _) => ((f64::from(h) * w0 / h0).round(), f64::from(h)),
                (None, None, Some(p)) if p > 0.0 => {
                    ((w0 * p / 100.0).round(), (h0 * p / 100.0).round())
                }
                _ => return Err("give «width» and/or «height», «percent», or «canvas»".into()),
            };
            Ok(vec![Command::ResizeImage {
                width: w.clamp(1.0, 32767.0) as u32,
                height: h.clamp(1.0, 32767.0) as u32,
                scale_text: true,
            }])
        }),
        "tone" => apply(agent, args, |_| {
            if args["reset"].as_bool().unwrap_or(false) {
                return Ok(vec![Command::ResetTone]);
            }
            let exposure = args["exposure"].as_f64().map(|v| v.clamp(-3.0, 3.0) as f32);
            let gamma = args["gamma"].as_f64().map(|v| v.clamp(0.3, 3.0) as f32);
            let contrast = opt_int(args, "contrast")?.map(|c| c.clamp(-100, 100));
            if exposure.is_none() && gamma.is_none() && contrast.is_none() {
                return Err("give «exposure», «gamma» or «contrast» (or reset=true)".into());
            }
            Ok(vec![Command::SetTone {
                exposure,
                gamma,
                contrast,
                merge: None,
            }])
        }),
        _ => return None,
    })
}

#[cfg(test)]
mod around_tests {
    use super::phrase_box;
    use znimok_models::ocr::{Line, Rect, Word};

    fn word(t: &str, x: f32, w: f32) -> Word {
        Word {
            text: t.into(),
            rect: Rect {
                x,
                y: 100.0,
                w,
                h: 16.0,
            },
        }
    }

    /// With words: the union of the words the phrase covers; without: the line's share (ZK-309).
    #[test]
    fn a_phrase_is_placed_by_its_words_or_its_share() {
        let lines = vec![
            Line {
                text: "Help every member".into(),
                rect: Rect {
                    x: 20.0,
                    y: 40.0,
                    w: 300.0,
                    h: 16.0,
                },
                confidence: None,
                words: vec![],
            },
            Line {
                text: "Request a demo See how it works".into(),
                rect: Rect {
                    x: 40.0,
                    y: 100.0,
                    w: 330.0,
                    h: 16.0,
                },
                confidence: None,
                words: vec![
                    word("Request", 40.0, 60.0),
                    word("a", 105.0, 10.0),
                    word("demo", 120.0, 45.0),
                    word("See", 220.0, 30.0),
                    word("how", 255.0, 30.0),
                    word("it", 290.0, 15.0),
                    word("works", 310.0, 60.0),
                ],
            },
        ];
        let b = phrase_box(&lines, "see how it works").unwrap();
        assert_eq!((b.x, b.y, b.w, b.h), (220.0, 100.0, 150.0, 16.0));
        let b = phrase_box(&lines, "Request a demo").unwrap();
        assert_eq!((b.x, b.w), (40.0, 125.0));
        // No words: «member» is the last 6 of 17 characters.
        let b = phrase_box(&lines, "member").unwrap();
        assert!((b.x - (20.0 + 300.0 * 11.0 / 17.0)).abs() < 0.01, "{b:?}");
        assert!((b.w - 300.0 * 6.0 / 17.0).abs() < 0.01);
        assert!(phrase_box(&lines, "Find a Job").is_none());
        // Latin read as its Cyrillic twins is still found (ZK-314).
        let twins = vec![Line {
            text: "Ноте Ргісіпд Find а Job".into(),
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 230.0,
                h: 16.0,
            },
            confidence: None,
            words: vec![],
        }];
        assert!(phrase_box(&twins, "Find a Job").is_some());
        assert!(phrase_box(&twins, "home").is_some());
        assert!(phrase_box(&twins, "Pricing").is_some());
        assert!(phrase_box(&lines, "  ").is_none());
    }
}
