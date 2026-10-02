//! Reading a recording (ZK-235): what it is (`video_info`), and the browser's DevTools log it
//! carries (`devlog_summary`, `devlog_get`) — so an agent can read a bug report the way a
//! developer does: the errors, the failed requests, what was clicked and when. And `find_text`:
//! where a word stands on a picture, to put a mark by it. All from the file, on this computer;
//! sensitive values of the log are hidden as the person's settings say.

use crate::library;
use crate::permissions::Scope;
use crate::tools::{Agent, Output, Tool, arg_int, arg_str, doc_arg, obj};
use serde_json::{Value, json};
use znimok_format::video::MouseButton;

pub(crate) const TOOLS: &[Tool] = &[
    Tool {
        name: "video_info",
        title: "About a recording",
        description: "What a recording in the library is: length, size, frame rate, what was trimmed or cut, its sound tracks, the marks with their times, the clicks, and whether it carries the browser's DevTools log.",
        scope: Some(Scope::LibraryRead),
        read_only: true,
        schema: || obj(json!({"document": doc_arg()}), &["document"]),
    },
    Tool {
        name: "devlog_summary",
        title: "The DevTools log at a glance",
        description: "A recording's browser log in short: how many events of each kind, the errors, the failed requests, the pages visited, the dataLayer events — each with its time in the video. Start here, then devlog_get for the details.",
        scope: Some(Scope::LibraryRead),
        read_only: true,
        schema: || obj(json!({"document": doc_arg()}), &["document"]),
    },
    Tool {
        name: "devlog_get",
        title: "Read the DevTools log",
        description: "Events of a recording's browser log, in time order: console lines, errors, network requests, WebSocket frames, navigations, dataLayer pushes. Filter by kinds, errors, text, time in the video; rows are short — give index for one event whole (headers, bodies, stack). Sensitive values are hidden as the person's settings say.",
        scope: Some(Scope::LibraryRead),
        read_only: true,
        schema: || {
            obj(
                json!({
                    "document": doc_arg(),
                    "kinds": {"type": "array", "items": {"type": "string", "enum": ["console", "error", "network", "websocket", "navigation", "datalayer", "other"]}},
                    "errors_only": {"type": "boolean", "description": "Errors and failed requests only"},
                    "query": {"type": "string", "description": "Text to find in the event (URL, message, payload)"},
                    "from_ms": {"type": "integer", "minimum": 0}, "to_ms": {"type": "integer", "minimum": 0},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 500},
                    "offset": {"type": "integer", "minimum": 0},
                    "index": {"type": "integer", "minimum": 0, "description": "One event by its index «i», whole"}
                }),
                &["document"],
            )
        },
    },
    Tool {
        name: "find_text",
        title: "Find text on a picture",
        description: "Where a word or a phrase stands on a library document: the lines that contain it (read on this computer) with their boxes in pixels — to frame it, point an arrow at it or hide it.",
        scope: Some(Scope::LibraryRead),
        read_only: true,
        schema: || {
            obj(
                json!({
                    "document": doc_arg(),
                    "text": {"type": "string"},
                    "languages": {"type": "array", "items": {"type": "string"}, "description": "BCP 47 tags in order of preference, e.g. [\"uk\", \"en\"]"}
                }),
                &["document", "text"],
            )
        },
    },
];

fn video(
    agent: &Agent,
    args: &Value,
) -> Result<(znimok_core::Document, znimok_format::VideoPart), String> {
    let d = arg_str(args, "document").ok_or("«document» is required")?;
    let path = agent
        .lib
        .resolve(d)
        .ok_or_else(|| format!("no document «{d}» in the library"))?;
    let (doc, v) =
        znimok_format::open_parts(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let v = v.ok_or_else(|| format!("«{}» is a screenshot, not a recording", doc.name))?;
    Ok((doc, v))
}

fn frames_ms(frames: i64, fps: f64) -> i64 {
    (frames as f64 * 1000.0 / fps.max(1e-6)).round() as i64
}

/// The log's events as the extension wrote them, each with `at` (seconds of the recording),
/// sensitive values hidden unless the person's settings say never.
pub(crate) fn events(part: &znimok_format::VideoPart) -> (Vec<Value>, usize) {
    let Some(log) = part.video.devlog.as_ref() else {
        return (Vec::new(), 0);
    };
    let mut ev = znimok_report::log_events(log, |ms| Some(f64::from(ms) / 1000.0));
    let prefs = znimok_settings::Store::open_default()
        .map(|s| s.get().video)
        .unwrap_or_default();
    let hidden = if prefs.hide_on_export == znimok_settings::HideOnExport::Never {
        0
    } else {
        znimok_report::mask::events(&mut ev, &znimok_report::mask::Rules::new(&prefs.hide_keys))
    };
    (ev, hidden)
}

fn kind(e: &Value) -> &'static str {
    match e["k"].as_str().unwrap_or("") {
        "console" | "log" => "console",
        "error" => "error",
        "net" => "network",
        "ws" => "websocket",
        "nav" => "navigation",
        "dl" => "datalayer",
        _ => "other",
    }
}

