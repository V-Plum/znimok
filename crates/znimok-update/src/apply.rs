//! Installing a verified update on Windows, with a way back (ZK-122).
//!
//! The app hands over to `znimok.exe update install` and exits. That command copies itself out of
//! the install folder (the MSI replaces the files there, a running exe would block it) and, from
//! the copy:
//!
//! 1. waits for the app to exit;
//! 2. `msiexec /i <new> /qn /norestart` — per user, no admin rights, the MSI's MajorUpgrade
//!    replaces the old version;
//! 3. starts the new app and waits for it to say it came up ([`mark_started`], called by the app
//!    at startup);
//! 4. if it did not: `msiexec /x <new>`, then `msiexec /i <previous>` — the previous MSI is kept
//!    after every successful update ([`installed_dir`]) — and starts that;
//! 5. writes the outcome for the app to show ([`last_outcome`]).
//!
//! Everything lives in the person's own `…\Znimok\Updates` folder, never a shared temp folder.

use std::path::{Path, PathBuf};
use std::time::Duration;

/// How long the new version has to come up.
pub const START_WAIT: Duration = Duration::from_secs(60);

/// Where the MSI of the running version is kept for a way back.
pub fn installed_dir(updates: &Path) -> PathBuf {
    updates.join("installed")
}

fn marker(updates: &Path, version: &str) -> PathBuf {
    updates.join(format!("started-{version}"))
}

/// Called by the app when it has started (its window is up): tells a running update that the new
/// version works. Cheap and harmless when no update is running.
pub fn mark_started(updates: &Path, version: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(updates)?;
    std::fs::write(marker(updates, version), b"")
}

/// The note `install` leaves for the app: the outcome and when (ms since the epoch).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recorded {
    pub outcome: Outcome,
    pub time_ms: i64,
}

fn note_path(updates: &Path) -> PathBuf {
    updates.join("last-outcome.txt")
}

/// What the last update did (`None` when nothing to tell, or a note this version cannot read).
pub fn last_outcome(updates: &Path) -> Option<Recorded> {
    let text = std::fs::read_to_string(note_path(updates)).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let s = |k: &str| {
        v.get(k)
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let outcome = match s("kind").as_str() {
        "installed" => Outcome::Installed {
            version: s("version"),
        },
        "rolled_back" => Outcome::RolledBack {
            version: s("version"),
            reason: s("reason"),
        },
        "failed" => Outcome::Failed {
            reason: s("reason"),
        },
        _ => return None,
    };
    Some(Recorded {
        outcome,
        time_ms: v
            .get("time_ms")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0),
    })
}

/// The note as the running app should show it, once (the note goes either way — ZK-211: a
/// note left by an older version was shown after a later update): «installed» only when the
/// version that runs is the one installed; the other outcomes only while fresh (`max_age`) —
/// after a rollback the running version is the previous one.
pub fn take_outcome(
    updates: &Path,
    running: &str,
    now_ms: i64,
    max_age_ms: i64,
) -> Option<Outcome> {
    let r = last_outcome(updates);
    let _ = std::fs::rename(note_path(updates), updates.join("last-outcome.shown.txt"));
    let r = r?;
    let fresh = now_ms - r.time_ms <= max_age_ms;
    match &r.outcome {
        Outcome::Installed { version } if version == running => Some(r.outcome),
        Outcome::Installed { .. } => None,
        _ if fresh => Some(r.outcome),
        _ => None,
    }
}

/// Writes the note for the app.
pub fn record(updates: &Path, outcome: &Outcome) {
    let time_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64);
    let v = match outcome {
        Outcome::Installed { version } => {
            serde_json::json!({"kind": "installed", "version": version, "time_ms": time_ms})
        }
        Outcome::RolledBack { version, reason } => serde_json::json!(
            {"kind": "rolled_back", "version": version, "reason": reason, "time_ms": time_ms}
        ),
        Outcome::Failed { reason } => {
            serde_json::json!({"kind": "failed", "reason": reason, "time_ms": time_ms})
        }
    };
    let _ = std::fs::write(note_path(updates), v.to_string());
}

/// Whether msiexec succeeded (0; 3010 / 1641 = success, restart wanted).
pub fn msi_ok(code: i32) -> bool {
    matches!(code, 0 | 3010 | 1641)
}

