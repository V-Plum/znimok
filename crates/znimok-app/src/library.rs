//! The library: a folder of `.znimok` documents, newest first. Cards are read from the head of
//! each file only (`znimok_format::peek` stops before the pixels), so a big library opens fast.

use std::io::Read;
use std::path::{Path, PathBuf};

use chrono::{Local, TimeZone};
use redb::{ReadableDatabase, ReadableTable};

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
/// With an index, a file whose time and size did not change is not read at all (ZK-131).
pub fn scan(dir: &Path, index: Option<&Index>) -> Vec<Entry> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let files: Vec<(PathBuf, Stamp)> = rd
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            let m = e.metadata().ok()?;
            (m.is_file() && p.extension().is_none_or(|x| x != "part")).then(|| (p, Stamp::of(&m)))
        })
        .collect();
    let mut v: Vec<Entry> = files
        .iter()
        .filter_map(|(p, st)| {
            if let Some(e) = index.and_then(|i| i.get(p, *st)) {
                return Some(e);
            }
            HEAD_READS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let e = read_entry(p);
            if let (Some(i), Some(e)) = (index, e.as_ref()) {
                i.put(e, *st);
            }
            e
        })
        .collect();
    if let Some(i) = index {
        i.prune(files.iter().filter_map(|(p, _)| key(p)));
    }
    v.sort_by_key(|e| std::cmp::Reverse(e.created_ms));
    v
}

/// How many file heads were read (for the self-test: an indexed rescan reads none).
pub static HEAD_READS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// A file's modification time and size: when both are the same, so is the card.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Stamp {
    mtime_ms: i64,
    size: u64,
}

impl Stamp {
    fn of(m: &std::fs::Metadata) -> Self {
        let mtime_ms = m
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        Stamp {
            mtime_ms,
            size: m.len(),
        }
    }
}

fn key(p: &Path) -> Option<String> {
    p.file_name().map(|n| n.to_string_lossy().into_owned())
}

/// What changed in the folder since last time, cheaply: names, times and sizes of the files
/// (the watcher polls it — cloud and network folders do not report changes reliably).
pub fn fingerprint(dir: &Path) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut items: Vec<(String, i64, u64)> = rd
        .flatten()
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            let st = Stamp::of(&m);
            m.is_file().then(|| {
                (
                    e.file_name().to_string_lossy().into_owned(),
                    st.mtime_ms,
                    st.size,
                )
            })
        })
        .collect();
    items.sort();
    items.hash(&mut h);
    h.finish()
}

/// The library index (ZK-131): cards by file name, with the time and size they were read at.
/// It lives in the local cache, not in the library — a library on a cloud drive would sync and
/// lock a database file. Losing it costs one full read; a broken one is rebuilt.
pub struct Index {
    db: redb::Database,
}

const CARDS: redb::TableDefinition<&str, &[u8]> = redb::TableDefinition::new("cards");

impl Index {
    /// The index of the library at `lib`, in the cache folder (one file per library).
    pub fn open(lib: &Path) -> Option<Index> {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        lib.hash(&mut h);
        let dir = cache_dir();
        std::fs::create_dir_all(&dir).ok()?;
        let path = dir.join(format!("library-{:016x}.redb", h.finish()));
        let db = match redb::Database::create(&path) {
            Ok(db) => db,
            Err(_) => {
                // Broken or from an incompatible version: start again.
                let _ = std::fs::remove_file(&path);
                redb::Database::create(&path).ok()?
            }
        };
        Some(Index { db })
    }

    fn get(&self, p: &Path, st: Stamp) -> Option<Entry> {
        let k = key(p)?;
        let tx = self.db.begin_read().ok()?;
        let t = tx.open_table(CARDS).ok()?;
        let v = t.get(k.as_str()).ok()??;
        decode(v.value(), p, st)
    }

    fn put(&self, e: &Entry, st: Stamp) {
        let Some(k) = key(&e.path) else { return };
        let bytes = encode(e, st);
        let Ok(tx) = self.db.begin_write() else {
            return;
        };
        {
            let Ok(mut t) = tx.open_table(CARDS) else {
                return;
            };
            let _ = t.insert(k.as_str(), bytes.as_slice());
        }
        let _ = tx.commit();
    }

    /// Forgets files that are gone.
    fn prune(&self, present: impl Iterator<Item = String>) {
        let present: std::collections::HashSet<String> = present.collect();
        let Ok(tx) = self.db.begin_write() else {
            return;
        };
        {
            let Ok(mut t) = tx.open_table(CARDS) else {
                return;
            };
            let gone: Vec<String> = match t.iter() {
                Ok(it) => it
                    .flatten()
                    .map(|(k, _)| k.value().to_string())
                    .filter(|k| !present.contains(k))
                    .collect(),
                Err(_) => Vec::new(),
            };
            for k in gone {
                let _ = t.remove(k.as_str());
            }
        }
        let _ = tx.commit();
    }
}

fn cache_dir() -> PathBuf {
    if cfg!(target_os = "macos") {
        let home = std::env::var_os("HOME").unwrap_or_default();
        return PathBuf::from(home).join("Library/Caches/Znimok");
    }
    let base = std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("HOME"))
        .unwrap_or_default();
    PathBuf::from(base).join("Znimok").join("Cache")
}

