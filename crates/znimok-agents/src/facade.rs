//! The tools as the client sees them (ZK-250): one per job, reads apart from writes — a client
//! asks the person about each tool it calls for the first time, and forty of them were forty
//! questions. The older, finer tools stay as the handlers these dispatch to; only this list is
//! published.
//!
//! Reads: `list_targets`, `library_search`, `library_get`, `list_marks`, `video_info`, `devlog`,
//! `video_frames`, `ocr`, `read_codes`, `record_status`, `app_state`, `share_targets`. Writes:
//! `capture`, `record`, `marks`, `transform`, `annotate`, `redact_pii`, `export`, `set_meta`,
//! `library_edit`, `hand_over`, `share`; `library_delete` alone is destructive.

use crate::permissions::Scope;
use crate::tools::{Agent, Output, Tool, arg_str, doc_arg, obj};
use serde_json::{Value, json};

/// The properties and the required names of an inner tool's schema, without `document`.
fn inner(list: &[Tool], name: &str) -> (Value, Vec<String>) {
    let t = list.iter().find(|t| t.name == name).expect("an inner tool");
    let s = (t.schema)();
    let mut props = s["properties"].clone();
    props.as_object_mut().map(|o| o.remove("document"));
    let req = s["required"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str())
                .filter(|k| *k != "document")
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();
    (props, req)
}

/// An inner tool's schema as one optional object property.
fn part(list: &[Tool], name: &str, what: &str) -> Value {
    let (props, req) = inner(list, name);
    json!({"type": "object", "description": what, "properties": props, "required": req, "additionalProperties": false})
}

fn with_doc(mut props: Value, required: &[&str]) -> Value {
    props["document"] = doc_arg();
    let mut req = vec!["document"];
    req.extend_from_slice(required);
    obj(props, &req)
}

