//! What an exported picture says about itself (ZK-61): the title, description, author, rights,
//! tags and the time the shot was taken — never window or program names or file paths (privacy
//! note, "Metadata in files you share"). PNG gets iTXt chunks (UTF-8), JPEG and WebP an EXIF
//! block with the classic tags plus the Windows XP* ones (UTF-16, what Explorer shows), and the
//! file's own time is set to the time of the shot, so a folder sorts by when things happened.

use std::path::Path;
use std::time::{Duration, SystemTime};

use chrono::{DateTime, Local, TimeZone};

thread_local! {
    /// "Write metadata" in the Copy menu (on by default; the privacy note promises the switch).
    static ENABLED: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

pub fn enabled() -> bool {
    ENABLED.with(|e| e.get())
}

pub fn set_enabled(on: bool) {
    ENABLED.with(|e| e.set(on));
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FileMeta {
    pub title: String,
    pub description: String,
    pub author: String,
    pub copyright: String,
    pub tags: Vec<String>,
    /// When the shot was taken (Unix ms); 0 = unknown.
    pub created_ms: i64,
    pub software: String,
}

impl FileMeta {
    pub fn from_doc(doc: &znimok_core::Document) -> Self {
        Self {
            title: doc.name.clone(),
            description: doc.meta.description.clone(),
            author: doc.meta.author.clone(),
            copyright: doc.meta.copyright.clone(),
            tags: doc.meta.tags.clone(),
            created_ms: doc.meta.created_ms,
            software: format!("Znimok {}", env!("CARGO_PKG_VERSION")),
        }
    }

    /// A fresh shot that is not a document yet (the card after a capture).
    pub fn new_shot(title: &str) -> Self {
        Self {
            title: title.to_string(),
            created_ms: Local::now().timestamp_millis(),
            software: format!("Znimok {}", env!("CARGO_PKG_VERSION")),
            ..Default::default()
        }
    }

    fn created(&self) -> Option<DateTime<Local>> {
        (self.created_ms > 0)
            .then(|| Local.timestamp_millis_opt(self.created_ms).single())
            .flatten()
    }

    /// PNG text chunks: the standard keywords of the PNG specification.
    pub fn png_text(&self) -> Vec<(&'static str, String)> {
        let mut v = Vec::new();
        let mut put = |k: &'static str, s: &str| {
            if !s.trim().is_empty() {
                v.push((k, s.to_string()));
            }
        };
        put("Title", &self.title);
        put("Description", &self.description);
        put("Author", &self.author);
        put("Copyright", &self.copyright);
        put("Keywords", &self.tags.join(", "));
        if let Some(t) = self.created() {
            put("Creation Time", &t.to_rfc3339());
        }
        put("Software", &self.software);
        v
    }

    /// A little-endian TIFF block for EXIF (the encoder adds the "Exif\0\0" header).
    pub fn exif(&self) -> Vec<u8> {
        let ascii = |s: &str| {
            let mut b = s.as_bytes().to_vec();
            b.push(0);
            b
        };
        let utf16 = |s: &str| {
            let mut b: Vec<u8> = s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
            b.extend_from_slice(&[0, 0]);
            b
        };
        const ASCII: u16 = 2;
        const BYTE: u16 = 1;
        let mut ifd0: Vec<(u16, u16, Vec<u8>)> = Vec::new();
        let mut exif: Vec<(u16, u16, Vec<u8>)> = Vec::new();
        let when = self.created();
        if !self.description.trim().is_empty() || !self.title.trim().is_empty() {
            let d = if self.description.trim().is_empty() {
                &self.title
            } else {
                &self.description
            };
            ifd0.push((0x010E, ASCII, ascii(d)));
        }
        if !self.software.is_empty() {
            ifd0.push((0x0131, ASCII, ascii(&self.software)));
        }
        if let Some(t) = when {
            ifd0.push((
                0x0132,
                ASCII,
                ascii(&t.format("%Y:%m:%d %H:%M:%S").to_string()),
            ));
        }
        if !self.author.trim().is_empty() {
            ifd0.push((0x013B, ASCII, ascii(&self.author)));
        }
        if !self.copyright.trim().is_empty() {
            ifd0.push((0x8298, ASCII, ascii(&self.copyright)));
        }
        if let Some(t) = when {
            let stamp = ascii(&t.format("%Y:%m:%d %H:%M:%S").to_string());
            exif.push((0x9003, ASCII, stamp.clone()));
            exif.push((0x9004, ASCII, stamp));
            exif.push((0x9010, ASCII, ascii(&t.format("%:z").to_string())));
        }
        if !self.title.trim().is_empty() {
            ifd0.push((0x9C9B, BYTE, utf16(&self.title)));
        }
        if !self.description.trim().is_empty() {
            ifd0.push((0x9C9C, BYTE, utf16(&self.description)));
        }
        if !self.author.trim().is_empty() {
            ifd0.push((0x9C9D, BYTE, utf16(&self.author)));
        }
        if !self.tags.is_empty() {
            ifd0.push((0x9C9E, BYTE, utf16(&self.tags.join(";"))));
        }
        // The pointer to the Exif sub-IFD sits between 0x8298 and 0x9C9B (tags ascending).
        let has_exif = !exif.is_empty();
        if has_exif {
            ifd0.push((0x8769, 4, vec![0; 4]));
        }
        ifd0.sort_by_key(|e| e.0);

        let ifd_len = |n: usize| 2 + 12 * n + 4;
        let data_len = |v: &[(u16, u16, Vec<u8>)]| {
            v.iter()
                .map(|e| {
                    if e.2.len() > 4 {
                        e.2.len() + e.2.len() % 2
                    } else {
                        0
                    }
                })
                .sum::<usize>()
        };
        let ifd0_at = 8usize;
        let ifd0_data = ifd0_at + ifd_len(ifd0.len());
        let exif_at = ifd0_data + data_len(&ifd0);
        if has_exif {
            let at = (exif_at as u32).to_le_bytes().to_vec();
            if let Some(e) = ifd0.iter_mut().find(|e| e.0 == 0x8769) {
                e.2 = at;
            }
        }
        let mut out = b"II*\0".to_vec();
        out.extend_from_slice(&(ifd0_at as u32).to_le_bytes());
        write_ifd(&mut out, &ifd0, ifd0_data);
        if has_exif {
            debug_assert_eq!(out.len(), exif_at);
            let exif_data = exif_at + ifd_len(exif.len());
            write_ifd(&mut out, &exif, exif_data);
        }
        out
    }
}

/// One IFD at the end of `out` with its out-of-line values right after it, starting at `data`.
fn write_ifd(out: &mut Vec<u8>, entries: &[(u16, u16, Vec<u8>)], data: usize) {
    let mut tail: Vec<u8> = Vec::new();
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for (tag, ty, bytes) in entries {
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&ty.to_le_bytes());
        // Count in units of the type: ASCII and BYTE are 1 byte, LONG (4) is one value.
        let count = if *ty == 4 { 1 } else { bytes.len() as u32 };
        out.extend_from_slice(&count.to_le_bytes());
        if bytes.len() <= 4 {
            let mut v = bytes.clone();
            v.resize(4, 0);
            out.extend_from_slice(&v);
        } else {
            out.extend_from_slice(&((data + tail.len()) as u32).to_le_bytes());
            tail.extend_from_slice(bytes);
            if bytes.len() % 2 == 1 {
                tail.push(0);
            }
        }
    }
    out.extend_from_slice(&0u32.to_le_bytes()); // no next IFD
    out.extend_from_slice(&tail);
}