fn level(e: &Value) -> &'static str {
    match e["s"].as_i64() {
        Some(2) => "error",
        Some(1) => "warning",
        _ => "info",
    }
}

fn failed(e: &Value) -> bool {
    e["s"].as_i64() == Some(2)
        || e["err"].is_string()
        || (kind(e) == "network" && e["status"].as_i64().is_some_and(|s| s >= 400))
}

fn at_ms(e: &Value) -> i64 {
    (e["at"].as_f64().unwrap_or(0.0) * 1000.0).round() as i64
}

fn short(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let cut: String = s.chars().take(n).collect();
        format!("{cut}…")
    }
}

/// One event in a line: what a person sees in the list of the log.
fn row(i: usize, e: &Value) -> Value {
    let mut v = json!({"i": i, "at_ms": at_ms(e), "kind": kind(e), "level": level(e)});
    let text = |k: &str, n: usize| e[k].as_str().map(|s| short(s, n));
    match kind(e) {
        "network" => {
            v["method"] = e["method"].clone();
            v["status"] = e["status"].clone();
            v["url"] = json!(text("url", 300));
            if let Some(d) = e["dur"].as_f64() {
                v["duration_ms"] = json!(d);
            }
            if e["err"].is_string() {
                v["error"] = e["err"].clone();
            }
        }
        "websocket" => {
            v["direction"] = e["dir"].clone();
            v["data"] = json!(text("data", 300));
        }
        "navigation" => v["url"] = json!(text("url", 300)),
        "datalayer" => {
            v["event"] = e["ev"].clone();
            v["data"] = json!(short(&e["data"].to_string(), 500));
        }
        _ => {
            v["text"] = json!(text("text", 500));
            if e["src"].is_string() {
                v["source"] = json!(text("src", 200));
            }
        }
    }
    v
}

/// The whole event, its long strings cut at 64 KB.
fn whole(i: usize, e: &Value) -> Value {
    fn cut(v: &mut Value) {
        match v {
            Value::String(s) if s.len() > 65_536 => {
                let keep: String = s.chars().take(65_536).collect();
                *s = format!("{keep}… ({} bytes more)", s.len() - keep.len());
            }
            Value::Array(a) => a.iter_mut().for_each(cut),
            Value::Object(o) => o.values_mut().for_each(cut),
            _ => {}
        }
    }
    let mut v = e.clone();
    cut(&mut v);
    v["i"] = json!(i);
    v["at_ms"] = json!(at_ms(e));
    v["kind"] = json!(kind(e));
    v["level"] = json!(level(e));
    v
}

