//! Logging and local crash reports (ZK-32). No telemetry: nothing leaves the machine unless the
//! user opens the prefilled GitHub issue page and submits it themselves.
//!
//! ```no_run
//! let _log = znimok_log::init(znimok_log::Config::for_app("znimok-app", env!("CARGO_PKG_VERSION")));
//! tracing::info!("started");
//! // At start-up the UI may offer the reports of the last crash:
//! for r in znimok_log::pending_reports() {
//!     let _url = znimok_log::issue_url(&r); // "Open issue" button; the folder: znimok_log::crashes_dir()
//!     znimok_log::mark_seen(&r);
//! }
//! ```
//!
//! - **Log** — `tracing` into `<logs>/znimok.log.YYYY-MM-DD`, daily files, the last 7 kept; level
//!   from `ZNIMOK_LOG` (e.g. `debug`, `znimok_app=trace`), default `info`. Debug builds also print
//!   to stderr.
//! - **Panic** — the hook writes `<logs>/crashes/crash-<time>.txt`: version, OS, thread, message,
//!   place, backtrace and the tail of the log. The user's home folder is replaced by `~`.
//! - **Native crash on Windows** — an unhandled exception writes `crash-<time>.dmp` (a small
//!   minidump: threads and stacks, no heap — no screen pixels) and a `.txt` next to it.
//!   On macOS the system already writes `~/Library/Logs/DiagnosticReports/<app>-*.ips`;
//!   [`system_reports_dir`] points there.
//!
//! Locations: Windows `%LOCALAPPDATA%\Znimok\Logs`, macOS `~/Library/Logs/Znimok`,
//! elsewhere `$XDG_STATE_HOME/znimok` or `~/.local/state/znimok`.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::writer::MakeWriterExt;

#[cfg(windows)]
mod minidump;

/// How many daily log files are kept.
pub const KEEP_LOG_FILES: usize = 7;
/// How many crash reports are kept (oldest removed first).
pub const KEEP_REPORTS: usize = 20;
/// Lines of the log copied into a crash report.
const LOG_TAIL_LINES: usize = 60;
const ISSUES_URL: &str = "https://github.com/V-Plum/znimok/issues/new";

#[derive(Clone, Debug)]
pub struct Config {
    pub app: String,
    pub version: String,
    /// Where logs and `crashes/` go; `None` = the OS default ([`default_logs_dir`]).
    pub dir: Option<PathBuf>,
    /// Also log to stderr (default: in debug builds).
    pub stderr: bool,
}

impl Config {
    pub fn for_app(app: &str, version: &str) -> Self {
        Self {
            app: app.into(),
            version: version.into(),
            dir: None,
            stderr: cfg!(debug_assertions),
        }
    }
}

struct State {
    app: String,
    version: String,
    logs: PathBuf,
}

static STATE: OnceLock<State> = OnceLock::new();

/// Keeps the background log writer alive; drop it last (end of `main`) to flush the log.
pub struct Guard {
    _writer: Option<tracing_appender::non_blocking::WorkerGuard>,
}

/// Default logs folder of this OS.
pub fn default_logs_dir() -> PathBuf {
    if cfg!(windows) {
        let base = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        base.join("Znimok").join("Logs")
    } else if cfg!(target_os = "macos") {
        home().join("Library").join("Logs").join("Znimok")
    } else {
        std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join(".local").join("state"))
            .join("znimok")
    }
}

