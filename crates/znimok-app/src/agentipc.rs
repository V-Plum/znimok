//! What the MCP server asks the app over IPC (ZK-241, `docs/IPC.md`): the permission question
//! (`agents.ask`), the «an agent is working» mark (`agents.activity`) and an agent's screen
//! recording (`agents.record`, ZK-237), a document opened or copied for it (`agents.app`,
//! ZK-238).
//!
//! The question comes in on an IPC thread and waits there; the dialog is shown on the UI thread
//! in the app's window and its answer goes back through a channel. Nobody answering for
//! [`WAIT`] is a refusal, and the dialog goes away. One question at a time: a second agent
//! waits for the first one's answer.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, mpsc};
use std::time::Duration;

use serde_json::{Value, json};

/// How long the person has to answer.
const WAIT: Duration = Duration::from_secs(120);

/// One question at a time.
static ASKING: Mutex<()> = Mutex::new(());
/// The question whose dialog is open (0: none) — a timed-out one closes only its own dialog.
static OPEN: AtomicU64 = AtomicU64::new(0);
static NEXT: AtomicU64 = AtomicU64::new(1);
/// The agent at work now: (client, tool).
static ACTIVE: Mutex<Option<(String, String)>> = Mutex::new(None);

/// The agent at work, for `app.state`.
pub fn active() -> Option<Value> {
    ACTIVE
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .as_ref()
        .map(|(client, tool)| json!({"client": client, "tool": tool}))
}

/// The IPC methods of this module; `None` for anything else. Runs on an IPC thread.
pub fn handle(method: &str, params: &Value) -> Option<Result<Value, znimok_ipc::RpcError>> {
    let text = |k: &str| {
        params
            .get(k)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    Some(match method {
        "agents.ask" => {
            let (client, scope, tool) = (text("client"), text("scope"), text("tool"));
            if client.is_empty() || scope.is_empty() {
                return Some(Err(znimok_ipc::RpcError::invalid_params(
                    "«client» and «scope» are required",
                )));
            }
            // Deleting for good: one yes for this one thing (ZK-234).
            let confirm = params
                .get("confirm")
                .and_then(|c| c.get("name"))
                .and_then(Value::as_str)
                .map(str::to_string);
            // One question per client (ZK-251): «this session» and «always» cover every
            // scope but the sound of a recording, which is asked for on its own.
            let all = confirm.is_none() && scope != "record_audio";
            let grant = ask(client, scope, tool, confirm, WAIT);
            Ok(json!({"grant": grant, "all": all && grant.is_some_and(|g| g != "once")}))
        }
        "agents.record" => record(params),
        "agents.app" => app(params),
        // ZK-242: screenshots for an agent — on macOS only the app may capture.
        "capture.displays" | "capture.windows" | "capture.take" => capture(method, params),
        "agents.activity" => {
            let on = params
                .get("active")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            *ACTIVE.lock().unwrap_or_else(|p| p.into_inner()) =
                on.then(|| (text("client"), text("tool")));
            Ok(Value::Null)
        }
        _ => return None,
    })
}

/// Shows the question and waits for the answer: "once" / "session" / "always", or `None`.
fn ask(
    client: String,
    scope: String,
    tool: String,
    confirm: Option<String>,
    wait: Duration,
) -> Option<&'static str> {
    let _one = ASKING.lock().unwrap_or_else(|p| p.into_inner());
    let id = NEXT.fetch_add(1, Ordering::SeqCst);
    let (tx, rx) = mpsc::channel();
    slint::invoke_from_event_loop(move || show(id, &client, &scope, &tool, confirm, tx)).ok()?;
    match rx.recv_timeout(wait) {
        Ok(grant) => grant,
        Err(_) => {
            let _ = slint::invoke_from_event_loop(move || {
                if OPEN.load(Ordering::SeqCst) == id {
                    crate::with_ctx(|_, ui| crate::dialog::answered(ui, -1));
                }
            });
            None
        }
    }
}