/// `None`: not one of these tools.
pub(crate) fn run(agent: &Agent, name: &str, args: &Value) -> Option<Result<Output, String>> {
    Some(match name {
        "video_info" => video(agent, args).map(|(doc, part)| {
            let v = &part.video;
            let fps = f64::from(v.info.fps_milli) / 1000.0;
            let cuts: Vec<Value> = v
                .edit
                .parts
                .iter()
                .filter(|p| p.off)
                .map(|p| json!({"from_ms": frames_ms(i64::from(p.a), fps), "to_ms": frames_ms(i64::from(p.b), fps)}))
                .collect();
            let clicks: Vec<Value> = v
                .mouse
                .iter()
                .filter(|m| m.down && m.button != MouseButton::Move)
                .take(500)
                .map(|m| {
                    json!({"at_ms": m.ms, "x": m.x, "y": m.y, "button": match m.button {
                        MouseButton::Right => "right",
                        MouseButton::Middle => "middle",
                        _ => "left",
                    }})
                })
                .collect();
            let marks: Vec<Value> = doc
                .objects
                .iter()
                .map(|o| {
                    let mut m = json!({"id": o.id});
                    if let Some((a, b)) = v.mark_spans.get(&o.id) {
                        m["from_ms"] = json!(frames_ms(i64::from(*a), fps));
                        m["to_ms"] = json!(frames_ms(i64::from(*b), fps));
                    }
                    m
                })
                .collect();
            Output::ok(
                json!({
                    "id": doc.id.to_string(), "name": doc.name,
                    "duration_ms": v.info.duration_hns / 10_000,
                    "width": v.info.width, "height": v.info.height,
                    "fps": fps, "frames": v.info.frames,
                    "trim": {"from_ms": frames_ms(i64::from(v.edit.in_frame), fps),
                             "to_ms": frames_ms(i64::from(v.edit.out_frame), fps), "cuts": cuts},
                    "export_size": v.out_size.map(|(w, h)| json!({"width": w, "height": h})),
                    "sound": v.audio.iter().map(|t| json!({
                        "label": t.label, "muted": t.muted, "volume": t.volume,
                        "source": if t.source == znimok_format::video::AudioSource::Microphone { "microphone" } else { "system" },
                    })).collect::<Vec<_>>(),
                    "marks": marks,
                    "clicks": clicks,
                    "devtools_log": v.devlog.as_ref().map(|l| json!({"events": l.events.len()})),
                }),
                vec![],
            )
        }),
        "devlog_summary" => video(agent, args).and_then(|(doc, part)| {
            let (ev, hidden) = events(&part);
            if part.video.devlog.is_none() {
                return Err(format!("«{}» has no browser log (record with the Znimok extension to get one)", doc.name));
            }
            let mut by_kind = std::collections::BTreeMap::<&str, usize>::new();
            let mut dl = std::collections::BTreeMap::<String, usize>::new();
            for e in &ev {
                *by_kind.entry(kind(e)).or_default() += 1;
                if kind(e) == "datalayer" {
                    *dl.entry(e["ev"].as_str().unwrap_or("").to_string()).or_default() += 1;
                }
            }
            let pick = |f: &dyn Fn(&Value) -> bool, n: usize| -> Vec<Value> {
                ev.iter().enumerate().filter(|(_, e)| f(e)).take(n).map(|(i, e)| row(i, e)).collect()
            };
            Ok(Output::ok(
                json!({
                    "events": ev.len(),
                    "by_kind": by_kind,
                    "errors": pick(&|e| kind(e) != "network" && e["s"].as_i64() == Some(2), 20),
                    "failed_requests": pick(&|e| kind(e) == "network" && failed(e), 20),
                    "navigations": pick(&|e| kind(e) == "navigation", 20),
                    "datalayer_events": dl,
                    "clicks": part.video.mouse.iter().filter(|m| m.down && m.button != MouseButton::Move).count(),
                    "duration_ms": part.video.info.duration_hns / 10_000,
                    "hidden_values": hidden,
                }),
                vec![],
            ))
        }),
        "devlog_get" => video(agent, args).and_then(|(doc, part)| {
            if part.video.devlog.is_none() {
                return Err(format!("«{}» has no browser log", doc.name));
            }
            let (ev, hidden) = events(&part);
            if args.get("index").is_some_and(|v| !v.is_null()) {
                let i = usize::try_from(arg_int(args, "index")?).map_err(|_| "«index» must not be negative")?;
                let e = ev.get(i).ok_or_else(|| format!("no event {i} (the log has {})", ev.len()))?;
                return Ok(Output::ok(json!({"event": whole(i, e)}), vec![]));
            }
            let kinds: Option<Vec<String>> = args["kinds"].as_array().map(|a| {
                a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()
            });
            let errors = args["errors_only"].as_bool().unwrap_or(false);
            let q = arg_str(args, "query").map(str::to_lowercase).filter(|q| !q.is_empty());
            let (from, to) = (args["from_ms"].as_i64(), args["to_ms"].as_i64());
            let matching: Vec<(usize, &Value)> = ev
                .iter()
                .enumerate()
                .filter(|(_, e)| kinds.as_ref().is_none_or(|k| k.iter().any(|x| x == kind(e))))
                .filter(|(_, e)| !errors || failed(e))
                .filter(|(_, e)| from.is_none_or(|t| at_ms(e) >= t) && to.is_none_or(|t| at_ms(e) < t))
                .filter(|(_, e)| q.as_ref().is_none_or(|q| e.to_string().to_lowercase().contains(q)))
                .collect();
            let limit = args["limit"].as_u64().unwrap_or(100).clamp(1, 500) as usize;
            let offset = args["offset"].as_u64().unwrap_or(0) as usize;
            let rows: Vec<Value> = matching.iter().skip(offset).take(limit).map(|(i, e)| row(*i, e)).collect();
            Ok(Output::ok(
                json!({
                    "matching": matching.len(), "offset": offset, "returned": rows.len(),
                    "events": rows, "hidden_values": hidden,
                }),
                vec![],
            ))
        }),
        "find_text" => (|| {
            let (doc, _) = agent.doc(args)?;
            let needle = arg_str(args, "text").map(str::trim).filter(|t| !t.is_empty()).ok_or("«text» is required")?;
            let langs: Vec<String> = args["languages"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            let langs: Vec<&str> = langs.iter().map(String::as_str).collect();
            let r = library::render(&doc, 1.0);
            let img = znimok_models::Rgba::new(r.width, r.height, r.rgba).ok_or("empty picture")?;
            let engine = znimok_models::ocr::system().ok_or("no text recognition on this system")?;
            let res = engine.recognize(&img, &langs).map_err(|e| e.to_string())?;
            let n = needle.to_lowercase();
            let found: Vec<Value> = res
                .lines
                .iter()
                .filter(|l| l.text.to_lowercase().contains(&n))
                .map(|l| json!({"text": l.text, "x": l.rect.x, "y": l.rect.y, "width": l.rect.w, "height": l.rect.h}))
                .collect();
            Ok(Output::ok(json!({"found": found, "lines_read": res.lines.len()}), vec![]))
        })(),
        _ => return None,
    })
}