pub(crate) const TOOLS: &[Tool] = &[
    Tool {
        name: "list_targets",
        title: "Displays and windows",
        description: "What can be captured or recorded: the displays (id, name, bounds, scale, primary) and the visible windows in front-to-back order (id, title, app, bounds).",
        scope: Some(Scope::Capture),
        read_only: true,
        schema: || obj(json!({}), &[]),
    },
    Tool {
        name: "capture",
        title: "Take a screenshot",
        description: "Screenshot of a display (the primary one by default), a window (id from list_targets), the window in front (active_window), or a region in desktop units. Saved as a new library document; returns the picture and its id. delay_seconds (up to 30) gives the person time to bring the right window forward.",
        scope: Some(Scope::Capture),
        read_only: false,
        schema: || {
            let mut props = json!({
                "target": {"type": "string", "enum": ["screen", "window", "active_window", "region"], "description": "What to capture"},
                "display": {"type": "string", "description": "Display id (target screen); the primary one by default"},
                "window": {"type": "integer", "description": "Window id (target window)"},
                "x": {"type": "integer"}, "y": {"type": "integer"},
                "width": {"type": "integer", "minimum": 1}, "height": {"type": "integer", "minimum": 1},
            });
            props["delay_seconds"] = crate::apptools::delay_arg();
            obj(props, &["target"])
        },
    },
    Tool {
        name: "record",
        title: "Record the screen",
        description: "action start: begins a screen recording of a display, a window or a region (see record_start's fields: sound needs the person's separate permission, devtools_log, limit_seconds — 300 by default, 3600 at most); the person sees the recording frame and can stop it. pause / resume. stop: ends it and returns the library document (also a recording that ended by its limit). Znimok must be running.",
        scope: Some(Scope::Record),
        read_only: false,
        schema: || {
            let (mut props, _) = inner(crate::rectools::TOOLS, "record_start");
            props["action"] =
                json!({"type": "string", "enum": ["start", "pause", "resume", "stop"]});
            obj(props, &["action"])
        },
    },
    Tool {
        name: "record_status",
        title: "Is a recording running",
        description: "Whether a recording runs, whether an agent started it, paused or not, its length so far, and the document of the last one that ended.",
        scope: None,
        read_only: true,
        schema: || obj(json!({}), &[]),
    },
    Tool {
        name: "marks",
        title: "Add, change or remove marks",
        description: "Changes a document's marks in one go, in this order: delete (ids, or all), update (move, resize, restyle by id), add (new marks in plain words: rect, ellipse, arrow, line, pen, text, counter, hide, highlighter, stamp). Each part is one step of undo. Returns the picture.",
        scope: Some(Scope::LibraryWrite),
        read_only: false,
        schema: || {
            let (add, _) = inner(crate::edit::TOOLS, "add_marks");
            with_doc(
                json!({
                    "add": add["marks"],
                    "update": {"type": "array", "items": part(crate::edit::TOOLS, "update_marks", "One change for a set of marks")},
                    "delete": part(crate::edit::TOOLS, "delete_marks", "Marks to remove: ids, or all"),
                }),
                &[],
            )
        },
    },
    Tool {
        name: "transform",
        title: "Crop, rotate, resize, tone",
        description: "Changes the picture itself, in this order: crop (or reset), rotate (turn right / left / half, mirror), resize (width / height / percent, or the canvas), tone (exposure, gamma, contrast, or reset). Marks follow. Each part is one step of undo.",
        scope: Some(Scope::LibraryWrite),
        read_only: false,
        schema: || {
            with_doc(
                json!({
                    "crop": part(crate::edit::TOOLS, "crop", "The frame to keep, in pixels of the picture"),
                    "rotate": part(crate::edit::TOOLS, "rotate", "A turn and / or a mirror"),
                    "resize": part(crate::edit::TOOLS, "resize", "A new size, or a new canvas"),
                    "tone": part(crate::edit::TOOLS, "tone", "Exposure, gamma, contrast"),
                }),
                &[],
            )
        },
    },
    Tool {
        name: "library_edit",
        title: "Import, copy, trash, restore",
        description: "action import: a picture file (PNG, JPEG, WebP) or a .znimok document becomes a new library document (path, name?). duplicate: a copy of a document (document, name?). trash: to Znimok's trash, no question — library_edit restore brings it back (document). All undoable; deleting for good is library_delete.",
        scope: Some(Scope::LibraryWrite),
        read_only: false,
        schema: || {
            obj(
                json!({
                    "action": {"type": "string", "enum": ["import", "duplicate", "trash", "restore"]},
                    "document": doc_arg(),
                    "path": {"type": "string", "description": "The file to import"},
                    "name": {"type": "string", "description": "Name of the new document"},
                }),
                &["action"],
            )
        },
    },
    Tool {
        name: "hand_over",
        title: "Open in the editor, or copy",
        description: "Hands a document to the person: to = editor opens it in Znimok's editor and brings the window forward; clipboard puts the picture with its marks on the clipboard. Znimok must be running.",
        scope: Some(Scope::LibraryRead),
        read_only: false,
        schema: || {
            with_doc(
                json!({"to": {"type": "string", "enum": ["editor", "clipboard"]}}),
                &["to"],
            )
        },
    },
    Tool {
        name: "devlog",
        title: "The browser's DevTools log of a recording",
        description: "part summary (the default): counts by kind, the errors, the failed requests, the navigations, the dataLayer events — each with its time in the video. part events: the events in time order, filtered by kinds / errors_only / query / from_ms–to_ms, paged by limit (≤ 500) and offset; index gives one event whole (stack, headers, body). Sensitive values are hidden as the settings say.",
        scope: Some(Scope::LibraryRead),
        read_only: true,
        schema: || {
            let (mut props, _) = inner(crate::vidtools::TOOLS, "devlog_get");
            props["part"] = json!({"type": "string", "enum": ["summary", "events"], "description": "summary by default; events when a filter, a page or an index is given"});
            with_doc(props, &[])
        },
    },
    Tool {
        name: "video_frames",
        title: "Look at a recording",
        description: "Frames of a recording as pictures, with the marks of their moment: at the times given (at_ms, as recorded, up to 8), or count frames spread over it (4 by default). To see what happened at an error's or a click's time from devlog / video_info.",
        scope: Some(Scope::LibraryRead),
        read_only: true,
        schema: || {
            with_doc(
                json!({
                    "at_ms": {"type": "array", "items": {"type": "integer", "minimum": 0}, "maxItems": 8},
                    "count": {"type": "integer", "minimum": 1, "maximum": 8},
                }),
                &[],
            )
        },
    },
    Tool {
        name: "ocr",
        title: "Read the text",
        description: "The text on a document with the box of every line, on the device. With find: only the lines that contain that text (to place a mark, or to check a word is on screen).",
        scope: Some(Scope::LibraryRead),
        read_only: true,
        schema: || {
            let (mut props, _) = inner(crate::tools::TOOLS, "ocr");
            props["find"] = json!({"type": "string", "description": "Return only the lines that contain this text"});
            with_doc(props, &[])
        },
    },
    Tool {
        name: "share_targets",
        title: "Where documents can be sent",
        description: "The person's connected services (ZK-274): each target's key and name (Google Drive, Gmail, Telegram, Jira, Slack, Redmine, webhooks — a service can have several accounts) and whether it needs a place. With target: that target's places (Slack channels, Jira or Redmine projects, Telegram chats), fetched from the service. No tokens are ever shown. Znimok must be running.",
        scope: Some(Scope::Share),
        read_only: true,
        schema: || {
            obj(
                json!({"target": {"type": "string", "description": "A target's key from this list: its places"}}),
                &[],
            )
        },
    },
    Tool {
        name: "share",
        title: "Send a document to a connected service",
        description: "Sends a library document to one of the person's targets (key from share_targets): what = image (a screenshot as PNG, the default) or document (the Znimok file); for a recording video (MP4, the default), report (one HTML page with the video and its log), document, or logs (the log alone, JSON). place: a channel, project, issue (ZK-101) or chat — the last one used when left out. text goes with it as a comment. Sending out is its own permission, asked every time unless the person allowed it for longer; the person sees it in the app, and Znimok sends it itself through its queue (tries again when the network is away). Znimok must be running.",
        scope: Some(Scope::Share),
        read_only: false,
        schema: || {
            with_doc(
                json!({
                    "target": {"type": "string", "description": "A target's key from share_targets"},
                    "what": {"type": "string", "enum": ["image", "document", "video", "report", "logs"]},
                    "place": {"type": "string", "description": "Where in the target: a channel ID, a project key, an issue, a chat"},
                    "text": {"type": "string", "description": "A comment that goes with it"},
                    "hide": {"type": "boolean", "description": "report / logs: hide sensitive values (true), when the settings leave it to the export"}
                }),
                &["target"],
            )
        },
    },
];