/// UI thread: the dialog in the app's window, brought forward.
fn show(
    id: u64,
    client: &str,
    scope: &str,
    tool: &str,
    confirm: Option<String>,
    tx: mpsc::Sender<Option<&'static str>>,
) {
    let mut tx = Some(tx);
    crate::with_ctx(|a, ui| {
        // Switched off in the settings: nothing is asked, nothing is allowed.
        if !a.prefs().agents.mcp_enabled {
            return;
        }
        let Some(tx) = tx.take() else { return };
        let args = |more: &[(&'static str, String)]| {
            let mut v = vec![("client", client.to_string()), ("tool", tool.to_string())];
            v.extend(more.iter().cloned());
            crate::app::fargs(&v)
        };
        let (title, body, buttons, grants): (_, _, Vec<String>, &'static [&'static str]) =
            match &confirm {
                Some(name) => (
                    a.tr.tr_args(
                        "agents-confirm-delete-title",
                        &args(&[("name", name.clone())]),
                    ),
                    a.tr.tr_args("agents-confirm-delete-body", &args(&[])),
                    vec![a.tr.tr("common-delete"), a.tr.tr("common-cancel")],
                    &["once"],
                ),
                None => {
                    let what = a.tr.tr(match scope {
                        "capture" => "agents-ask-capture",
                        "library_read" => "agents-ask-library-read",
                        "library_write" => "agents-ask-library-write",
                        "settings" => "agents-ask-settings",
                        "record" => "agents-ask-record",
                        "record_audio" => "agents-ask-record-audio",
                        _ => "agents-ask-other",
                    });
                    let mut body = a.tr.tr_args("agents-ask-body", &args(&[("what", what)]));
                    if scope != "record_audio" {
                        body.push_str(
                            "

",
                        );
                        body.push_str(&a.tr.tr("agents-ask-all"));
                    }
                    (
                        a.tr.tr_args("agents-ask-title", &args(&[])),
                        body,
                        vec![
                            a.tr.tr("agents-ask-once"),
                            a.tr.tr("agents-ask-session"),
                            a.tr.tr("agents-ask-always"),
                            a.tr.tr("agents-ask-deny"),
                        ],
                        &["once", "session", "always"],
                    )
                }
            };
        let last = buttons.len() - 1;
        crate::show_window(ui);
        OPEN.store(id, Ordering::SeqCst);
        // «Deny» / «Cancel» is what Enter and Esc press: access is never given by accident.
        crate::dialog::ask(
            ui,
            title,
            body,
            buttons,
            last,
            Some(last),
            move |_, answer| {
                OPEN.store(0, Ordering::SeqCst);
                let _ = tx.send(answer.and_then(|i| grants.get(i).copied()));
            },
        );
    });
    // Nobody to ask (no window, switched off): a refusal at once.
    if let Some(tx) = tx {
        let _ = tx.send(None);
    }
}

// ------------------------------------------------------------------ recording (ZK-237)

/// The longest an agent's recording may ask for, and what it gets when it does not say.
const LIMIT_MAX_S: u64 = 3600;
const LIMIT_DEFAULT_S: u64 = 300;
/// How long `stop` waits for the file to be finished and wrapped.
const SAVE_WAIT: Duration = Duration::from_secs(180);

#[derive(Default)]
struct AgentRec {
    /// An agent's recording runs or is being saved.
    owned: bool,
    /// Which one (a limit's timer stops only its own).
    generation: u64,
    /// The last one that ended: the document's path, or why there is none.
    result: Option<Result<String, String>>,
}

static REC: Mutex<AgentRec> = Mutex::new(AgentRec {
    owned: false,
    generation: 0,
    result: None,
});
static REC_DONE: std::sync::Condvar = std::sync::Condvar::new();

fn rec_state() -> std::sync::MutexGuard<'static, AgentRec> {
    REC.lock().unwrap_or_else(|p| p.into_inner())
}

/// From `rec::saved` (UI thread): the recording that ended was an agent's — it gets the result
/// and the app shows nothing.
pub fn recording_done(r: Result<&std::path::Path, &str>) -> bool {
    let mut s = rec_state();
    if !s.owned {
        return false;
    }
    s.owned = false;
    s.result = Some(match r {
        Ok(p) => Ok(p.display().to_string()),
        Err(e) => Err(e.to_string()),
    });
    REC_DONE.notify_all();
    true
}

