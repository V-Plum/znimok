//! `settings.json`: lenient load, atomic save, keys of newer versions kept.

use crate::model::Settings;
use serde_json::{Map, Value};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

/// A settings file larger than this is not read (it is not ours or it is damaged).
const MAX_FILE: u64 = 1 << 20;

/// Something in the file that could not be used as it was.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// The file is unreadable or not a JSON object; defaults are in use. Before the first save
    /// overwrites it, the file is copied to `<name>.broken-<unix seconds>`.
    Unreadable(String),
    /// A value of the wrong type or kind (dotted name); its default is in use.
    Invalid(String),
    /// A value out of its range, brought into it (dotted name).
    Adjusted(String),
}

/// Settings of this user, shared by the app, the CLI and the MCP server.
///
/// Every [`update`](Self::update) re-reads the file first, so a change saved meanwhile by another
/// process is not lost (last writer wins per field, not per file). Nothing is written until the
/// first change: a missing file just means defaults.
pub struct Store {
    path: PathBuf,
    inner: Mutex<Inner>,
}

struct Inner {
    settings: Settings,
    /// The file as read: keys this version does not know survive a save.
    raw: Map<String, Value>,
    problems: Vec<Problem>,
    /// The file on disk could not be parsed; keep a copy before replacing it.
    broken: bool,
}

impl Store {
    pub fn open(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let inner = read(&path);
        Self {
            path,
            inner: Mutex::new(inner),
        }
    }

    /// `settings.json` in [`Dirs::system`](crate::Dirs::system).
    pub fn open_default() -> Option<Self> {
        crate::Dirs::system().map(|d| Self::open(d.settings_file()))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn get(&self) -> Settings {
        self.lock().settings.clone()
    }

    /// What was wrong with the file at the last read (for the log and a hint in the settings page).
    pub fn problems(&self) -> Vec<Problem> {
        self.lock().problems.clone()
    }

    /// Re-reads the file (after another process saved it). `true` if the settings changed.
    pub fn reload(&self) -> bool {
        let fresh = read(&self.path);
        let mut g = self.lock();
        let changed = fresh.settings != g.settings;
        *g = fresh;
        changed
    }

    /// Changes the settings and saves them. The closure sees the latest file contents; values it
    /// puts out of range are brought back ([`Settings::sanitize`]). Returns the saved settings.
    pub fn update(&self, f: impl FnOnce(&mut Settings)) -> io::Result<Settings> {
        let mut g = self.lock();
        let fresh = read(&self.path);
        if fresh.settings != g.settings || fresh.raw != g.raw || fresh.broken {
            *g = fresh;
        }
        let mut next = g.settings.clone();
        f(&mut next);
        next.sanitize();
        let on_disk_ok = self.path.exists() && !g.broken;
        if next == g.settings && on_disk_ok {
            return Ok(next);
        }
        if g.broken {
            keep_broken_copy(&self.path);
        }
        let mut raw = g.raw.clone();
        let Value::Object(ours) = serde_json::to_value(&next).map_err(io::Error::other)? else {
            unreachable!("settings serialize to an object")
        };
        merge(&mut raw, ours);
        let mut text = serde_json::to_vec_pretty(&Value::Object(raw.clone()))?;
        text.push(b'\n');
        write_atomic(&self.path, &text)?;
        *g = Inner {
            settings: next.clone(),
            raw,
            problems: Vec::new(),
            broken: false,
        };
        Ok(next)
    }

    /// Back to the defaults (keys of newer versions stay).
    pub fn reset(&self) -> io::Result<Settings> {
        self.update(|s| *s = Settings::default())
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }
}

fn read(path: &Path) -> Inner {
    let defaults = |problems, broken| Inner {
        settings: Settings::default(),
        raw: Map::new(),
        problems,
        broken,
    };
    let bytes = match fs::metadata(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return defaults(Vec::new(), false),
        Ok(m) if m.len() > MAX_FILE => {
            let p = Problem::Unreadable(format!("{} bytes", m.len()));
            return defaults(vec![p], true);
        }
        _ => match fs::read(path) {
            Ok(b) => b,
            // Unreadable now (locked, no access) — do not treat it as broken and overwrite it.
            Err(e) => return defaults(vec![Problem::Unreadable(e.to_string())], false),
        },
    };
    let text = String::from_utf8_lossy(&bytes);
    match serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')) {
        Ok(Value::Object(raw)) => {
            let (settings, mut problems) = lenient(&raw);
            let mut settings = settings;
            problems.extend(settings.sanitize().into_iter().map(Problem::Adjusted));
            Inner {
                settings,
                raw,
                problems,
                broken: false,
            }
        }
        Ok(_) => defaults(vec![Problem::Unreadable("not a JSON object".into())], true),
        Err(e) => defaults(vec![Problem::Unreadable(e.to_string())], true),
    }
}

