//! Screen recording for agents (ZK-237). The recorder lives in the app, so these tools are calls
//! to it over IPC (`agents.record`): start (a display, a window, a region), pause, resume, stop
//! (the recording becomes a library document), status.
//!
//! The permission is its own (`record`), and sound is asked for separately (`record_audio`):
//! a recording with the microphone is never what «record the screen» allowed. Every recording
//! has a time limit; the person sees the usual red frame and the bar with Stop. Only starting
//! asks: the other tools touch nothing but the agent's own recording, and «this time» must not
//! ask again at stop.

use crate::library;
use crate::permissions::Scope;
use crate::tools::{Agent, Output, Tool, arg_str, obj};
use serde_json::{Value, json};

pub(crate) const TOOLS: &[Tool] = &[
    Tool {
        name: "record_start",
        title: "Start a screen recording",
        description: "Starts recording the screen as video: a display (the primary one by default), a window (id from list_windows) or a region in desktop units. Without sound unless asked — sound needs the person's separate permission. With the Znimok browser extension the recording carries the browser's DevTools log (devtools_log: false leaves it out). It stops at record_stop or at the time limit (5 minutes by default, an hour at most); the person sees the recording frame and can stop it too. Znimok must be running.",
        scope: Some(Scope::Record),
        read_only: false,
        schema: || {
            obj(
                json!({
                    "display": {"type": "string", "description": "Display id from list_displays"},
                    "window": {"type": "integer", "description": "Window id from list_windows"},
                    "region": {"type": "object", "description": "A part of one display, desktop units",
                        "properties": {"x": {"type": "integer"}, "y": {"type": "integer"},
                                       "width": {"type": "integer", "minimum": 16}, "height": {"type": "integer", "minimum": 16}},
                        "required": ["x", "y", "width", "height"]},
                    "sound": {"type": "string", "enum": ["none", "system", "microphone", "both"],
                        "description": "none by default; anything else asks the person for the sound permission"},
                    "devtools_log": {"type": "boolean", "description": "Write the browser's DevTools log when the extension is connected (true by default)"},
                    "limit_seconds": {"type": "integer", "minimum": 1, "maximum": 3600,
                        "description": "The recording stops by itself after this long (300 by default)"}
                }),
                &[],
            )
        },
    },
    Tool {
        name: "record_pause",
        title: "Pause the recording",
        description: "Pauses the recording this agent started.",
        scope: None,
        read_only: false,
        schema: || obj(json!({}), &[]),
    },
    Tool {
        name: "record_resume",
        title: "Resume the recording",
        description: "Resumes the paused recording.",
        scope: None,
        read_only: false,
        schema: || obj(json!({}), &[]),
    },
    Tool {
        name: "record_stop",
        title: "Stop the recording",
        description: "Stops the recording and returns it as a library document (id, length, whether it has the DevTools log) — read it with video_info and devlog_summary, export it with export. Also returns a recording that already ended by its time limit.",
        scope: None,
        read_only: false,
        schema: || obj(json!({}), &[]),
    },
    Tool {
        name: "record_status",
        title: "Is a recording running",
        description: "Whether a recording runs, whether an agent started it, paused or not, its length so far, and the document of the last one that ended.",
        scope: None,
        read_only: true,
        schema: || obj(json!({}), &[]),
    },
];

fn app(agent: &Agent, params: Value) -> Result<Value, String> {
    agent.gui.ensure_running()?;
    agent.gui.call("agents.record", params)
}

/// `None`: not a recording tool.
pub(crate) fn run(
    agent: &Agent,
    client: &str,
    name: &str,
    args: &Value,
) -> Option<Result<Output, String>> {
    Some(match name {
        "record_start" => (|| {
            let (system, microphone) = match arg_str(args, "sound").unwrap_or("none") {
                "none" => (false, false),
                "system" => (true, false),
                "microphone" | "mic" => (false, true),
                "both" => (true, true),
                s => {
                    return Err(format!(
                        "«sound» is none, system, microphone or both, not «{s}»"
                    ));
                }
            };
            // Sound is its own permission, asked before anything starts.
            if system || microphone {
                agent.authorize(client, Scope::RecordAudio, name)?;
            }
            let mut p = json!({
                "op": "start",
                "system_audio": system,
                "microphone": microphone,
                "log": args["devtools_log"].as_bool().unwrap_or(true),
            });
            for k in ["display", "window", "region"] {
                if !args[k].is_null() {
                    p[k] = args[k].clone();
                }
            }
            if let Some(n) = args["limit_seconds"].as_u64() {
                p["limit_s"] = json!(n);
            }
            Ok(Output::ok(app(agent, p)?, vec![]))
        })(),
        "record_pause" => app(agent, json!({"op": "pause"})).map(|v| Output::ok(v, vec![])),
        "record_resume" => app(agent, json!({"op": "resume"})).map(|v| Output::ok(v, vec![])),
        "record_status" => app(agent, json!({"op": "status"})).map(|mut v| {
            // The finished one as a document id, not a path.
            if let Some(item) = v["finished"]
                .as_str()
                .and_then(|p| library::peek_item(p.as_ref()))
            {
                v["finished"] = json!({"id": item.id, "name": item.name});
            }
            Output::ok(v, vec![])
        }),
        "record_stop" => (|| {
            let v = app(agent, json!({"op": "stop"}))?;
            let path = v["path"]
                .as_str()
                .ok_or("the app did not return a document")?;
            let item = library::peek_item(path.as_ref())
                .ok_or_else(|| format!("{path}: the recording cannot be read"))?;
            Ok(Output::ok(
                serde_json::to_value(&item).map_err(|e| e.to_string())?,
                vec![],
            ))
        })(),
        _ => return None,
    })
}