/// Runs `f` on the UI thread and waits for what it returns.
fn on_ui<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
) -> Result<T, znimok_ipc::RpcError> {
    let fail = |m: &str| znimok_ipc::RpcError::new(-32000, m);
    let (tx, rx) = mpsc::channel();
    slint::invoke_from_event_loop(move || {
        let _ = tx.send(f());
    })
    .map_err(|_| fail("Znimok is closing"))?;
    rx.recv_timeout(Duration::from_secs(15))
        .map_err(|_| fail("Znimok did not answer"))
}

/// What to record, as the agent named it.
enum What {
    /// A display by its id; `None`: the primary one.
    Display(Option<String>),
    Window(u64),
    Region(znimok_platform::Rect),
}

/// `agents.record {op: start | pause | resume | stop | status, …}`. An IPC thread.
fn record(params: &Value) -> Result<Value, znimok_ipc::RpcError> {
    let fail = |m: String| znimok_ipc::RpcError::new(-32000, m);
    match params.get("op").and_then(Value::as_str).unwrap_or("") {
        "start" => {
            let what =
                if let Some(id) = params.get("window").and_then(Value::as_u64) {
                    What::Window(id)
                } else if let Some(r) = params.get("region").filter(|r| r.is_object()) {
                    What::Region(serde_json::from_value(r.clone()).map_err(|e| {
                        znimok_ipc::RpcError::invalid_params(format!("region: {e}"))
                    })?)
                } else {
                    What::Display(
                        params
                            .get("display")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                    )
                };
            let flag = |k: &str, d: bool| params.get(k).and_then(Value::as_bool).unwrap_or(d);
            let opts = crate::rec::AgentOpts {
                system: flag("system_audio", false),
                microphone: flag("microphone", false),
                log: flag("log", true),
            };
            let limit = params
                .get("limit_s")
                .and_then(Value::as_u64)
                .unwrap_or(LIMIT_DEFAULT_S)
                .clamp(1, LIMIT_MAX_S);
            on_ui(move || start(what, opts, limit))?.map_err(fail)
        }
        op @ ("pause" | "resume") => {
            let pause = op == "pause";
            on_ui(move || {
                if !rec_state().owned || !crate::rec::is_recording() {
                    return Err("no recording of an agent is running".to_string());
                }
                if crate::rec::is_paused() != pause {
                    crate::rec::toggle_pause();
                }
                Ok(status())
            })?
            .map_err(fail)
        }
        "stop" => {
            on_ui(|| {
                let s = rec_state();
                if s.owned {
                    drop(s);
                    if crate::rec::is_recording() {
                        crate::rec::stop();
                    }
                    Ok(())
                } else if s.result.is_some() {
                    // Already ended (the limit, or the person pressed Stop): its result waits.
                    Ok(())
                } else {
                    Err("no recording of an agent is running".to_string())
                }
            })?
            .map_err(fail)?;
            let s = rec_state();
            let (mut s, _) = REC_DONE
                .wait_timeout_while(s, SAVE_WAIT, |s| s.owned)
                .unwrap_or_else(|p| p.into_inner());
            match s.result.take() {
                Some(Ok(path)) => Ok(json!({"path": path})),
                Some(Err(e)) => Err(fail(e)),
                None => Err(fail("the recording is still being saved".into())),
            }
        }
        "status" => on_ui(status),
        op => Err(znimok_ipc::RpcError::invalid_params(format!(
            "unknown op «{op}»"
        ))),
    }
}

/// UI thread: what is going on with recording.
fn status() -> Value {
    let s = rec_state();
    let recording = crate::rec::is_recording();
    json!({
        "recording": recording,
        // Started by an agent (false: the person's own recording — not ours to touch).
        "agent": s.owned,
        "saving": s.owned && !recording,
        "paused": crate::rec::is_paused(),
        "elapsed_ms": crate::rec::elapsed_ms(),
        "finished": s.result.as_ref().and_then(|r| r.as_ref().ok()),
        "failed": s.result.as_ref().and_then(|r| r.as_ref().err()),
    })
}