/// Takes every value from `file` that fits, one leaf at a time; the rest stay default. Sections
/// and nested groups are walked; a leaf is anything the defaults do not have as an object.
fn lenient(file: &Map<String, Value>) -> (Settings, Vec<Problem>) {
    let Value::Object(mut acc) = serde_json::to_value(Settings::default()).expect("defaults")
    else {
        unreachable!()
    };
    let mut problems = Vec::new();
    let fits = |m: &Map<String, Value>| {
        serde_json::from_value::<Settings>(Value::Object(m.clone())).is_ok()
    };

    fn walk(
        root_path: &[String],
        file: &Map<String, Value>,
        root: &mut Map<String, Value>,
        fits: &dyn Fn(&Map<String, Value>) -> bool,
        problems: &mut Vec<Problem>,
    ) {
        for (k, v) in file {
            let mut path = root_path.to_vec();
            path.push(k.clone());
            let known = node(root, &path).cloned();
            match (known, v) {
                // Unknown key: not ours to judge — it stays in the raw file.
                (None, _) => {}
                (Some(Value::Object(_)), Value::Object(inner)) => {
                    walk(&path, inner, root, fits, problems)
                }
                (Some(_), v) => {
                    let mut trial = root.clone();
                    *node_mut(&mut trial, &path) = v.clone();
                    if fits(&trial) {
                        *root = trial;
                    } else {
                        problems.push(Problem::Invalid(path.join(".")));
                    }
                }
            }
        }
    }

    walk(&[], file, &mut acc, &fits, &mut problems);
    let settings = serde_json::from_value(Value::Object(acc)).expect("checked leaf by leaf");
    (settings, problems)
}

fn node<'a>(m: &'a Map<String, Value>, path: &[String]) -> Option<&'a Value> {
    let (first, rest) = path.split_first()?;
    let mut v = m.get(first)?;
    for k in rest {
        v = v.as_object()?.get(k)?;
    }
    Some(v)
}

fn node_mut<'a>(m: &'a mut Map<String, Value>, path: &[String]) -> &'a mut Value {
    let (first, rest) = path.split_first().expect("non-empty path");
    let mut v = m.get_mut(first).expect("known path");
    for k in rest {
        v = v
            .as_object_mut()
            .and_then(|o| o.get_mut(k))
            .expect("known path");
    }
    v
}

/// Our values over the file's: objects merge key by key, everything else is replaced.
fn merge(into: &mut Map<String, Value>, ours: Map<String, Value>) {
    for (k, v) in ours {
        match (into.get_mut(&k), v) {
            (Some(Value::Object(a)), Value::Object(b)) => merge(a, b),
            (_, v) => {
                into.insert(k, v);
            }
        }
    }
}

fn keep_broken_copy(path: &Path) {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".broken-{secs}"));
    let _ = fs::copy(path, path.with_file_name(name));
}

