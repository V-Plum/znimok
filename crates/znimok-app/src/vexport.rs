//! The export sheet of a video (ZK-190): its state, the GIF's fitting under a size limit, and an
//! exported MP4 wrapped into a library document. The work itself is `znimok-export` (ZK-95),
//! on a worker thread with a progress counter the sheet shows.

use std::path::{Path, PathBuf};

/// Formats of the sheet, in the order of its cards.
pub const MP4: usize = 0;
pub const GIF: usize = 1;
pub const HTML: usize = 2;
pub const FRAME: usize = 3;
pub const REPORT: usize = 4;

pub const GIF_WIDTHS: [u32; 4] = [480, 640, 800, 1280];
pub const GIF_FPS: [u32; 4] = [5, 10, 15, 20];
/// Steps of the GIF size limit, MB (0 = none).
pub const LIMITS: [u32; 7] = [0, 5, 10, 15, 25, 50, 100];

/// Where an export goes.
pub const TO_CLIPBOARD: usize = 0;
pub const TO_FILE: usize = 1;
pub const TO_LIBRARY: usize = 2;

/// The sheet's choices (kept between openings in this session).
#[derive(Clone, Debug, PartialEq)]
pub struct VidExport {
    pub kind: usize,
    pub gif_w: u32,
    pub gif_fps: u32,
    pub dither: bool,
    /// Index into [`LIMITS`].
    pub limit: usize,
    pub to: usize,
    /// The report as a `.zreport` (else one HTML page; ZK-98).
    pub zreport: bool,
    /// Hide the log's sensitive values (when Settings leave it to the sheet).
    pub hide: bool,
}

impl Default for VidExport {
    fn default() -> Self {
        Self {
            kind: MP4,
            gif_w: 640,
            gif_fps: 10,
            dither: true,
            limit: 0,
            to: TO_FILE,
            zreport: false,
            hide: true,
        }
    }
}

impl VidExport {
    pub fn ext(&self) -> &'static str {
        if self.kind == REPORT && self.zreport {
            "zreport"
        } else {
            ext(self.kind)
        }
    }

    /// The format's name for the dialog's filter and the toasts.
    pub fn name(&self) -> &'static str {
        if self.kind == REPORT && self.zreport {
            ".zreport"
        } else {
            format_name(self.kind)
        }
    }

    pub fn limit_bytes(&self) -> Option<u64> {
        LIMITS
            .get(self.limit)
            .copied()
            .filter(|m| *m > 0)
            .map(|m| u64::from(m) * 1024 * 1024)
    }

    /// The engine's kind for this choice (None: the frame and the report are not files of it).
    pub fn engine_kind(&self, sound: bool) -> Option<znimok_export::Kind> {
        match self.kind {
            MP4 => Some(znimok_export::Kind::Mp4 { sound }),
            GIF => Some(znimok_export::Kind::Gif {
                width: self.gif_w,
                fps: f64::from(self.gif_fps),
                dither: self.dither,
            }),
            HTML => Some(znimok_export::Kind::Html),
            _ => None,
        }
    }
}

pub fn ext(kind: usize) -> &'static str {
    match kind {
        GIF => "gif",
        HTML | REPORT => "html",
        _ => "mp4",
    }
}

pub fn format_name(kind: usize) -> &'static str {
    match kind {
        GIF => "GIF",
        HTML => "HTML",
        _ => "MP4",
    }
}

/// A GIF over `limit`: lower the frame rate first, then the width, until the estimate fits (or
/// nothing is left to lower). Returns the width and rate to use and whether they changed.
pub fn fit_gif(
    job: &znimok_export::Job,
    gpu: &znimok_play::Gpu,
    width: u32,
    fps: u32,
    dither: bool,
    limit: u64,
) -> Result<(u32, u32, bool), String> {
    let (mut w, mut f) = (width, fps);
    loop {
        let est = znimok_export::estimate_gif(job, gpu, w, f64::from(f), dither)?;
        if est <= limit {
            return Ok((w, f, (w, f) != (width, fps)));
        }
        if let Some(lower) = GIF_FPS.iter().rev().find(|x| **x < f) {
            f = *lower;
        } else if let Some(lower) = GIF_WIDTHS.iter().rev().find(|x| **x < w) {
            w = *lower;
            f = fps;
        } else {
            return Ok((w, f, (w, f) != (width, fps)));
        }
    }
}