/// An MSI we may install: a file directly in the updates folder or in its `installed` folder,
/// with the release name pattern — never a path the caller made up.
pub fn allowed_msi(updates: &Path, msi: &Path) -> bool {
    let name_ok = msi
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with("Znimok-") && n.ends_with("-windows-x64.msi"));
    let parent = msi.parent().and_then(|p| p.canonicalize().ok());
    let inside = [updates.to_path_buf(), installed_dir(updates)]
        .iter()
        .filter_map(|d| d.canonicalize().ok())
        .any(|d| Some(&d) == parent.as_ref());
    name_ok && inside && msi.is_file()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Installed {
        version: String,
    },
    /// The new version did not start; the previous one is back.
    RolledBack {
        version: String,
        reason: String,
    },
    Failed {
        reason: String,
    },
}

#[cfg(windows)]
pub use win::{install, run_detached_copy, wait_for_exit};

#[cfg(windows)]
mod win {
    use super::*;
    use std::process::Command;
    use std::time::Instant;

    fn msiexec(args: &[&std::ffi::OsStr], log: &Path) -> i32 {
        let mut c = Command::new("msiexec");
        c.args(args)
            .arg("/qn")
            .arg("/norestart")
            .arg("/l*v")
            .arg(log);
        c.status().ok().and_then(|s| s.code()).unwrap_or(-1)
    }

