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
                .or_else(|| crate::commands::hub().handle(method, &params))
                .or_else(|| crate::agentipc::handle(method, &params))
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
    if screen_permission_missing() {
        return fail("no-permission");
    }
    #[cfg(any(windows, target_os = "macos"))]
    {
        let marker = r.get("marker").and_then(Value::as_str).unwrap_or("");
        if marker.is_empty() {
            return fail("not-found");
        }
        find_marked(r.clone(), marker.to_string(), 0);
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    fail("unsupported");
}

/// How many times the mark is looked for, 150 ms apart (ZK-291).
#[cfg(any(windows, target_os = "macos"))]
const MARK_TRIES: u32 = 10;

/// Looks for the window whose title carries the mark. On macOS the browser hands its new title
/// to the window server a moment later, so a first look often finds the old one (ZK-291): look
/// again, then by the window's bounds the extension sent (points, as macOS has them).
#[cfg(any(windows, target_os = "macos"))]
fn find_marked(r: Value, marker: String, tries: u32) {
    let rid = r.get("rid").cloned().unwrap_or(Value::Null);
    let fail = |why: &str| hub().reply(json!({"rec": "fail", "rid": rid, "why": why}));
    let (windows, displays) = windows_and_displays();
    let found: Vec<_> = windows
        .iter()
        .filter(|w| w.title.contains(&marker))
        .cloned()
        .collect();
    let w = match found.as_slice() {
        [w] => w.clone(),
        [_, _, ..] => return fail("ambiguous"),
        [] if tries + 1 < MARK_TRIES => {
            slint::Timer::single_shot(std::time::Duration::from_millis(150), move || {
                find_marked(r, marker, tries + 1)
            });
            return;
        }
        [] => match by_bounds(&r, &windows) {
            Some(w) => w,
            None => {
                tracing::warn!(
                    tries,
                    "record this window: no window with the mark {marker}"
                );
                return fail("not-found");
            }
        },
    };
    if tries > 0 {
        tracing::info!(
            tries,
            "record this window: found after waiting for the title"
        );
    }
    record_found(&r, w, &displays);
}

/// The browser's window by the bounds the extension sent (`wl`, `wt`, `ww`, `wh`): on macOS the
/// browser's window coordinates are the system's points, so the window is the one with the same
/// frame (±2). Only one such window, else nothing. On Windows the browser counts in its own
/// device-independent pixels, which are not the system's: not used there.
#[cfg(any(windows, target_os = "macos"))]
fn by_bounds(
    r: &Value,
    windows: &[znimok_platform::WindowInfo],
) -> Option<znimok_platform::WindowInfo> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let n = |k: &str| r.get(k).and_then(Value::as_f64).map(|v| v.round() as i64);
    let (x, y, w, h) = (n("wl")?, n("wt")?, n("ww")?, n("wh")?);
    let near = |a: i64, b: i64| (a - b).abs() <= 2;
    let found: Vec<_> = windows
        .iter()
        .filter(|win| {
            !win.own
                && near(i64::from(win.bounds.x), x)
                && near(i64::from(win.bounds.y), y)
                && near(i64::from(win.bounds.width), w)
                && near(i64::from(win.bounds.height), h)
        })
        .collect();
    match found.as_slice() {
        [one] => {
            tracing::info!(app = %one.app, "record this window: found by its bounds");
            Some((*one).clone())
        }
        _ => None,
    }
}