/// What a finished MP4 is: its poster (the first frame), size, length and whether it has sound.
struct Probe {
    poster: znimok_core::Raster,
    width: u32,
    height: u32,
    duration_hns: i64,
    audio: bool,
}

#[cfg(windows)]
fn probe(mp4: &Path) -> Result<Probe, String> {
    use znimok_video::traits::{Decoded, VideoDecoder};
    let mut dec = znimok_video_win::MfDecoder::open(mp4).map_err(|e| e.to_string())?;
    let info = dec.info().clone();
    let poster = loop {
        match dec.next().map_err(|e| e.to_string())? {
            Some(Decoded::Video { frame, .. }) => break crate::video::nv12_to_rgba(&frame),
            Some(_) => continue,
            None => return Err("no frames".into()),
        }
    };
    Ok(Probe {
        poster,
        width: info.width,
        height: info.height,
        duration_hns: info.duration_hns,
        audio: info.audio,
    })
}

/// macOS (ZK-205): the poster by AVAssetReader, the rest from the file's own boxes.
#[cfg(target_os = "macos")]
fn probe(mp4: &Path) -> Result<Probe, String> {
    let (w, h, rgba) = znimok_video_mac::poster::first_frame(mp4)?;
    let info = znimok_video::check::mp4::read_mp4_file(mp4).map_err(|e| e.to_string())?;
    Ok(Probe {
        poster: znimok_core::Raster::new(w, h, rgba),
        width: w,
        height: h,
        duration_hns: (info.duration_s() * 1e7).round() as i64,
        audio: info.audio().is_some(),
    })
}

