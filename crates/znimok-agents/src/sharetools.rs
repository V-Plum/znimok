//! Sending for agents (ZK-274): a library document to one of the person's connected services.
//! The targets, their tokens and the sending queue live in the app, so this is a call to it over
//! IPC (`agents.share`); the file is made here (the same export as `export`) and handed over.
//! The permission is its own (`share`), never given with «everything» (ZK-251).

use std::path::{Path, PathBuf};

use crate::library;
use crate::tools::{Agent, Output, arg_str};
use serde_json::{Value, json};

fn app(agent: &Agent, params: Value) -> Result<Value, String> {
    agent.gui.ensure_running()?;
    agent.gui.call("agents.share", params)
}

/// `share_targets`: the list, or one target's places.
pub(crate) fn targets(agent: &Agent, args: &Value) -> Result<Output, String> {
    let v = match arg_str(args, "target") {
        Some(t) => app(agent, json!({"op": "places", "target": t}))?,
        None => app(agent, json!({"op": "targets"}))?,
    };
    Ok(Output::ok(v, vec![]))
}

/// A file of its own for this sending, beside the others.
fn scratch(ext: &str) -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join("znimok-share");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join(format!("{}.{ext}", uuid::Uuid::new_v4())))
}

/// `share`: the file made as `what` says, then queued by the app.
pub(crate) fn send(agent: &Agent, client: &str, args: &Value) -> Result<Output, String> {
    let target = arg_str(args, "target").ok_or("«target» is required (from share_targets)")?;
    let (doc, src) = agent.doc(args)?;
    let video = library::peek_item(&src).is_some_and(|i| i.kind == "video");
    let what = arg_str(args, "what").unwrap_or(if video { "video" } else { "image" });
    let stem = znimok_platform::clipfile::file_stem(&doc.name);
    let export = |format: &str, ext: &str| -> Result<PathBuf, String> {
        let out = scratch(ext)?;
        let mut a = json!({"document": args["document"], "format": format, "path": out.display().to_string()});
        if let Some(h) = args["hide"].as_bool() {
            a["hide"] = json!(h);
        }
        agent.run_inner(client, "export", &a)?;
        Ok(out)
    };
    // (the file, its name, its type, its kind, whether it is ours to remove)
    let (path, file_name, mime, kind, ours): (PathBuf, String, &str, &str, bool) =
        match (what, video) {
            ("document", _) => (
                src.clone(),
                src.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| format!("{stem}.znimok")),
                "application/octet-stream",
                "document",
                false,
            ),
            ("image", false) => (
                export("png", "png")?,
                format!("{stem}.png"),
                "image/png",
                "screenshot",
                true,
            ),
            ("video", true) => (
                export("mp4", "mp4")?,
                format!("{stem}.mp4"),
                "video/mp4",
                "video",
                true,
            ),
            ("report", true) => (
                export("report", "html")?,
                format!("{stem}.html"),
                "text/html",
                "report",
                true,
            ),
            ("logs", true) => {
                let z = export("zreport", "zreport")?;
                let log = log_of(&z);
                let _ = std::fs::remove_file(&z);
                let out = scratch("json")?;
                std::fs::write(&out, log?).map_err(|e| e.to_string())?;
                (
                    out,
                    format!("{stem}.log.json"),
                    "application/json",
                    "log",
                    true,
                )
            }
            (w, false) => {
                return Err(format!("a screenshot goes as image or document, not «{w}»"));
            }
            (w, true) => {
                return Err(format!(
                    "a recording goes as video, report, document or logs, not «{w}»"
                ));
            }
        };
    let r = app(
        agent,
        json!({
            "op": "send",
            "client": client,
            "target": target,
            "place": arg_str(args, "place").unwrap_or(""),
            "text": arg_str(args, "text").unwrap_or(""),
            "title": doc.name,
            "kind": kind,
            "mime": mime,
            "file_name": file_name,
            "path": path.display().to_string(),
        }),
    );
    if ours {
        let _ = std::fs::remove_file(&path);
    }
    Ok(Output::ok(r?, vec![]))
}

/// The log of a `.zreport` — masked and on the exported time line, as in the report.
fn log_of(zreport: &Path) -> Result<Vec<u8>, String> {
    let bytes = std::fs::read(zreport).map_err(|e| e.to_string())?;
    znimok_report::zip::read(&bytes)?
        .into_iter()
        .find(|(n, _)| n == "log.json")
        .map(|(_, b)| b)
        .ok_or_else(|| "the recording has no log".to_string())
}