#[cfg(any(windows, target_os = "macos"))]
fn targets() -> (
    Vec<znimok_platform::WindowInfo>,
    Vec<znimok_platform::DisplayInfo>,
) {
    crate::devlog::windows_and_displays()
}

#[cfg(not(any(windows, target_os = "macos")))]
fn targets() -> (
    Vec<znimok_platform::WindowInfo>,
    Vec<znimok_platform::DisplayInfo>,
) {
    (Vec::new(), Vec::new())
}

/// UI thread: starts the recording and its time limit.
fn start(what: What, opts: crate::rec::AgentOpts, limit_s: u64) -> Result<Value, String> {
    if !crate::with_prefs(|p| p.agents.mcp_enabled).unwrap_or(false) {
        return Err("MCP is switched off in Znimok's settings".into());
    }
    if crate::rec::is_recording() || rec_state().owned {
        return Err("a recording is already running".into());
    }
    if crate::overlay::is_open() || !crate::capture::available() {
        return Err("Znimok is busy with a capture".into());
    }
    let (windows, displays) = targets();
    let holding = |x: i32, y: i32| {
        displays.iter().find(|d| {
            let b = d.bounds;
            x >= b.x && x < b.x + b.width as i32 && y >= b.y && y < b.y + b.height as i32
        })
    };
    let centre = |r: &znimok_platform::Rect| (r.x + r.width as i32 / 2, r.y + r.height as i32 / 2);
    let choice = match what {
        What::Display(id) => {
            let d = match &id {
                Some(id) => displays
                    .iter()
                    .find(|d| &d.id.0 == id)
                    .ok_or_else(|| format!("no display «{id}»"))?,
                None => displays
                    .iter()
                    .find(|d| d.primary)
                    .or_else(|| displays.first())
                    .ok_or("no display")?,
            };
            crate::rec::Choice {
                display: d.bounds,
                frame: d.bounds,
                window: None,
                source: "screen",
            }
        }
        What::Window(id) => {
            let w = windows
                .iter()
                .find(|w| w.id.0 == id && !w.minimized)
                .ok_or_else(|| format!("no window {id} on screen"))?;
            let (cx, cy) = centre(&w.bounds);
            let display = displays
                .iter()
                .find(|d| Some(&d.id) == w.display.as_ref())
                .or_else(|| holding(cx, cy))
                .or_else(|| displays.first())
                .map_or(w.bounds, |d| d.bounds);
            crate::rec::Choice {
                display,
                frame: w.bounds,
                window: Some(id),
                source: "window",
            }
        }
        What::Region(r) => {
            let (cx, cy) = centre(&r);
            let d = holding(cx, cy)
                .ok_or("the region is not on a display")?
                .bounds;
            // The part of it on that display (a recording is of one display).
            let (x0, y0) = (r.x.max(d.x), r.y.max(d.y));
            let x1 = (r.x + r.width as i32).min(d.x + d.width as i32);
            let y1 = (r.y + r.height as i32).min(d.y + d.height as i32);
            if x1 - x0 < 16 || y1 - y0 < 16 {
                return Err("the region is too small (16 × 16 at least)".into());
            }
            crate::rec::Choice {
                display: d,
                frame: znimok_platform::Rect {
                    x: x0,
                    y: y0,
                    width: (x1 - x0) as u32,
                    height: (y1 - y0) as u32,
                },
                window: None,
                source: "region",
            }
        }
    };
    let frame = choice.frame;
    crate::rec::start_for_agent(choice, opts)?;
    let generation = {
        let mut s = rec_state();
        s.owned = true;
        s.generation += 1;
        s.result = None;
        s.generation
    };
    // The limit: an agent's recording never runs for ever.
    slint::Timer::single_shot(Duration::from_secs(limit_s), move || {
        let mine = {
            let s = rec_state();
            s.owned && s.generation == generation
        };
        if mine && crate::rec::is_recording() {
            crate::rec::stop();
        }
    });
    Ok(json!({
        "recording": true,
        "width": frame.width,
        "height": frame.height,
        "limit_s": limit_s,
        "system_audio": opts.system,
        "microphone": opts.microphone,
        "log": opts.log,
    }))
}

