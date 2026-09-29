//! Library documents without the app: find, read, save, render, export. The files are the source
//! of truth, so the running app picks up what agents add or change on its next scan — the same
//! as documents saved by the CLI.

use std::path::{Path, PathBuf};

use znimok_core::{Document, Raster};
use znimok_format::WriteOptions;
use znimok_render::{Renderer, View, pixmap_to_rgba, vello_cpu::Pixmap};

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Item {
    pub id: String,
    pub path: String,
    pub name: String,
    pub created_ms: i64,
    pub width: u32,
    pub height: u32,
    pub marks: u32,
    pub source: String,
    pub tags: Vec<String>,
    pub description: String,
}

pub struct Library {
    pub dir: PathBuf,
}

impl Library {
    /// The user's library: the folder from the settings, else the standard one.
    pub fn from_settings() -> Self {
        let dirs = znimok_settings::Dirs::system();
        let chosen = znimok_settings::Store::open_default().and_then(|s| s.get().library.dir);
        let dir = chosen
            .or_else(|| dirs.map(|d| d.default_library()))
            .unwrap_or_else(|| PathBuf::from("Znimok Library"));
        Self { dir }
    }

    pub fn items(&self) -> Vec<Item> {
        let Ok(rd) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut v: Vec<Item> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "znimok"))
            .filter_map(|p| peek_item(&p))
            .collect();
        v.sort_by_key(|i| std::cmp::Reverse(i.created_ms));
        v
    }

    /// Case-insensitive match in name, description and tags; empty query = everything.
    pub fn search(&self, query: &str, tag: Option<&str>, limit: usize) -> Vec<Item> {
        let q = query.to_lowercase();
        self.items()
            .into_iter()
            .filter(|i| {
                q.is_empty()
                    || i.name.to_lowercase().contains(&q)
                    || i.description.to_lowercase().contains(&q)
                    || i.tags.iter().any(|t| t.to_lowercase().contains(&q))
            })
            .filter(|i| tag.is_none_or(|t| i.tags.iter().any(|x| x.eq_ignore_ascii_case(t))))
            .take(limit.max(1))
            .collect()
    }

    /// A document by its id (from the library) or by a path to a `.znimok` file **inside the
    /// library** — an agent's scopes are about the library, not any file of the person (ZK-113).
    pub fn resolve(&self, doc: &str) -> Option<PathBuf> {
        let p = Path::new(doc);
        if p.extension().is_some_and(|e| e == "znimok") && p.is_file() {
            let inside = match (p.canonicalize(), self.dir.canonicalize()) {
                (Ok(f), Ok(d)) => f.starts_with(d),
                _ => false,
            };
            return inside.then(|| p.to_path_buf());
        }
        self.items()
            .into_iter()
            .find(|i| i.id.eq_ignore_ascii_case(doc) || i.id.starts_with(&doc.to_lowercase()))
            .map(|i| PathBuf::from(i.path))
    }

    /// Where a new document goes: sortable time plus a short part of its id (as the app names them).
    pub fn new_path(&self, doc: &Document) -> PathBuf {
        let t = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let id = doc.id.to_string();
        self.dir.join(format!("Znimok-{t}-{}.znimok", &id[..8]))
    }
}

fn peek_item(p: &Path) -> Option<Item> {
    let bytes = std::fs::read(p).ok()?;
    let k = znimok_format::peek(&bytes).ok()?;
    Some(Item {
        id: k.id.map(|u| u.to_string()).unwrap_or_default(),
        path: p.display().to_string(),
        name: k.name,
        created_ms: k.meta.created_ms,
        width: k.width,
        height: k.height,
        marks: k.object_count,
        source: k.meta.source,
        tags: k.meta.tags,
        description: k.meta.description,
    })
}

