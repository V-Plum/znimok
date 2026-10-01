//! Updates in the app (ZK-142), on top of `znimok-update` (ZK-122):
//!
//! - at start, the app confirms that this version came up (`mark_started`) — without it a
//!   running update rolls back to the previous version — and shows once what the last update did;
//! - with «Перевіряти щодня» on, a background check once a day; a newer release is reported once;
//! - «Встановити» (Windows): download and verify (signature, then checksum), hand the installer to
//!   `znimok.exe update install` and exit — it installs, starts the new version and rolls back if
//!   that does not come up.
//! - macOS (ZK-143): Sparkle 2 does the checking, the EdDSA-verified download, the installation
//!   and the relaunch; the page in the settings is Znimok's own (`mac` below, on
//!   `znimok_mac::sparkle`). Without the framework in the bundle (a dev build) or a feed, the
//!   page says updates are not set up.
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

/// What the last update did, once (ZK-211: only the note about this very version, or a fresh
/// rollback / failure; the note is moved aside after it is read).
pub fn take_outcome() -> Option<znimok_update::apply::Outcome> {
    let now = chrono::Utc::now().timestamp_millis();
    znimok_update::apply::take_outcome(&updates_dir(), VERSION, now, 24 * 3600 * 1000)
}

/// The result of a check.
#[derive(Clone, Debug)]
pub enum Found {
    UpToDate,
    /// Windows: a release with an installer to download.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    Available(znimok_update::Available),
    /// No release key in this build yet (ZK-111).
    NotConfigured,
    Failed(String),
    /// macOS: what Sparkle found (`size` in bytes; `notes` the release page).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Sparkle {
        version: String,
        notes: Option<String>,
        size: u64,
    },
}

/// Where a Sparkle update stands (macOS), for the page.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub enum Phase {
    Idle,
    Downloading { received: u64, total: u64 },
    Extracting(f64),
    Installing,
}

#[cfg(target_os = "macos")]
pub mod mac {
    //! The one Sparkle updater of the process, on the main thread.
    use std::cell::RefCell;

    pub use znimok_mac::sparkle::{Choice, Event};
    use znimok_mac::sparkle::{Sparkle, framework_in_bundle};

    thread_local! {
        static SPARKLE: RefCell<Option<Sparkle>> = const { RefCell::new(None) };
    }

    /// Start Sparkle when the bundle carries it and a feed is set (`SUFeedURL`, or
    /// `ZNIMOK_SPARKLE_FEED` in the environment for tests). `false` = updates not set up.
    pub fn init() -> bool {
        let Some(fw) = framework_in_bundle() else {
            return false;
        };
        let feed = std::env::var("ZNIMOK_SPARKLE_FEED").ok();
        match Sparkle::start(&fw, None, feed.as_deref()) {
            Ok(s) => {
                SPARKLE.with(|c| *c.borrow_mut() = Some(s));
                true
            }
            Err(e) => {
                eprintln!("Sparkle: {e}");
                false
            }
        }
    }

    pub fn available() -> bool {
        SPARKLE.with(|c| c.borrow().is_some())
    }

    /// A check: by the person (Sparkle reports «up to date» too) or in the background.
    pub fn check(user_initiated: bool) -> bool {
        SPARKLE.with(|c| match c.borrow().as_ref() {
            Some(s) if s.can_check() => {
                s.check(user_initiated);
                true
            }
            _ => false,
        })
    }

    pub fn reply(choice: Choice) -> bool {
        SPARKLE.with(|c| c.borrow().as_ref().is_some_and(|s| s.reply(choice)))
    }

    pub fn poll() -> Vec<Event> {
        SPARKLE.with(|c| c.borrow().as_ref().map(|s| s.poll()).unwrap_or_default())
    }
}

/// Checks on a worker thread; `then` runs on the UI thread (Windows; macOS asks Sparkle).
#[cfg_attr(target_os = "macos", allow(dead_code))]
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
#[cfg_attr(target_os = "macos", allow(dead_code))]
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