// ------------------------------------------------------------------ the editor, the clipboard (ZK-238)

/// `agents.app {op: open | copy, path}`: a library document in the editor, or its picture on the
/// clipboard. An IPC thread.
fn app(params: &Value) -> Result<Value, znimok_ipc::RpcError> {
    let fail = |m: String| znimok_ipc::RpcError::new(-32000, m);
    let path = std::path::PathBuf::from(params.get("path").and_then(Value::as_str).unwrap_or(""));
    if !path.is_file() {
        return Err(znimok_ipc::RpcError::invalid_params("«path» is not a file"));
    }
    let enabled = || crate::with_prefs(|p| p.agents.mcp_enabled).unwrap_or(false);
    let off = "MCP is switched off in Znimok's settings";
    match params.get("op").and_then(Value::as_str).unwrap_or("") {
        "open" => on_ui(move || {
            if !enabled() {
                return Err(off.to_string());
            }
            crate::with_ctx(|a, ui| {
                a.open_path(ui, &path);
                crate::show_window(ui);
            });
            Ok(json!({"opened": true}))
        })?
        .map_err(fail),
        "copy" => {
            // Rendered here, off the UI thread; only the clipboard is the UI thread's.
            let doc = znimok_agents::library::load(&path).map_err(fail)?;
            let r = znimok_agents::library::render(&doc, 1.0);
            let (w, h) = (r.width, r.height);
            on_ui(move || {
                if !enabled() {
                    return Err(off.to_string());
                }
                crate::io::copy_image(w, h, r.rgba)?;
                crate::commands::emit("copied");
                Ok(json!({"copied": true, "width": w, "height": h}))
            })?
            .map_err(fail)
        }
        op => Err(znimok_ipc::RpcError::invalid_params(format!(
            "unknown op «{op}»"
        ))),
    }
}

// ------------------------------------------------------------------ screenshots (ZK-242)

/// `capture.displays`, `capture.windows`: what can be captured; `capture.take {target}`: the shot,
/// saved as a new library document (as a quiet shot from the overlay is, with its source), its
/// path back; the editor does not open. An IPC thread: the capture runs here, the save on the UI
/// thread.
fn capture(method: &str, params: &Value) -> Result<Value, znimok_ipc::RpcError> {
    let fail = |m: String| znimok_ipc::RpcError::new(-32000, m);
    // The settings live on the UI thread.
    if !on_ui(|| crate::with_prefs(|p| p.agents.mcp_enabled).unwrap_or(false))? {
        return Err(fail("MCP is switched off in Znimok's settings".into()));
    }
    let to = |e: serde_json::Error| fail(e.to_string());
    match method {
        "capture.displays" => serde_json::to_value(crate::capture::targets().0).map_err(to),
        "capture.windows" => serde_json::to_value(crate::capture::targets().1).map_err(to),
        _ => {
            let target: znimok_platform::CaptureTarget =
                serde_json::from_value(params.get("target").cloned().unwrap_or(Value::Null))
                    .map_err(|e| znimok_ipc::RpcError::invalid_params(format!("target: {e}")))?;
            let source = match target {
                znimok_platform::CaptureTarget::Window { .. } => "window",
                znimok_platform::CaptureTarget::Region { .. } => "region",
                _ => "screen",
            };
            let raster = crate::capture::take(&target).map_err(|e| match e {
                crate::capture::Fail::Permission => fail(
                    "Znimok has no Screen Recording permission yet: the system asked the person — try again once it is given".into(),
                ),
                crate::capture::Fail::Other(m) => fail(m),
            })?;
            on_ui(move || {
                let mut out = Err("Znimok's window is not ready".to_string());
                crate::with_ctx(|a, ui| out = a.store_quietly(ui, raster, source));
                out
            })?
            .map(|(path, name)| json!({"path": path.display().to_string(), "name": name}))
            .map_err(fail)
        }
    }
}