    /// Waits (at most `limit`) for the process `pid` to exit; `true` when it has.
    pub fn wait_for_exit(pid: u32, limit: Duration) -> bool {
        use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
        use windows::Win32::System::Threading::{
            OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
        };
        // SAFETY: a plain handle to wait on, closed below.
        let Ok(h) = (unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) }) else {
            return true; // gone already
        };
        // SAFETY: valid handle.
        let r = unsafe { WaitForSingleObject(h, limit.as_millis().min(u32::MAX as u128) as u32) };
        // SAFETY: our handle.
        let _ = unsafe { CloseHandle(h) };
        r == WAIT_OBJECT_0
    }

    fn started(updates: &Path, app: &[String], version: &str, wait: Duration) -> bool {
        let m = marker(updates, version);
        let _ = std::fs::remove_file(&m);
        let Some((exe, args)) = app.split_first() else {
            return false;
        };
        if Command::new(exe).args(args).spawn().is_err() {
            return false;
        }
        let t0 = Instant::now();
        while t0.elapsed() < wait {
            if m.exists() {
                let _ = std::fs::remove_file(&m);
                return true;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        false
    }

    /// Steps 2–5 of the module doc. `app` = the installed `znimok-app.exe`.
    pub fn install(
        updates: &Path,
        msi: &Path,
        version: &str,
        app: &[String],
        wait: Duration,
    ) -> Outcome {
        let outcome = run(updates, msi, version, app, wait);
        record(updates, &outcome);
        outcome
    }

    fn run(updates: &Path, msi: &Path, version: &str, app: &[String], wait: Duration) -> Outcome {
        if !allowed_msi(updates, msi) {
            return Outcome::Failed {
                reason: format!("{} is not a downloaded update", msi.display()),
            };
        }
        let prev_dir = installed_dir(updates);
        let previous = std::fs::read_dir(&prev_dir).ok().and_then(|d| {
            d.filter_map(Result::ok)
                .map(|e| e.path())
                .find(|p| allowed_msi(updates, p))
        });
        let code = msiexec(
            &["/i".as_ref(), msi.as_os_str()],
            &updates.join(format!("install-{version}.log")),
        );
        if !msi_ok(code) {
            return Outcome::Failed {
                reason: format!("msiexec exit code {code}"),
            };
        }
        if started(updates, app, version, wait) {
            // The way back for the next update: this version's MSI.
            let _ = std::fs::remove_dir_all(&prev_dir);
            if std::fs::create_dir_all(&prev_dir).is_ok()
                && let Some(name) = msi.file_name()
            {
                let _ = std::fs::copy(msi, prev_dir.join(name));
            }
            let _ = std::fs::remove_file(msi);
            return Outcome::Installed {
                version: version.to_string(),
            };
        }
        let Some(previous) = previous else {
            return Outcome::Failed {
                reason: format!(
                    "{version} did not start and there is no previous installer to go back to"
                ),
            };
        };
        let _ = msiexec(
            &["/x".as_ref(), msi.as_os_str()],
            &updates.join(format!("uninstall-{version}.log")),
        );
        let code = msiexec(
            &["/i".as_ref(), previous.as_os_str()],
            &updates.join("rollback.log"),
        );
        if !msi_ok(code) {
            return Outcome::Failed {
                reason: format!("{version} did not start; going back failed (msiexec {code})"),
            };
        }
        if let Some(exe) = app.first() {
            let _ = Command::new(exe).spawn();
        }
        Outcome::RolledBack {
            version: version.to_string(),
            reason: "no start signal".into(),
        }
    }

    /// Runs `exe args` from a copy in the updates folder (the MSI replaces the original) and
    /// returns at once; the copy waits for `wait_pid` to exit first.
    pub fn run_detached_copy(updates: &Path, exe: &Path, args: &[String]) -> std::io::Result<()> {
        let dir = updates.join("runner");
        std::fs::create_dir_all(&dir)?;
        let copy = dir.join("znimok-update-runner.exe");
        std::fs::copy(exe, &copy)?;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        use std::os::windows::process::CommandExt;
        Command::new(copy)
            .args(args)
            .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
            .spawn()
            .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ZK-211: the note is shown once, and only to the version it is about — a note left by an
    /// older version (the app updated by hand since) goes unseen; a rollback or a failure shows
    /// while fresh, whatever version runs.
    #[test]
    fn outcome_note_is_for_its_version_and_shown_once() {
        let updates =
            std::env::temp_dir().join(format!("znimok-outcome-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&updates);
        std::fs::create_dir_all(&updates).unwrap();
        let day = 24 * 3600 * 1000;
        let now = 1_790_000_000_000;
        let installed = |v: &str| Outcome::Installed {
            version: v.to_string(),
        };
        record(&updates, &installed("0.0.4"));
        let r = last_outcome(&updates).unwrap();
        assert_eq!(r.outcome, installed("0.0.4"));
        assert!(r.time_ms > now);
        // Read by 0.0.5 (installed by hand after): nothing, and the note is gone.
        assert_eq!(take_outcome(&updates, "0.0.5", now, day), None);
        assert_eq!(
            take_outcome(&updates, "0.0.4", now, day),
            None,
            "shown once"
        );
        record(&updates, &installed("0.0.5"));
        assert_eq!(
            take_outcome(&updates, "0.0.5", now, day),
            Some(installed("0.0.5"))
        );
        // A failure is shown while fresh (the note's time is now), by any version.
        let failed = Outcome::Failed {
            reason: "msiexec exit code 1603".into(),
        };
        record(&updates, &failed);
        assert_eq!(
            take_outcome(&updates, "0.0.4", r.time_ms + 1000, day),
            Some(failed.clone())
        );
        record(&updates, &failed);
        assert_eq!(
            take_outcome(&updates, "0.0.4", r.time_ms + 3 * day, day),
            None,
            "stale"
        );
        // A note this version cannot read (the old plain text) is dropped quietly.
        std::fs::write(updates.join("last-outcome.txt"), "installed 0.0.3").unwrap();
        assert_eq!(take_outcome(&updates, "0.0.3", now, day), None);
        assert!(!updates.join("last-outcome.txt").exists());
        let _ = std::fs::remove_dir_all(&updates);
    }

    #[test]
    fn exit_codes() {
        assert!(msi_ok(0) && msi_ok(3010) && msi_ok(1641));
        assert!(!msi_ok(1602) && !msi_ok(1603) && !msi_ok(-1));
    }

    #[test]
    fn only_downloaded_installers() {
        let updates =
            std::env::temp_dir().join(format!("znimok-apply-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&updates);
        std::fs::create_dir_all(installed_dir(&updates)).unwrap();
        let good = updates.join("Znimok-1.2.0-windows-x64.msi");
        let kept = installed_dir(&updates).join("Znimok-1.1.0-windows-x64.msi");
        let odd = updates.join("setup.msi");
        for p in [&good, &kept, &odd] {
            std::fs::write(p, b"x").unwrap();
        }
        assert!(allowed_msi(&updates, &good));
        assert!(allowed_msi(&updates, &kept));
        assert!(!allowed_msi(&updates, &odd), "not a release name");
        let elsewhere = std::env::temp_dir().join("Znimok-9.9.9-windows-x64.msi");
        std::fs::write(&elsewhere, b"x").unwrap();
        assert!(
            !allowed_msi(&updates, &elsewhere),
            "outside the updates folder"
        );
        assert!(
            !allowed_msi(&updates, &updates.join("Znimok-2.0.0-windows-x64.msi")),
            "missing"
        );
        let _ = std::fs::remove_file(elsewhere);

        mark_started(&updates, "1.2.0").unwrap();
        assert!(marker(&updates, "1.2.0").exists());
        let _ = std::fs::remove_dir_all(&updates);
    }
}
