//! The browser log of a recording in the app (ZK-97): the hub the Native Messaging host talks
//! to over the app's IPC server, the host's registration for the browsers, and the extension's
//! requests carried out on the UI thread (record this window, stop, pause, resume).

use std::sync::OnceLock;

use serde_json::{Value, json};
use znimok_devtools::Hub;

static HUB: OnceLock<Hub> = OnceLock::new();

pub fn hub() -> &'static Hub {
    HUB.get_or_init(Hub::new)
}

/// The app's IPC server with the hub's methods (None when another instance holds the endpoint).
pub fn start_server() -> Option<znimok_ipc::Server> {
    let server = znimok_ipc::Server::start(
        znimok_ipc::Config::default(),
        |method: &str, params: Value| {
            hub()
                .handle(method, &params)
                .unwrap_or_else(|| Err(znimok_ipc::RpcError::method_not_found(method)))
        },
    );
    match server {
        Ok(s) => Some(s),
        Err(e) => {
            eprintln!("devtools: no IPC server ({e})");
            None
        }
    }
}

/// The CLI next to the app is the browsers' host; registered on every start (it follows the app
/// when it moves). Nothing when the CLI is not there (a bare build).
pub fn register_host() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let Some(dir) = exe.parent() else { return };
    let cli = dir.join(if cfg!(windows) {
        "znimok.exe"
    } else {
        "znimok"
    });
    if !cli.is_file() {
        return;
    }
    if let Err(e) = znimok_devtools::register::register(&cli) {
        eprintln!("devtools: host registration: {e}");
    }
}

/// The extension's requests, a few times a second on the UI thread; the settings that rule the
/// log (written or not, the extension may start or not) reach the hub when they change.
pub fn poll() {
    thread_local! {
        static LAST: std::cell::Cell<Option<(bool, bool)>> = const { std::cell::Cell::new(None) };
    }
    if let Some(o) = crate::with_prefs(|p| (p.video.devtools_log, p.video.extension_control))
        && LAST.with(|l| l.replace(Some(o))) != Some(o)
    {
        hub().set_options(o.0, o.1);
    }
    for r in hub().take_requests() {
        match r.get("cmd").and_then(Value::as_str) {
            Some("stop") => crate::rec::stop(),
            Some("pause") if crate::rec::is_recording() && !crate::rec::is_paused() => {
                crate::rec::toggle_pause()
            }
            Some("resume") if crate::rec::is_paused() => crate::rec::toggle_pause(),
            Some("rec") => record_window(&r),
            _ => {}
        }
    }
}

/// «Record this window» from the browser: the window whose title carries the extension's mark.
fn record_window(r: &Value) {
    let rid = r.get("rid").cloned().unwrap_or(Value::Null);
    let fail = |why: &str| hub().reply(json!({"rec": "fail", "rid": rid, "why": why}));
    let ctl = crate::with_prefs(|p| p.video.extension_control).unwrap_or(true);
    if !ctl {
        return fail("disabled");
    }
    if crate::rec::is_recording() || crate::overlay::is_open() {
        return fail("busy");
    }
    #[cfg(windows)]
    {
        use znimok_platform::WindowList;
        let marker = r.get("marker").and_then(Value::as_str).unwrap_or("");
        if marker.is_empty() {
            return fail("not-found");
        }
        let cap = znimok_win::WinCapture::new();
        let found: Vec<_> = cap
            .windows()
            .unwrap_or_default()
            .into_iter()
            .filter(|w| w.title.contains(marker))
            .collect();
        let w = match found.as_slice() {
            [w] => w.clone(),
            [] => return fail("not-found"),
            _ => return fail("ambiguous"),
        };
        hub().reply(json!({"rec": "found", "rid": rid}));
        let display = znimok_win::raw::monitors()
            .into_iter()
            .find(|m| Some(&m.info.id) == w.display.as_ref())
            .or_else(|| znimok_win::raw::monitors().into_iter().next())
            .map(|m| m.info.bounds)
            .unwrap_or(w.bounds);
        // The mark is still in the title for a moment: start once the extension took it away.
        let rid2 = rid.clone();
        let (id, bounds) = (w.id.0, w.bounds);
        slint::Timer::single_shot(std::time::Duration::from_millis(250), move || {
            crate::rec::start(crate::rec::Choice {
                display,
                frame: bounds,
                window: Some(id),
                source: "browser",
            });
            if crate::rec::is_recording() {
                let log = crate::with_prefs(|p| p.video.devtools_log).unwrap_or(true);
                hub().reply(json!({"rec": "ok", "rid": rid2, "log": u8::from(log)}));
            } else {
                hub().reply(json!({"rec": "fail", "rid": rid2, "why": "failed"}));
            }
        });
    }
    #[cfg(not(windows))]
    fail("unsupported");
}