/// The browser's window is found: its display, then the recording once the mark is gone.
#[cfg(any(windows, target_os = "macos"))]
fn record_found(
    r: &Value,
    w: znimok_platform::WindowInfo,
    displays: &[znimok_platform::DisplayInfo],
) {
    let rid = r.get("rid").cloned().unwrap_or(Value::Null);
    {
        hub().reply(json!({"rec": "found", "rid": rid}));
        // Its display: as the system says, else the one its centre is on.
        let (cx, cy) = (
            w.bounds.x + w.bounds.width as i32 / 2,
            w.bounds.y + w.bounds.height as i32 / 2,
        );
        let display = displays
            .iter()
            .find(|d| Some(&d.id) == w.display.as_ref())
            .or_else(|| {
                displays.iter().find(|d| {
                    let b = d.bounds;
                    cx >= b.x
                        && cx < b.x + b.width as i32
                        && cy >= b.y
                        && cy < b.y + b.height as i32
                })
            })
            .or_else(|| displays.first())
            .map(|d| d.bounds)
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
}

/// Whether the browser's log belongs to this recording (ZK-306): never data of a program that
/// is not in front. A recording started from the extension («Record this window») — yes; a
/// window — only a browser's; the screen or a region — only when a browser has the focus. A
/// game recorded with a browser left behind it wrote that browser's log (owner, 12.10.2026).
pub(crate) fn log_belongs(source: &str, window: Option<u64>, frame: znimok_platform::Rect) -> bool {
    if source == "browser" {
        return true;
    }
    #[cfg(any(windows, target_os = "macos"))]
    {
        let app = match window {
            Some(id) => windows_and_displays()
                .0
                .into_iter()
                .find(|w| w.id.0 == id)
                .map(|w| w.app),
            None => foreground_app(frame),
        };
        let ok = app.as_deref().is_some_and(is_browser);
        tracing::info!(source, ?app, ok, "recording: the browser's log");
        ok
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = (window, frame);
        false
    }
}

/// Before the recording overlay (ZK-352): a browser in front gets its debugger attached now, so
/// its «is debugging» bar, which pushes the page down, is on screen before the region is
/// chosen. Returns whether the extension was asked — the overlay then waits for the bar.
pub(crate) fn arm_browser() -> bool {
    if !crate::with_prefs(|p| p.video.devtools_log).unwrap_or(false) {
        return false;
    }
    #[cfg(any(windows, target_os = "macos"))]
    {
        let app = foreground_app(znimok_platform::Rect::default());
        let browser = app.as_deref().is_some_and(is_browser);
        tracing::info!(
            ?app,
            browser,
            "recording overlay: the browser's debugger goes first"
        );
        browser && hub().arm()
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    false
}

/// The overlay was cancelled, or the recording is not the browser's (ZK-352).
pub(crate) fn disarm_browser() {
    hub().disarm();
}

/// A browser the extension runs in (Chromium's family), by its executable or bundle name.
pub(crate) fn is_browser(app: &str) -> bool {
    let a = app.to_lowercase();
    [
        "chrome",
        "chromium",
        "msedge",
        "microsoft edge",
        "brave",
        "vivaldi",
        "opera",
        "arc",
    ]
    .iter()
    .any(|b| {
        if *b == "arc" {
            a == "arc" || a == "arc.exe"
        } else {
            a.contains(b)
        }
    })
}

/// The program whose window has the focus now.
/// Windows: when that is Znimok itself (its overlay has just gone), the topmost other window over
/// the recorded part — the list is in z-order, front first.
#[cfg(windows)]
fn foreground_app(frame: znimok_platform::Rect) -> Option<String> {
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    let mut pid = 0u32;
    // SAFETY: plain queries; the pid is written into our variable.
    unsafe {
        let h = GetForegroundWindow();
        if h.0.is_null() {
            return None;
        }
        GetWindowThreadProcessId(h, Some(&mut pid));
    }
    let windows = windows_and_displays().0;
    if pid != std::process::id()
        && let Some(w) = windows.iter().find(|w| w.pid == pid && !w.own)
    {
        return Some(w.app.clone());
    }
    let over = |w: &znimok_platform::WindowInfo| {
        let b = w.bounds;
        b.x < frame.x + frame.width as i32
            && frame.x < b.x + b.width as i32
            && b.y < frame.y + frame.height as i32
            && frame.y < b.y + b.height as i32
    };
    windows
        .into_iter()
        .find(|w| !w.own && !w.minimized && w.pid != std::process::id() && over(w))
        .map(|w| w.app)
}

/// The application in front now (Znimok itself right after its overlay — then no log).
#[cfg(target_os = "macos")]
fn foreground_app(_frame: znimok_platform::Rect) -> Option<String> {
    let app = objc2_app_kit::NSWorkspace::sharedWorkspace().frontmostApplication()?;
    app.localizedName().map(|n| n.to_string())
}

/// The windows on screen (with their titles) and the displays.
#[cfg(windows)]
pub(crate) fn windows_and_displays() -> (
    Vec<znimok_platform::WindowInfo>,
    Vec<znimok_platform::DisplayInfo>,
) {
    use znimok_platform::WindowList;
    let windows = znimok_win::WinCapture::new().windows().unwrap_or_default();
    let displays = znimok_win::raw::monitors()
        .into_iter()
        .map(|m| m.info)
        .collect();
    (windows, displays)
}

/// macOS: no «Screen Recording» permission — ScreenCaptureKit then lists no window at all, and a
/// window looked for is «not found» (ZK-301: the first Developer ID build is another app for TCC,
/// the old permission no longer counts). Asks for it (the system prompt the first time, the
/// settings pane after that) and says so.
#[cfg(target_os = "macos")]
pub(crate) fn screen_permission_missing() -> bool {
    use znimok_platform::{Permission, PermissionState, Permissions};
    let cap = znimok_mac::MacCapture::new();
    if cap.status(Permission::ScreenRecording) == PermissionState::Granted
        || cap.request(Permission::ScreenRecording) == PermissionState::Granted
    {
        return false;
    }
    tracing::warn!("no Screen Recording permission: the settings pane is opened");
    let _ = cap.open_settings(Permission::ScreenRecording);
    true
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn screen_permission_missing() -> bool {
    false
}

/// The windows on screen (titles need «Screen Recording», which recording has anyway) and the
/// displays, in points (ZK-207).
#[cfg(target_os = "macos")]
pub(crate) fn windows_and_displays() -> (
    Vec<znimok_platform::WindowInfo>,
    Vec<znimok_platform::DisplayInfo>,
) {
    use znimok_platform::{Capture, WindowList};
    let cap = znimok_mac::MacCapture::new();
    (
        cap.windows().unwrap_or_default(),
        cap.displays().unwrap_or_default(),
    )
}

#[cfg(test)]
mod log_owner_tests {
    /// ZK-306: the log is written only for a browser — by its executable (Windows) or its
    /// application name (macOS); a game, an editor or Znimok itself is not one.
    #[test]
    fn a_browser_is_known_by_its_name() {
        for b in [
            "chrome.exe",
            "Google Chrome",
            "Google Chrome Canary",
            "msedge.exe",
            "Microsoft Edge",
            "brave.exe",
            "Brave Browser",
            "vivaldi.exe",
            "opera.exe",
            "Arc",
            "chromium",
        ] {
            assert!(super::is_browser(b), "{b}");
        }
        for o in [
            "MaydayMail.exe",
            "Mayday Mail",
            "znimok-app.exe",
            "Code.exe",
            "Finder",
            "Archive Utility",
            "search.exe",
        ] {
            assert!(!super::is_browser(o), "{o}");
        }
    }
}