/// The file's modification (and, where the system has one, creation) time = the shot's time.
pub fn set_file_time(path: &Path, created_ms: i64) {
    if created_ms <= 0 {
        return;
    }
    let t = SystemTime::UNIX_EPOCH + Duration::from_millis(created_ms as u64);
    let Ok(f) = std::fs::File::options().write(true).open(path) else {
        return;
    };
    let times = std::fs::FileTimes::new().set_modified(t).set_accessed(t);
    #[cfg(windows)]
    let times = {
        use std::os::windows::fs::FileTimesExt;
        times.set_created(t)
    };
    #[cfg(target_os = "macos")]
    let times = {
        use std::os::macos::fs::FileTimesExt;
        times.set_created(t)
    };
    let _ = f.set_times(times);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta() -> FileMeta {
        FileMeta {
            title: "Налаштування друку".into(),
            description: "крок 3".into(),
            author: "Plum".into(),
            copyright: "".into(),
            tags: vec!["друк".into(), "крок".into()],
            created_ms: 1_790_558_000_123,
            software: "Znimok 0.0.0".into(),
        }
    }

    fn u16_at(b: &[u8], i: usize) -> u16 {
        u16::from_le_bytes([b[i], b[i + 1]])
    }
    fn u32_at(b: &[u8], i: usize) -> u32 {
        u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
    }

    #[test]
    fn exif_is_a_valid_tiff_with_sorted_tags() {
        let b = meta().exif();
        assert_eq!(&b[..4], b"II*\0");
        let ifd = u32_at(&b, 4) as usize;
        let n = u16_at(&b, ifd) as usize;
        let tags: Vec<u16> = (0..n).map(|k| u16_at(&b, ifd + 2 + 12 * k)).collect();
        let mut sorted = tags.clone();
        sorted.sort();
        assert_eq!(tags, sorted);
        assert!(tags.contains(&0x9C9B) && tags.contains(&0x8769) && !tags.contains(&0x8298));
        // XPTitle points at the UTF-16 title.
        let k = tags.iter().position(|t| *t == 0x9C9B).unwrap();
        let e = ifd + 2 + 12 * k;
        let (count, off) = (u32_at(&b, e + 4) as usize, u32_at(&b, e + 8) as usize);
        let words: Vec<u16> = (0..count / 2 - 1)
            .map(|i| u16_at(&b, off + 2 * i))
            .collect();
        assert_eq!(String::from_utf16(&words).unwrap(), "Налаштування друку");
        // The Exif sub-IFD has DateTimeOriginal.
        let k = tags.iter().position(|t| *t == 0x8769).unwrap();
        let sub = u32_at(&b, ifd + 2 + 12 * k + 8) as usize;
        assert_eq!(u16_at(&b, sub + 2), 0x9003);
    }

    #[test]
    fn png_text_skips_empty_fields() {
        let t = meta().png_text();
        let keys: Vec<&str> = t.iter().map(|e| e.0).collect();
        assert_eq!(
            keys,
            [
                "Title",
                "Description",
                "Author",
                "Keywords",
                "Creation Time",
                "Software"
            ]
        );
    }
}