/// Temporary file in the same folder, flushed to disk, then renamed over the old one: a crash or
/// a power cut leaves either the old file or the new one, never half of it.
fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(dir)?;
    let mut tmp = path.file_name().unwrap_or_default().to_os_string();
    tmp.push(format!(".tmp-{}", std::process::id()));
    let tmp = dir.join(tmp);
    let result = (|| {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        // On Windows a reader without FILE_SHARE_DELETE (an antivirus scan, an editor) can hold
        // the target for a moment.
        let mut tries = 0;
        loop {
            match fs::rename(&tmp, path) {
                Err(e) if tries < 10 && e.kind() == io::ErrorKind::PermissionDenied => {
                    tries += 1;
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                r => return r,
            }
        }
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Quality, Theme};

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("znimok-settings-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d.join("settings.json")
    }

    #[test]
    fn missing_file_means_defaults_and_nothing_written() {
        let p = temp("missing");
        let s = Store::open(&p);
        assert_eq!(s.get(), Settings::default());
        assert!(s.problems().is_empty());
        assert!(!p.exists());
        s.update(|x| x.general.theme = Theme::Dark).unwrap();
        assert!(p.exists());
        assert_eq!(Store::open(&p).get().general.theme, Theme::Dark);
    }

    #[test]
    fn bad_values_fall_back_one_by_one() {
        let p = temp("lenient");
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(
            &p,
            r#"{
              "general": { "theme": "purple", "language": "uk" },
              "capture": { "hotkeys": { "region": "Q", "screen": "Ctrl+Alt+9" } },
              "video": { "fps": "fast", "quality": "high", "cursor": false },
              "gif": { "colors": 9999 },
              "editor": 7
            }"#,
        )
        .unwrap();
        let s = Store::open(&p);
        let v = s.get();
        let d = Settings::default();
        assert_eq!(v.general.theme, Theme::Auto);
        assert_eq!(v.general.language.as_deref(), Some("uk"));
        assert_eq!(v.capture.hotkeys.region, d.capture.hotkeys.region);
        assert_eq!(v.capture.hotkeys.screen.unwrap().to_string(), "Ctrl+Alt+9");
        assert_eq!(v.video.fps, 30);
        assert_eq!(v.video.quality, Quality::High);
        assert!(!v.video.cursor);
        assert_eq!(v.gif.colors, 256);
        assert_eq!(v.editor, d.editor);
        let mut pr = s.problems();
        pr.sort_by_key(|p| format!("{p:?}"));
        assert_eq!(
            pr,
            [
                Problem::Adjusted("gif.colors".into()),
                Problem::Invalid("capture.hotkeys.region".into()),
                Problem::Invalid("editor".into()),
                Problem::Invalid("general.theme".into()),
                Problem::Invalid("video.fps".into()),
            ]
        );
    }

    #[test]
    fn keys_of_a_newer_version_survive_a_save() {
        let p = temp("newer");
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(
            &p,
            r#"{ "cloud": { "sync": true }, "video": { "hdr": "pq", "fps": 60 } }"#,
        )
        .unwrap();
        let s = Store::open(&p);
        assert!(s.problems().is_empty(), "{:?}", s.problems());
        assert_eq!(s.get().video.fps, 60);
        s.update(|x| x.video.cursor = false).unwrap();
        let v: Value = serde_json::from_slice(&fs::read(&p).unwrap()).unwrap();
        assert_eq!(v["cloud"]["sync"], true);
        assert_eq!(v["video"]["hdr"], "pq");
        assert_eq!(v["video"]["cursor"], false);
        assert_eq!(v["video"]["fps"], 60);
    }

    #[test]
    fn broken_file_is_kept_aside_before_the_first_save() {
        let p = temp("broken");
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, b"{ \"general\": ").unwrap();
        let s = Store::open(&p);
        assert!(matches!(s.problems()[..], [Problem::Unreadable(_)]));
        assert_eq!(s.get(), Settings::default());
        s.update(|x| x.editor.keep_tool = false).unwrap();
        let copies: Vec<_> = fs::read_dir(p.parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".broken-"))
            .collect();
        assert_eq!(copies.len(), 1);
        assert_eq!(fs::read(copies[0].path()).unwrap(), b"{ \"general\": ");
        assert!(Store::open(&p).problems().is_empty());
    }

    #[test]
    fn two_processes_do_not_lose_each_others_changes() {
        let p = temp("two");
        let app = Store::open(&p);
        let cli = Store::open(&p);
        app.update(|x| x.general.theme = Theme::Light).unwrap();
        cli.update(|x| x.gif.fps = 25).unwrap();
        app.update(|x| x.video.clicks = false).unwrap();
        let v = Store::open(&p).get();
        assert_eq!(v.general.theme, Theme::Light);
        assert_eq!(v.gif.fps, 25);
        assert!(!v.video.clicks);
        assert!(cli.reload());
        assert!(!cli.get().video.clicks);
        assert!(!cli.reload(), "nothing new");
    }

    #[test]
    fn values_out_of_range_are_brought_back_on_update() {
        let p = temp("range");
        let s = Store::open(&p);
        let v = s.update(|x| x.video.fps = 144).unwrap();
        assert_eq!(v.video.fps, 60);
        assert_eq!(Store::open(&p).get().video.fps, 60);
    }

    #[test]
    fn utf8_bom_and_no_temp_files_left() {
        let p = temp("bom");
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, "\u{feff}{\"general\":{\"language\":\"en\"}}").unwrap();
        let s = Store::open(&p);
        assert_eq!(s.get().general.language.as_deref(), Some("en"));
        s.reset().unwrap();
        assert_eq!(s.get(), Settings::default());
        let names: Vec<_> = fs::read_dir(p.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["settings.json"]);
    }
}
