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
            "name": {"type": "string", "description": "A name for the layers list"}
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
                    "name": {"type": "string"}
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
    let (doc, path) = agent.doc(args)?;
    let cmds = cmds(&doc)?;
    let mut ed = Editor::new(doc);
    let mut created = Vec::new();
    for c in cmds {
        let a = ed.apply(c).map_err(|e| e.to_string())?;
        created.extend(a.created);
    }
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
        "add_marks" => apply(agent, args, |_| {
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
        }),
        "update_marks" => apply(agent, args, |doc| {
            let ids = ids(args)?;
            known(doc, &ids)?;
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
            if out.is_empty() {
                return Err("nothing to change: give dx / dy, a box, a colour, text…".into());
            }
            Ok(out)
        }),
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