/// The document with its marks (for a video, its poster document — see [`save`]).
pub fn load(path: &Path) -> Result<Document, String> {
    znimok_format::open_parts(path)
        .map(|(doc, _)| doc)
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// The document with its marks, 1:1 (or scaled).
pub fn render(doc: &Document, scale: f64) -> Raster {
    let mut view = View::one_to_one(doc);
    if (scale - 1.0).abs() > 1e-9 {
        let f = doc.frame();
        view.scale = scale;
        view.width = ((f.w as f64 * scale).round() as i64).clamp(1, 65535) as u16;
        view.height = ((f.h as f64 * scale).round() as i64).clamp(1, 65535) as u16;
    }
    let mut pix = Pixmap::new(1, 1);
    Renderer::new().render(doc, view, &mut pix);
    Raster::new(
        pix.width() as u32,
        pix.height() as u32,
        pixmap_to_rgba(&pix),
    )
}

pub fn save(path: &Path, doc: &Document) -> Result<(), String> {
    let f = doc.frame();
    let s = (320.0 / f.w as f64).min(240.0 / f.h as f64).min(1.0);
    let opts = WriteOptions {
        app_version: format!("znimok agents {}", env!("CARGO_PKG_VERSION")),
        thumbnail: Some(render(doc, s)),
        ..Default::default()
    };
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    // A video in the library stays a video when an agent changes its marks (ZK-145): its video
    // blocks and stream are taken from the file being replaced.
    let video = if path.exists() {
        znimok_format::open_parts(path).ok().and_then(|(_, v)| v)
    } else {
        None
    };
    znimok_format::save_same_kind(path, doc, video.as_ref(), &opts)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportFormat {
    Png,
    Jpeg,
    Webp,
    Html,
}

impl ExportFormat {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.to_ascii_lowercase().as_str() {
            "png" => Self::Png,
            "jpg" | "jpeg" => Self::Jpeg,
            "webp" => Self::Webp,
            "html" | "htm" => Self::Html,
            _ => return None,
        })
    }
}

pub fn encode_png(r: &Raster) -> Result<Vec<u8>, String> {
    let mut out = std::io::Cursor::new(Vec::new());
    image::write_buffer_with_format(
        &mut out,
        &r.rgba,
        r.width,
        r.height,
        image::ExtendedColorType::Rgba8,
        image::ImageFormat::Png,
    )
    .map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}

/// Writes the document as a picture (or the self-contained page) to `out`.
pub fn export(doc: &Document, fmt: ExportFormat, out: &Path) -> Result<(), String> {
    let r = render(doc, 1.0);
    let bytes = match fmt {
        ExportFormat::Png => encode_png(&r)?,
        ExportFormat::Jpeg | ExportFormat::Webp => {
            let rgb = image::DynamicImage::ImageRgba8(
                image::RgbaImage::from_raw(r.width, r.height, r.rgba.clone())
                    .ok_or("pixel data does not match the size")?,
            );
            let mut buf = std::io::Cursor::new(Vec::new());
            if fmt == ExportFormat::Jpeg {
                image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 92)
                    .encode_image(&rgb.to_rgb8())
                    .map_err(|e| e.to_string())?;
            } else {
                rgb.write_to(&mut buf, image::ImageFormat::WebP)
                    .map_err(|e| e.to_string())?;
            }
            buf.into_inner()
        }
        ExportFormat::Html => {
            let png = encode_png(&r)?;
            let tr = znimok_i18n::Localizer::for_system(None);
            znimok_html::page(doc, &png, r.width, r.height, &tr).into_bytes()
        }
    };
    std::fs::write(out, bytes).map_err(|e| format!("{}: {e}", out.display()))
}

#[cfg(test)]
pub(crate) fn test_doc(w: u32, h: u32) -> Document {
    let mut d = Document::from_raster("Тест", Raster::solid(w, h, znimok_core::Rgb::WHITE));
    d.meta.tags = vec!["demo".into()];
    d.meta.description = "Вікно налаштувань".into();
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_find_search_resolve_export() {
        let dir = std::env::temp_dir().join(format!("znimok-agents-lib-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let lib = Library { dir: dir.clone() };
        let d = test_doc(64, 40);
        let p = lib.new_path(&d);
        save(&p, &d).unwrap();
        let items = lib.items();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, d.id.to_string());
        assert_eq!(lib.search("налаштувань", None, 10).len(), 1);
        assert_eq!(lib.search("", Some("DEMO"), 10).len(), 1);
        assert!(lib.search("нема", None, 10).is_empty());
        assert_eq!(lib.resolve(&d.id.to_string()[..8]), Some(p.clone()));
        assert_eq!(lib.resolve(&p.display().to_string()), Some(p.clone()));
        // A .znimok outside the library is not reachable by path (ZK-113).
        let outside =
            std::env::temp_dir().join(format!("zk-outside-{}.znimok", std::process::id()));
        save(&outside, &d).unwrap();
        assert_eq!(lib.resolve(&outside.display().to_string()), None);
        let _ = std::fs::remove_file(&outside);
        for (f, ext) in [
            (ExportFormat::Png, "png"),
            (ExportFormat::Jpeg, "jpg"),
            (ExportFormat::Webp, "webp"),
            (ExportFormat::Html, "html"),
        ] {
            let out = dir.join(format!("out.{ext}"));
            export(&load(&p).unwrap(), f, &out).unwrap();
            assert!(std::fs::metadata(&out).unwrap().len() > 0);
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