fn home() -> PathBuf {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

/// The logs folder in use (after [`init`]), else the default.
pub fn logs_dir() -> PathBuf {
    STATE
        .get()
        .map_or_else(default_logs_dir, |s| s.logs.clone())
}

pub fn crashes_dir() -> PathBuf {
    logs_dir().join("crashes")
}

/// Where the OS keeps its own crash reports of native crashes (macOS: DiagnosticReports).
pub fn system_reports_dir() -> Option<PathBuf> {
    cfg!(target_os = "macos").then(|| {
        home()
            .join("Library")
            .join("Logs")
            .join("DiagnosticReports")
    })
}

/// Start logging and install the crash handlers. Call once, first thing in `main`; a second
/// call only returns an empty guard.
pub fn init(cfg: Config) -> Guard {
    let logs = cfg.dir.clone().unwrap_or_else(default_logs_dir);
    if STATE
        .set(State {
            app: cfg.app.clone(),
            version: cfg.version.clone(),
            logs: logs.clone(),
        })
        .is_err()
    {
        return Guard { _writer: None };
    }
    let _ = fs::create_dir_all(logs.join("crashes"));
    let filter = EnvFilter::try_from_env("ZNIMOK_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    let appender = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("znimok.log")
        .max_log_files(KEEP_LOG_FILES)
        .build(&logs);
    let writer = match appender {
        Ok(a) => {
            let (nb, guard) = tracing_appender::non_blocking(a);
            let make = nb.and(std::io::stderr.with_filter(move |_| cfg.stderr));
            let _ = tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_ansi(false)
                .with_writer(make)
                .try_init();
            Some(guard)
        }
        Err(_) => {
            let _ = tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_writer(std::io::stderr)
                .try_init();
            None
        }
    };
    install_panic_hook();
    #[cfg(windows)]
    minidump::install(&logs.join("crashes"));
    tracing::info!(app = %cfg.app, version = %cfg.version, os = std::env::consts::OS, arch = std::env::consts::ARCH, "started");
    Guard { _writer: writer }
}

fn install_panic_hook() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let msg = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "(не текст)".into());
        let place = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_default();
        let bt = std::backtrace::Backtrace::force_capture();
        tracing::error!(thread = thread.name().unwrap_or("?"), %place, "panic: {msg}");
        let body = format!(
            "panic у потоці «{}»\nповідомлення: {msg}\nмісце: {place}\n\nbacktrace:\n{bt}",
            thread.name().unwrap_or("?")
        );
        let _ = write_report("panic", &body);
        prev(info);
    }));
}

/// Header of every report: app, version, OS, time.
fn header(kind: &str) -> String {
    let (app, version) = STATE
        .get()
        .map_or(("znimok", "?"), |s| (s.app.as_str(), s.version.as_str()));
    format!(
        "Znimok crash report\nkind: {kind}\napp: {app} {version}\nos: {} {}\ntime: {}\n",
        std::env::consts::OS,
        std::env::consts::ARCH,
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S %z")
    )
}

/// Write `crashes/crash-<time>.txt` with the header, `body` and the tail of the log. Returns its path.
pub(crate) fn write_report(kind: &str, body: &str) -> std::io::Result<PathBuf> {
    let dir = crashes_dir();
    fs::create_dir_all(&dir)?;
    let stamp = chrono::Local::now()
        .format("%Y%m%d-%H%M%S%.3f")
        .to_string()
        .replace('.', "-");
    let path = dir.join(format!("crash-{stamp}.txt"));
    let text = format!(
        "{}\n{}\n\nостанні рядки журналу:\n{}",
        header(kind),
        body,
        log_tail(LOG_TAIL_LINES)
    );
    let mut f = fs::File::create(&path)?;
    f.write_all(sanitize(&text).as_bytes())?;
    prune(&dir, KEEP_REPORTS);
    Ok(path)
}

/// Replace the home folder by `~` (user names are personal data).
pub fn sanitize(text: &str) -> String {
    let h = home().to_string_lossy().to_string();
    if h.len() < 3 {
        return text.to_string();
    }
    let mut out = text.replace(&h, "~");
    if cfg!(windows) {
        out = out.replace(&h.replace('\\', "/"), "~");
    }
    out
}

fn newest_log() -> Option<PathBuf> {
    let mut files: Vec<_> = fs::read_dir(logs_dir())
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("znimok.log"))
        })
        .collect();
    files.sort();
    files.pop()
}

/// Last `n` lines of the current log file.
pub fn log_tail(n: usize) -> String {
    let Some(p) = newest_log() else {
        return "(журналу немає)".into();
    };
    let text = fs::read_to_string(p).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

/// Keep only the `keep` newest crash reports (a `.dmp` goes with its `.txt`).
fn prune(dir: &Path, keep: usize) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut txt: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension().is_some_and(|e| e == "txt")
                && p.file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with("crash-"))
        })
        .collect();
    txt.sort();
    let excess = txt.len().saturating_sub(keep);
    for p in txt.into_iter().take(excess) {
        let _ = fs::remove_file(p.with_extension("dmp"));
        let _ = fs::remove_file(p.with_extension("seen"));
        let _ = fs::remove_file(p);
    }
}

