//! Journal of what agents did (ZK-69): one JSON line per tool call in `<data>/agents-audit.jsonl`,
//! kept 90 days. It records who, what, when, whether a picture was taken and how it ended — never
//! the pictures, texts or arguments themselves (only a document id).

use crate::permissions::{Grant, Scope};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const KEEP_DAYS: i64 = 90;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// Unix milliseconds.
    pub ts: i64,
    pub client: String,
    pub tool: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<Scope>,
    /// `None` = refused.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grant: Option<Grant>,
    /// A screenshot was taken.
    pub capture: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document: Option<String>,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub struct Audit {
    path: PathBuf,
    lock: Mutex<()>,
}

impl Audit {
    /// Opens the journal and drops entries older than [`KEEP_DAYS`].
    pub fn open(path: impl Into<PathBuf>) -> Self {
        let a = Self {
            path: path.into(),
            lock: Mutex::new(()),
        };
        a.prune(chrono::Utc::now().timestamp_millis());
        a
    }

    pub fn open_default() -> Option<Self> {
        znimok_settings::Dirs::system().map(|d| Self::open(d.data.join("agents-audit.jsonl")))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn record(&self, e: &Entry) {
        let _g = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(d) = self.path.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            && let Ok(mut line) = serde_json::to_vec(e)
        {
            line.push(b'\n');
            let _ = f.write_all(&line);
        }
    }

    pub fn entries(&self) -> Vec<Entry> {
        std::fs::read_to_string(&self.path)
            .unwrap_or_default()
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
    }

    fn prune(&self, now_ms: i64) {
        let _g = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        let Ok(text) = std::fs::read_to_string(&self.path) else {
            return;
        };
        let cutoff = now_ms - KEEP_DAYS * 24 * 3600 * 1000;
        let kept: Vec<&str> = text
            .lines()
            .filter(|l| serde_json::from_str::<Entry>(l).is_ok_and(|e| e.ts >= cutoff))
            .collect();
        if kept.len() != text.lines().count() {
            let mut out = kept.join("\n");
            if !out.is_empty() {
                out.push('\n');
            }
            let _ = std::fs::write(&self.path, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(ts: i64) -> Entry {
        Entry {
            ts,
            client: "Claude Code".into(),
            tool: "capture_screen".into(),
            scope: Some(Scope::Capture),
            grant: Some(Grant::Session),
            capture: true,
            document: Some("0b3f…".into()),
            ok: true,
            error: None,
        }
    }

    #[test]
    fn records_and_keeps_ninety_days() {
        let p = std::env::temp_dir().join(format!("znimok-audit-{}.jsonl", std::process::id()));
        let _ = std::fs::remove_file(&p);
        let now = chrono::Utc::now().timestamp_millis();
        let a = Audit::open(&p);
        a.record(&entry(now - 100 * 24 * 3600 * 1000));
        a.record(&entry(now - 1000));
        assert_eq!(a.entries().len(), 2);
        let a = Audit::open(&p);
        let e = a.entries();
        assert_eq!(e.len(), 1, "the 100-day-old one is gone");
        assert_eq!(e[0], entry(now - 1000));
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(!text.contains("\"arguments\""));
        let _ = std::fs::remove_file(p);
    }
}
