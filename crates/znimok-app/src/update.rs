//! Updates in the app (ZK-142), on top of `znimok-update` (ZK-122):
//!
//! - at start, the app confirms that this version came up (`mark_started`) — without it a
//!   running update rolls back to the previous version — and shows once what the last update did;
//! - with «Перевіряти щодня» on, a background check once a day; a newer release is reported once;
//! - «Встановити» (Windows): download and verify (signature, then checksum), hand the installer to
//!   `znimok.exe update install` and exit — it installs, starts the new version and rolls back if
//!   that does not come up. macOS installs through Sparkle (ZK-143); until then, the release page.
//!
//! Nothing touches the network unless the person turned the daily check on or pressed a button.

use std::path::PathBuf;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// `…\Znimok\Updates` — the same folder the command line uses.
pub fn updates_dir() -> PathBuf {
    znimok_agents::data_dir().join("Updates")
}

/// The app is up: confirm the start of this version to a running update.
pub fn confirm_start() {
    let _ = znimok_update::apply::mark_started(&updates_dir(), VERSION);
}

/// What the last update did, once: the note is moved aside after it is read.
pub fn take_outcome() -> Option<String> {
    let dir = updates_dir();
    let text = znimok_update::apply::last_outcome(&dir)?;
    let _ = std::fs::rename(
        dir.join("last-outcome.txt"),
        dir.join("last-outcome.shown.txt"),
    );
    let text = text.trim().to_string();
    (!text.is_empty()).then_some(text)
}

/// The result of a check.
#[derive(Clone, Debug)]
pub enum Found {
    UpToDate,
    Available(znimok_update::Available),
    /// No release key in this build yet (ZK-111).
    NotConfigured,
    Failed(String),
}

/// Checks on a worker thread; `then` runs on the UI thread.
pub fn check(then: impl FnOnce(Found) + Send + 'static) {
    std::thread::spawn(move || {
        // Without a release key the answer needs no HTTP client (making one may take long:
        // proxy discovery on Windows).
        let found = match znimok_update::Platform::current() {
            _ if !znimok_update::configured() => Found::NotConfigured,
            None => Found::Failed("no installer for this system".into()),
            Some(platform) => {
                let http = znimok_models::http::system();
                match znimok_update::check(http.as_ref(), VERSION, platform) {
                    Ok(Some(a)) => Found::Available(a),
                    Ok(None) => Found::UpToDate,
                    Err(znimok_update::UpdateError::NotConfigured) => Found::NotConfigured,
                    Err(e) => Found::Failed(e.to_string()),
                }
            }
        };
        let _ = slint::invoke_from_event_loop(move || then(found));
    });
}

/// Windows: downloads and verifies the installer, starts `znimok.exe update install` (it waits for
/// this process to exit) and asks the app to quit. `failed` gets the reason otherwise.
pub fn install(a: znimok_update::Available, failed: impl FnOnce(String) + Send + 'static) {
    std::thread::spawn(move || {
        let http = znimok_models::http::system();
        let r = znimok_update::download(http.as_ref(), &a, VERSION, &updates_dir())
            .map_err(|e| e.to_string())
            .and_then(|msi| run_installer(&msi, &a.version));
        let _ = slint::invoke_from_event_loop(move || match r {
            Ok(()) => {
                let _ = slint::quit_event_loop();
            }
            Err(e) => failed(e),
        });
    });
}

fn run_installer(msi: &std::path::Path, version: &str) -> Result<(), String> {
    let app = std::env::current_exe().map_err(|e| e.to_string())?;
    let cli = app.with_file_name(if cfg!(windows) {
        "znimok.exe"
    } else {
        "znimok"
    });
    if !cli.exists() {
        return Err(format!("{} is missing", cli.display()));
    }
    std::process::Command::new(&cli)
        .args(["update", "install", "--msi"])
        .arg(msi)
        .args(["--version", version, "--wait-pid"])
        .arg(std::process::id().to_string())
        .arg("--app")
        .arg(&app)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("{}: {e}", cli.display()))
}

/// The release page in the browser (macOS until Sparkle, ZK-143). Only GitHub pages of the repo.
pub fn open_page(url: &str) {
    if !url.starts_with("https://github.com/V-Plum/znimok/") {
        return;
    }
    #[cfg(windows)]
    let _ = std::process::Command::new("rundll32.exe")
        .arg("url.dll,FileProtocolHandler")
        .arg(url)
        .spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
}