/// A crash report the user has not been told about yet.
#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    pub path: PathBuf,
    /// Minidump next to it (Windows native crash).
    pub dump: Option<PathBuf>,
    pub text: String,
}

impl Report {
    /// First line after the header that says what happened.
    pub fn summary(&self) -> String {
        self.text
            .lines()
            .find(|l| l.starts_with("повідомлення:") || l.starts_with("exception:"))
            .map(|l| l.split_once(':').map_or(l, |x| x.1).trim().to_string())
            .unwrap_or_else(|| "збій".into())
    }
}

/// Reports not yet marked with [`mark_seen`], newest first.
pub fn pending_reports() -> Vec<Report> {
    let Ok(rd) = fs::read_dir(crashes_dir()) else {
        return Vec::new();
    };
    let mut out: Vec<Report> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "txt") && !p.with_extension("seen").exists())
        .filter_map(|p| {
            let text = fs::read_to_string(&p).ok()?;
            let dump = Some(p.with_extension("dmp")).filter(|d| d.exists());
            Some(Report {
                path: p,
                dump,
                text,
            })
        })
        .collect();
    out.sort_by(|a, b| b.path.cmp(&a.path));
    out
}

/// Don't offer this report again.
pub fn mark_seen(r: &Report) {
    let _ = fs::write(r.path.with_extension("seen"), b"");
}

fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// A "new issue" page on GitHub prefilled with the report (the user reviews and submits it; a
/// minidump, if any, is attached by hand). The body is cut to keep the URL within browser limits.
pub fn issue_url(r: &Report) -> String {
    let title = format!("Збій: {}", r.summary().chars().take(80).collect::<String>());
    let mut body: String = r.text.chars().take(5000).collect();
    if r.text.chars().count() > 5000 {
        body.push_str("\n…(обрізано — повний звіт у файлі)");
    }
    if r.dump.is_some() {
        body.push_str("\n\n(поруч зі звітом є .dmp — за бажання додайте його до issue)");
    }
    let body = format!("```\n{body}\n```");
    format!(
        "{ISSUES_URL}?labels=crash&title={}&body={}",
        percent_encode(&title),
        percent_encode(&body)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("znimok-log-test-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn panic_writes_a_report_with_log_tail_and_issue_link() {
        let dir = temp_dir("panic");
        let guard = init(Config {
            app: "znimok-test".into(),
            version: "0.0.0".into(),
            dir: Some(dir.clone()),
            stderr: false,
        });
        tracing::info!("line before the crash");
        drop(guard); // flush the non-blocking writer into the file
        let r = std::panic::catch_unwind(|| panic!("щось пішло не так"));
        assert!(r.is_err());
        let reports = pending_reports();
        assert_eq!(
            reports.len(),
            1,
            "{:?}",
            fs::read_dir(crashes_dir()).map(|d| d.count())
        );
        let rep = &reports[0];
        assert!(rep.text.contains("kind: panic") && rep.text.contains("app: znimok-test 0.0.0"));
        assert_eq!(rep.summary(), "щось пішло не так");
        assert!(rep.text.contains("line before the crash"), "{}", rep.text);
        let url = issue_url(rep);
        assert!(
            url.starts_with("https://github.com/V-Plum/znimok/issues/new?labels=crash&title=")
                && !url.contains(' ')
        );
        assert!(url.len() < 20_000);
        mark_seen(rep);
        assert!(pending_reports().is_empty());
        // Retention.
        for i in 0..(KEEP_REPORTS + 5) {
            let _ = write_report("test", &format!("n{i}"));
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let count = fs::read_dir(crashes_dir())
            .unwrap()
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "txt"))
            .count();
        assert_eq!(count, KEEP_REPORTS);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn home_folder_is_hidden_and_urls_are_encoded() {
        let h = home().to_string_lossy().to_string();
        assert_eq!(
            sanitize(&format!("at {h}{}x.rs", std::path::MAIN_SEPARATOR)),
            format!("at ~{}x.rs", std::path::MAIN_SEPARATOR)
        );
        assert_eq!(percent_encode("a b/ї"), "a%20b%2F%D1%97");
    }
}