/// An exported MP4 as a new video document in the library (its first frame the poster): the
/// document's path.
#[cfg(any(windows, target_os = "macos"))]
pub fn wrap_into_library(
    mp4: &Path,
    lib: &Path,
    name: &str,
    fps: u32,
    sound: bool,
) -> Result<PathBuf, String> {
    let info = probe(mp4)?;
    let poster = info.poster.clone();
    let mut doc = znimok_core::Document::from_raster(name.to_string(), poster.clone());
    doc.meta.created_ms = chrono::Local::now().timestamp_millis();
    doc.meta.source = "export".into();
    let frames = ((info.duration_hns as f64 / 1e7) * f64::from(fps))
        .round()
        .max(1.0) as u32;
    let vinfo = znimok_format::VideoInfo {
        width: info.width,
        height: info.height,
        fps_milli: fps * 1000,
        frames,
        duration_hns: info.duration_hns,
        codec: znimok_format::video::CODEC_H264,
    };
    let mut video = znimok_format::Video::new(vinfo);
    if sound && info.audio {
        video.audio = vec![znimok_format::video::AudioTrack::default()];
    }
    doc.timeline = Some(video.edit.to_timeline());
    let small = {
        let k = (320.0 / f64::from(poster.width))
            .min(240.0 / f64::from(poster.height))
            .min(1.0);
        let (w, h) = (
            ((f64::from(poster.width) * k).round() as u32).max(1),
            ((f64::from(poster.height) * k).round() as u32).max(1),
        );
        let px = znimok_export::scale_rgba(&poster.rgba, (poster.width, poster.height), (w, h));
        znimok_core::Raster::new(w, h, px)
    };
    let opts = znimok_format::WriteOptions {
        app_version: format!("Znimok {}", env!("CARGO_PKG_VERSION")),
        thumbnail: Some(small),
        ..Default::default()
    };
    std::fs::create_dir_all(lib).map_err(|e| e.to_string())?;
    let path = crate::library::new_path(lib, &doc.id.simple().to_string());
    let part = path.with_extension("part");
    let len = std::fs::metadata(mp4).map_err(|e| e.to_string())?.len();
    {
        use std::io::Write;
        let src = std::io::BufReader::new(std::fs::File::open(mp4).map_err(|e| e.to_string())?);
        let mut out =
            std::io::BufWriter::new(std::fs::File::create(&part).map_err(|e| e.to_string())?);
        let r = {
            let _guard = crate::app::SAVE_LOCK
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            znimok_format::write_video_to(&mut out, &doc, &video, src, len, &opts)
        };
        if let Err(e) = r {
            drop(out);
            let _ = std::fs::remove_file(&part);
            return Err(e.to_string());
        }
        out.flush().map_err(|e| e.to_string())?;
    }
    std::fs::rename(&part, &path).map_err(|e| e.to_string())?;
    Ok(path)
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn wrap_into_library(
    _mp4: &Path,
    _lib: &Path,
    _name: &str,
    _fps: u32,
    _sound: bool,
) -> Result<PathBuf, String> {
    Err("MP4 export on this system comes with its encoder".into())
}

/// A `.zreport` (ZK-98) opened: its video, poster and log as a new video document in the
/// library — the document's path.
pub fn import_zreport(path: &Path, lib: &Path) -> Result<PathBuf, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let r = znimok_report::read_zreport(&bytes)?;
    let fps = if r.fps > 0.0 { r.fps } else { 30.0 };
    let (w, h) = (r.width.max(2), r.height.max(2));
    let poster = r
        .poster_png
        .as_deref()
        .and_then(|p| image::load_from_memory(p).ok())
        .map(|i| {
            let i = i.to_rgba8();
            let (pw, ph) = i.dimensions();
            znimok_core::Raster::new(pw, ph, i.into_raw())
        })
        .unwrap_or_else(|| znimok_core::Raster::solid(w, h, znimok_core::Rgb::new(0, 0, 0)));
    let name = if r.title.is_empty() {
        path.file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    } else {
        r.title.clone()
    };
    let mut doc = znimok_core::Document::from_raster(name, poster.clone());
    doc.meta.created_ms = chrono::Local::now().timestamp_millis();
    doc.meta.source = "zreport".into();
    let frames = (r.seconds * fps).round().max(1.0) as u32;
    let mut video = znimok_format::Video::new(znimok_format::VideoInfo {
        width: w,
        height: h,
        fps_milli: (fps * 1000.0).round() as u32,
        frames,
        duration_hns: (r.seconds * 1e7).round() as i64,
        codec: znimok_format::video::CODEC_H264,
    });
    if r.audio {
        video.audio = vec![znimok_format::video::AudioTrack::default()];
    }
    video.devlog = r.log;
    doc.timeline = Some(video.edit.to_timeline());
    let small = {
        let k = (320.0 / f64::from(poster.width))
            .min(240.0 / f64::from(poster.height))
            .min(1.0);
        let (tw, th) = (
            ((f64::from(poster.width) * k).round() as u32).max(1),
            ((f64::from(poster.height) * k).round() as u32).max(1),
        );
        let px = znimok_export::scale_rgba(&poster.rgba, (poster.width, poster.height), (tw, th));
        znimok_core::Raster::new(tw, th, px)
    };
    let opts = znimok_format::WriteOptions {
        app_version: format!("Znimok {}", env!("CARGO_PKG_VERSION")),
        thumbnail: Some(small),
        ..Default::default()
    };
    std::fs::create_dir_all(lib).map_err(|e| e.to_string())?;
    let dest = crate::library::new_path(lib, &doc.id.simple().to_string());
    let part = dest.with_extension("part");
    let data = {
        let _guard = crate::app::SAVE_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        znimok_format::write_video(&doc, &video, &r.mp4, &opts)
    };
    std::fs::write(&part, data).map_err(|e| e.to_string())?;
    std::fs::rename(&part, &dest).map_err(|e| e.to_string())?;
    Ok(dest)
}

/// A file on the clipboard (to paste into a chat or a folder).
pub fn copy_file(path: &Path) -> Result<(), String> {
    use znimok_platform::{ClipItem, Clipboard};
    let items = [ClipItem::Files(vec![path.to_path_buf()])];
    #[cfg(windows)]
    {
        znimok_win::WinClipboard::new()
            .write(&items)
            .map_err(|e| e.to_string())
    }
    #[cfg(target_os = "macos")]
    {
        znimok_mac::MacClipboard::new()
            .write(&items)
            .map_err(|e| e.to_string())
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = items;
        Err("no clipboard".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choices_map_to_the_engine() {
        let mut v = VidExport::default();
        assert_eq!(v.ext(), "mp4");
        assert_eq!(
            v.engine_kind(true),
            Some(znimok_export::Kind::Mp4 { sound: true })
        );
        v.kind = GIF;
        v.limit = 4;
        assert_eq!(v.limit_bytes(), Some(25 * 1024 * 1024));
        assert!(matches!(
            v.engine_kind(true),
            Some(znimok_export::Kind::Gif { width: 640, .. })
        ));
        v.kind = FRAME;
        assert_eq!(v.engine_kind(true), None);
    }
}