// A card in the index: stamp, then length-prefixed fields (little endian). Version byte first,
// so a future layout is simply read again from the file.
const INDEX_VERSION: u8 = 1;

fn encode(e: &Entry, st: Stamp) -> Vec<u8> {
    let mut v = vec![INDEX_VERSION];
    let bytes = |v: &mut Vec<u8>, b: &[u8]| {
        v.extend_from_slice(&(b.len() as u32).to_le_bytes());
        v.extend_from_slice(b);
    };
    v.extend_from_slice(&st.mtime_ms.to_le_bytes());
    v.extend_from_slice(&st.size.to_le_bytes());
    v.extend_from_slice(&e.created_ms.to_le_bytes());
    v.extend_from_slice(&e.width.to_le_bytes());
    v.extend_from_slice(&e.height.to_le_bytes());
    bytes(&mut v, e.name.as_bytes());
    bytes(&mut v, e.description.as_bytes());
    bytes(&mut v, e.tags.join("\n").as_bytes());
    bytes(&mut v, e.thumb_png.as_deref().unwrap_or(&[]));
    v
}

fn decode(b: &[u8], p: &Path, st: Stamp) -> Option<Entry> {
    let mut at = 0usize;
    let mut take = |n: usize| -> Option<&[u8]> {
        let s = b.get(at..at + n)?;
        at += n;
        Some(s)
    };
    if take(1)?[0] != INDEX_VERSION {
        return None;
    }
    let i64_ = |s: &[u8]| i64::from_le_bytes(s.try_into().unwrap_or_default());
    let mtime_ms = i64_(take(8)?);
    let size = u64::from_le_bytes(take(8)?.try_into().ok()?);
    if (Stamp { mtime_ms, size }) != st {
        return None;
    }
    let created_ms = i64_(take(8)?);
    let width = u32::from_le_bytes(take(4)?.try_into().ok()?);
    let height = u32::from_le_bytes(take(4)?.try_into().ok()?);
    let mut field = || -> Option<Vec<u8>> {
        let n = u32::from_le_bytes(take(4)?.try_into().ok()?) as usize;
        Some(take(n)?.to_vec())
    };
    let name = String::from_utf8(field()?).ok()?;
    let description = String::from_utf8(field()?).ok()?;
    let tags = String::from_utf8(field()?).ok()?;
    let thumb = field()?;
    Some(Entry {
        path: p.to_path_buf(),
        name,
        created_ms,
        width,
        height,
        tags: if tags.is_empty() {
            Vec::new()
        } else {
            tags.split('\n').map(str::to_string).collect()
        },
        description,
        thumb_png: (!thumb.is_empty()).then_some(thumb),
    })
}

/// File name for a new document: sortable time plus a short part of its id (unique enough
/// for two shots in the same second).
pub fn new_path(dir: &Path, id: &str) -> PathBuf {
    let t = Local::now().format("%Y%m%d-%H%M%S");
    dir.join(format!("Znimok-{t}-{}.znimok", &id[..id.len().min(8)]))
}

/// A card's picture from the thumbnail stored in the file, decoded within the same limits as
/// the system thumbnail handlers: a small file with a "PNG bomb" inside is refused (ZK-121).
pub fn thumb_image(png: &[u8]) -> Option<slint::Image> {
    let r = znimok_format::decode_png(png, &znimok_format::Limits::thumbnail()).ok()?;
    if r.rgba.len() != r.width as usize * r.height as usize * 4 {
        return None;
    }
    let buf =
        slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(&r.rgba, r.width, r.height);
    Some(slint::Image::from_rgba8(buf))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut out = Vec::new();
        image::RgbaImage::new(w, h)
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    #[test]
    fn index_card_round_trip() {
        let e = Entry {
            path: PathBuf::from("x.znimok"),
            name: "Знімок".into(),
            created_ms: 1_700_000_000_000,
            width: 1600,
            height: 1000,
            tags: vec!["тест".into(), "znimok".into()],
            description: "опис".into(),
            thumb_png: Some(png(4, 3)),
        };
        let st = Stamp {
            mtime_ms: 5,
            size: 99,
        };
        let d = decode(&encode(&e, st), &e.path, st).expect("decodes");
        assert_eq!(
            (
                d.name,
                d.created_ms,
                d.width,
                d.height,
                d.tags,
                d.description,
                d.thumb_png
            ),
            (
                e.name,
                e.created_ms,
                e.width,
                e.height,
                e.tags,
                e.description,
                e.thumb_png
            )
        );
        // A changed file is read again.
        assert!(
            decode(
                &encode(&d_entry(), st),
                Path::new("x"),
                Stamp {
                    mtime_ms: 6,
                    size: 99
                }
            )
            .is_none()
        );
    }

    fn d_entry() -> Entry {
        Entry {
            path: PathBuf::from("y"),
            name: String::new(),
            created_ms: 0,
            width: 1,
            height: 1,
            tags: Vec::new(),
            description: String::new(),
            thumb_png: None,
        }
    }

    #[test]
    fn thumbnails_within_limits_only() {
        assert!(thumb_image(&png(320, 240)).is_some());
        // Compresses to a few kilobytes, would unpack to 256 MB.
        assert!(thumb_image(&png(8192, 8192)).is_none());
        assert!(thumb_image(b"not a png").is_none());
    }
}
