//! What the MCP server asks the app over IPC (ZK-241, `docs/IPC.md`): the permission question
//! (`agents.ask`) and the «an agent is working» mark (`agents.activity`).
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
            Ok(json!({"grant": ask(client, scope, tool, confirm, WAIT)}))
        }
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
                        _ => "agents-ask-other",
                    });
                    (
                        a.tr.tr_args("agents-ask-title", &args(&[])),
                        a.tr.tr_args("agents-ask-body", &args(&[("what", what)])),
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
