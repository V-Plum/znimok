//! What an agent does with the running app (ZK-238): the window in front captured, a document
//! opened in the editor or put on the clipboard, and what the app is doing now.

use crate::backend::Shot;
use crate::library;
use crate::permissions::Scope;
use crate::tools::{Agent, Output, Tool, arg_str, doc_arg, image, link, obj, summary};
use serde_json::{Value, json};
use znimok_platform::CaptureTarget;

/// The longest a capture waits before it is taken.
pub(crate) const DELAY_MAX_S: u64 = 30;

/// `delay_seconds` of a capture tool's schema.
pub(crate) fn delay_arg() -> Value {
    json!({"type": "integer", "minimum": 0, "maximum": DELAY_MAX_S,
           "description": "Wait this many seconds first — time to bring the right window forward or open a menu"})
}

/// Waits as the capture asked.
pub(crate) fn delay(args: &Value) {
    if let Some(s) = args["delay_seconds"].as_u64().filter(|s| *s > 0) {
        std::thread::sleep(std::time::Duration::from_secs(s.min(DELAY_MAX_S)));
    }
}

pub(crate) const TOOLS: &[Tool] = &[
    Tool {
        name: "capture_active_window",
        title: "Capture the window in front",
        description: "Screenshot of the window in front of the others (not Znimok's own). When the agent runs in a terminal, that terminal is in front — give delay_seconds so the person can switch to the window they mean. Saved as a new library document.",
        scope: Some(Scope::Capture),
        read_only: false,
        schema: || obj(json!({"delay_seconds": delay_arg()}), &[]),
    },
    Tool {
        name: "open_in_editor",
        title: "Open in Znimok's editor",
        description: "Opens a library document in the Znimok editor and brings the window forward — to hand the work over to the person. Znimok must be running.",
        scope: Some(Scope::LibraryRead),
        read_only: true,
        schema: || obj(json!({"document": doc_arg()}), &["document"]),
    },
    Tool {
        name: "copy_to_clipboard",
        title: "Copy the picture to the clipboard",
        description: "Puts a document's picture with its marks on the clipboard, ready to paste (a recording: its poster frame). Replaces what the clipboard held. Znimok must be running.",
        scope: Some(Scope::LibraryRead),
        read_only: false,
        schema: || obj(json!({"document": doc_arg()}), &["document"]),
    },
    Tool {
        name: "app_state",
        title: "What Znimok is doing",
        description: "Whether the Znimok app is running and what it shows: the page (library, editor, settings, overlay, recording), the open document, the tool, the zoom, a running recording.",
        scope: None,
        read_only: true,
        schema: || obj(json!({}), &[]),
    },
];

/// `None`: not one of these tools.
pub(crate) fn run(agent: &Agent, name: &str, args: &Value) -> Option<Result<Output, String>> {
    Some(match name {
        "capture_active_window" => (|| {
            delay(args);
            // The list is in front-to-back order; Znimok's own windows are already left out.
            let w = agent
                .capture
                .windows()?
                .into_iter()
                .next()
                .ok_or("no window on screen")?;
            let target = CaptureTarget::Window { id: w.id };
            let (doc, path) = match agent.capture.take(&target)? {
                Shot::Pixels(r) => agent.new_doc(r, "window")?,
                Shot::Saved(p) => (library::load(&p)?, p),
            };
            let r = library::render(&doc, 1.0);
            let mut s = summary(&doc, &path);
            s["window"] = json!({"id": w.id.0, "title": w.title, "app": w.app});
            Ok(Output::ok(
                s,
                image(&r).into_iter().chain([link(&doc)]).collect(),
            ))
        })(),
        "open_in_editor" | "copy_to_clipboard" => (|| {
            let d = arg_str(args, "document").ok_or("«document» is required")?;
            let path = agent
                .lib
                .resolve(d)
                .ok_or_else(|| format!("no document «{d}» in the library"))?;
            agent.gui.ensure_running()?;
            let op = if name == "open_in_editor" {
                "open"
            } else {
                "copy"
            };
            let v = agent.gui.call(
                "agents.app",
                json!({"op": op, "path": path.display().to_string()}),
            )?;
            Ok(Output::ok(v, vec![]))
        })(),
        "app_state" => Ok(Output::ok(
            match agent.gui.call("app.state", json!({})) {
                Ok(mut v) => {
                    v["running"] = json!(true);
                    v
                }
                Err(_) => json!({"running": false}),
            },
            vec![],
        )),
        _ => return None,
    })
}
