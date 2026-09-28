//! The library: a folder of `.znimok` documents, newest first. Cards are read from the head of
//! each file only (`znimok_format::peek` stops before the pixels), so a big library opens fast.

use std::io::Read;
use std::path::{Path, PathBuf};

use chrono::{Local, TimeZone};

/// Same default as `znimok library list` (docs/CLI.md).
pub fn default_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("ZNIMOK_LIBRARY") {
        return d.into();
    }
    if cfg!(target_os = "macos") {
        let home = std::env::var_os("HOME").unwrap_or_default();
        return PathBuf::from(home).join("Library/Application Support/Znimok/Library");
    }
    let base = std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("HOME"))
        .unwrap_or_default();
    PathBuf::from(base).join("Znimok").join("Library")
}

pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub created_ms: i64,
    pub width: u32,
    pub height: u32,
    pub tags: Vec<String>,
    pub description: String,
    pub thumb_png: Option<Vec<u8>>,
}

impl Entry {
    pub fn meta_line(&self) -> String {
        let when = Local
            .timestamp_millis_opt(self.created_ms)
            .single()
            .map(|t| t.format("%d.%m.%Y %H:%M").to_string())
            .unwrap_or_default();
        format!("{} × {} · {when}", self.width, self.height)
    }

    pub fn matches(&self, filter: &str) -> bool {
        if filter.is_empty() {
            return true;
        }
        let f = filter.to_lowercase();
        self.name.to_lowercase().contains(&f)
            || self.description.to_lowercase().contains(&f)
            || self.tags.iter().any(|t| t.to_lowercase().contains(&f))
    }
}

/// Head of a file: enough for the metadata and the thumbnail, never the source pixels.
const HEAD_BYTES: u64 = 2 * 1024 * 1024;

pub fn read_entry(path: &Path) -> Option<Entry> {
    let mut head = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(HEAD_BYTES)
        .read_to_end(&mut head)
        .ok()?;
    if !znimok_format::is_znimok(&head) {
        return None;
    }
    let p = znimok_format::peek(&head).ok()?;
    let created_ms = if p.meta.created_ms > 0 {
        p.meta.created_ms
    } else {
        std::fs::metadata(path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    };
    Some(Entry {
        path: path.to_path_buf(),
        name: p.name,
        created_ms,
        width: p.width,
        height: p.height,
        tags: p.meta.tags,
        description: p.meta.description,
        thumb_png: p.thumbnail_png,
    })
}

/// The library's own trash (ZK-55): a hidden folder inside it, so a deleted screenshot can be
/// brought back with one click and follows the library to another disk; emptied after 30 days.
pub fn trash_dir(lib: &Path) -> PathBuf {
    lib.join(".trash")
}

pub const TRASH_DAYS: u64 = 30;

/// Moves a document into the trash; returns where it went.
pub fn move_to_trash(lib: &Path, file: &Path) -> std::io::Result<PathBuf> {
    let dir = trash_dir(lib);
    std::fs::create_dir_all(&dir)?;
    #[cfg(windows)]
    hide(&dir);
    let name = file.file_name().unwrap_or_default();
    let mut to = dir.join(name);
    let mut n = 1;
    while to.exists() {
        let stem = file.file_stem().unwrap_or_default().to_string_lossy();
        to = dir.join(format!("{stem} ({n}).znimok"));
        n += 1;
    }
    std::fs::rename(file, &to)?;
    // The time it went in, for the 30 days.
    let _ = std::fs::File::options()
        .write(true)
        .open(&to)
        .and_then(|f| f.set_modified(std::time::SystemTime::now()));
    Ok(to)
}

/// Brings a trashed document back to where it was (or next to it, if that name is taken).
pub fn restore(trashed: &Path, original: &Path) -> std::io::Result<()> {
    let mut to = original.to_path_buf();
    let mut n = 1;
    while to.exists() {
        let stem = original.file_stem().unwrap_or_default().to_string_lossy();
        to = original.with_file_name(format!("{stem} ({n}).znimok"));
        n += 1;
    }
    std::fs::rename(trashed, to)
}

/// Deletes for good what has been in the trash longer than [`TRASH_DAYS`].
pub fn purge_trash(lib: &Path) {
    let Ok(rd) = std::fs::read_dir(trash_dir(lib)) else {
        return;
    };
    let limit = std::time::Duration::from_secs(TRASH_DAYS * 24 * 3600);
    for e in rd.flatten() {
        let old = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > limit);
        if old {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

#[cfg(windows)]
fn hide(dir: &Path) {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_HIDDEN, SetFileAttributesW};
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: a NUL-terminated path.
    let _ =
        unsafe { SetFileAttributesW(windows::core::PCWSTR(wide.as_ptr()), FILE_ATTRIBUTE_HIDDEN) };
}

/// Opens the system file manager at a folder, or with a file selected in it.
pub fn show_in_folder(p: &Path) {
    #[cfg(windows)]
    {
        let arg = if p.is_dir() {
            p.as_os_str().to_owned()
        } else {
            let mut a = std::ffi::OsString::from("/select,");
            a.push(p.as_os_str());
            a
        };
        let _ = std::process::Command::new("explorer.exe").arg(arg).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let mut c = std::process::Command::new("open");
        if !p.is_dir() {
            c.arg("-R");
        }
        let _ = c.arg(p).spawn();
    }
}

/// Documents in `dir`, newest first. Recognised by content; `.part` leftovers are skipped.
pub fn scan(dir: &Path) -> Vec<Entry> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut v: Vec<Entry> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_none_or(|e| e != "part"))
        .filter_map(|p| read_entry(&p))
        .collect();
    v.sort_by_key(|e| std::cmp::Reverse(e.created_ms));
    v
}

/// File name for a new document: sortable time plus a short part of its id (unique enough
/// for two shots in the same second).
pub fn new_path(dir: &Path, id: &str) -> PathBuf {
    let t = Local::now().format("%Y%m%d-%H%M%S");
    dir.join(format!("Znimok-{t}-{}.znimok", &id[..id.len().min(8)]))
}

pub fn thumb_image(png: &[u8]) -> Option<slint::Image> {
    let img = image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .ok()?
        .to_rgba8();
    let (w, h) = img.dimensions();
    let buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(img.as_raw(), w, h);
    Some(slint::Image::from_rgba8(buf))
}
