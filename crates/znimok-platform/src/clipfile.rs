//! «Копіювати як файл» (ZK-62): the picture as a real file next to the pixels on the clipboard,
//! so a messenger or Explorer / Finder that wants a file gets one with a readable name.
//!
//! Files live in `<temp>/Znimok/clip/<unique>/<name>.png`: one folder per copy, so the same name
//! can be copied twice; folders older than a day are removed on the next copy (the receiving
//! program has long read the file by then).

use crate::traits::{ClipImage, ClipItem};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const KEEP: Duration = Duration::from_secs(24 * 3600);

/// Where the temporary copies go.
pub fn clip_dir() -> PathBuf {
    std::env::temp_dir().join("Znimok").join("clip")
}

/// Writes `png` as `<name>.png` in a fresh folder under `root` and returns its path. `name` is
/// cleaned of characters no file system takes; empty → `Znimok`.
pub fn write_clip_file(root: &Path, name: &str, png: &[u8]) -> io::Result<PathBuf> {
    prune(root, KEEP);
    let stamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let dir = root.join(format!("{stamp:x}-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.png", file_stem(name)));
    std::fs::write(&path, png)?;
    Ok(path)
}

/// The items for «Копіювати як файл»: the picture (PNG + bitmap for programs that paste pixels)
/// and the file (for programs that attach files).
pub fn image_with_file(image: ClipImage, file: PathBuf) -> Vec<ClipItem> {
    vec![ClipItem::Image(image), ClipItem::Files(vec![file])]
}

/// A name every file system accepts: no `\ / : * ? " < > |` or control characters, no trailing
/// dots or spaces (Windows drops them), not a reserved device name, at most 120 characters.
pub fn file_stem(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| {
            if c.is_control() || r#"\/:*?"<>|"#.contains(c) {
                '_'
            } else {
                c
            }
        })
        .take(120)
        .collect();
    while s.ends_with(['.', ' ']) {
        s.pop();
    }
    let s = s.trim_start().to_string();
    let upper = s.to_ascii_uppercase();
    let base = upper.split('.').next().unwrap_or("");
    let reserved = matches!(base, "CON" | "PRN" | "AUX" | "NUL")
        || ((base.starts_with("COM") || base.starts_with("LPT"))
            && base.len() == 4
            && base.as_bytes()[3].is_ascii_digit());
    if s.is_empty() {
        "Znimok".into()
    } else if reserved {
        format!("_{s}")
    } else {
        s
    }
}

/// Removes copy folders older than `keep`. Errors are ignored: a file may still be open.
fn prune(root: &Path, keep: Duration) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    let now = SystemTime::now();
    for e in entries.flatten() {
        let old = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| now.duration_since(t).ok())
            .is_some_and(|age| age > keep);
        if old && e.path().is_dir() {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_cleaned() {
        assert_eq!(
            file_stem("Знімок 2026-09-28 22:15:00"),
            "Знімок 2026-09-28 22_15_00"
        );
        assert_eq!(file_stem(r#"a/b\c*?"<>|"#), "a_b_c______");
        assert_eq!(file_stem("кінець. . "), "кінець");
        assert_eq!(file_stem("   "), "Znimok");
        assert_eq!(file_stem("con"), "_con");
        assert_eq!(file_stem("COM1.png"), "_COM1.png");
        assert_eq!(file_stem("COMPUTER"), "COMPUTER");
        assert_eq!(file_stem(&"я".repeat(300)).chars().count(), 120);
    }

    #[test]
    fn same_name_twice_and_old_folders_go() {
        let root = std::env::temp_dir().join(format!("znimok-clipfile-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let a = write_clip_file(&root, "Знімок", b"one").unwrap();
        let b = write_clip_file(&root, "Знімок", b"two").unwrap();
        assert_ne!(a, b);
        assert_eq!(a.file_name(), b.file_name());
        assert_eq!(std::fs::read(&a).unwrap(), b"one");
        assert_eq!(std::fs::read(&b).unwrap(), b"two");
        prune(&root, Duration::ZERO);
        std::thread::sleep(Duration::from_millis(20));
        prune(&root, Duration::from_millis(1));
        assert!(!a.exists() && !b.exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