/// The names a client sees: these, plus the inner ones kept as they are.
pub(crate) const KEPT: &[&str] = &[
    "library_search",
    "library_get",
    "list_marks",
    "video_info",
    "read_codes",
    "app_state",
    "annotate",
    "redact_pii",
    "export",
    "set_meta",
    "library_delete",
];

/// `None`: not a facade tool.
pub(crate) fn run(
    agent: &Agent,
    client: &str,
    name: &str,
    args: &Value,
) -> Option<Result<Output, String>> {
    let inner = |n: &str, a: Value| agent.run_inner(client, n, &a);
    let mut a = args.clone();
    Some(match name {
        "list_targets" => (|| {
            let d = inner("list_displays", json!({}))?;
            let w = inner("list_windows", json!({}))?;
            let mut v = d.structured.unwrap_or_default();
            v["windows"] = w.structured.unwrap_or_default()["windows"].take();
            Ok(Output::ok(v, vec![]))
        })(),
        "capture" => match arg_str(args, "target").unwrap_or("screen") {
            "window" => inner("capture_window", a),
            "active_window" => inner("capture_active_window", a),
            "region" => inner("capture_region", a),
            _ => inner("capture_screen", a),
        },
        "record" => match arg_str(args, "action") {
            Some(act @ ("start" | "pause" | "resume" | "stop")) => {
                inner(&format!("record_{act}"), a)
            }
            other => Err(format!(
                "«action» is start, pause, resume or stop, not {other:?}"
            )),
        },
        "marks" => (|| {
            let doc = args["document"].clone();
            let mut last = None;
            if let Some(d) = args.get("delete").filter(|d| d.is_object()) {
                let mut p = d.clone();
                p["document"] = doc.clone();
                last = Some(inner("delete_marks", p)?);
            }
            for u in args["update"].as_array().into_iter().flatten() {
                let mut p = u.clone();
                p["document"] = doc.clone();
                last = Some(inner("update_marks", p)?);
            }
            if args.get("add").is_some_and(|x| x.is_array()) {
                last = Some(inner(
                    "add_marks",
                    json!({"document": doc, "marks": args["add"]}),
                )?);
            }
            last.ok_or_else(|| "nothing to do: give add, update or delete".to_string())
        })(),
        "transform" => (|| {
            let doc = args["document"].clone();
            let mut last = None;
            for step in ["crop", "rotate", "resize", "tone"] {
                if let Some(p) = args.get(step).filter(|p| p.is_object()) {
                    let mut p = p.clone();
                    p["document"] = doc.clone();
                    last = Some(inner(step, p)?);
                }
            }
            last.ok_or_else(|| "nothing to do: give crop, rotate, resize or tone".to_string())
        })(),
        "library_edit" => match arg_str(args, "action") {
            Some(act @ ("import" | "duplicate" | "trash" | "restore")) => {
                inner(&format!("library_{act}"), a)
            }
            other => Err(format!(
                "«action» is import, duplicate, trash or restore, not {other:?}"
            )),
        },
        "hand_over" => match arg_str(args, "to") {
            Some("editor") => inner("open_in_editor", a),
            Some("clipboard") => inner("copy_to_clipboard", a),
            other => Err(format!("«to» is editor or clipboard, not {other:?}")),
        },
        "devlog" => {
            let filtered = [
                "index",
                "kinds",
                "errors_only",
                "query",
                "from_ms",
                "to_ms",
                "limit",
                "offset",
            ]
            .iter()
            .any(|k| !args[*k].is_null());
            if arg_str(args, "part") == Some("events") || filtered {
                inner("devlog_get", a)
            } else {
                inner("devlog_summary", a)
            }
        }
        "video_frames" => crate::vexport::frames(agent, args),
        "share_targets" => crate::sharetools::targets(agent, args),
        "share" => crate::sharetools::send(agent, client, args),
        "ocr" => match a.get("find").and_then(Value::as_str).map(str::to_string) {
            Some(text) => {
                a["text"] = json!(text);
                inner("find_text", a)
            }
            None => inner("ocr", a),
        },
        _ => return None,
    })
}
