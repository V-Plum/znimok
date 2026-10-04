//! A recording exported or looked at by an agent (ZK-243), in this process — the app does not
//! have to run: MP4, GIF, the HTML page (with the browser's log when it has one: the report's
//! page), the report as one page or a `.zreport`, one frame as a picture; and frames to look at.
//! The video is decoded and encoded on the GPU (`znimok_play::headless_gpu`), as the app does.

use crate::tools::{Agent, Output, arg_str, image};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use znimok_core::{Document, Raster};
use znimok_format::VideoPart;

/// The formats of a recording's export, and their files' extensions.
pub(crate) const FORMATS: &[(&str, &str)] = &[
    ("mp4", "mp4"),
    ("gif", "gif"),
    ("html", "html"),
    ("report", "html"),
    ("zreport", "zreport"),
    ("png", "png"),
    ("jpeg", "jpg"),
    ("webp", "webp"),
];

/// Frames an agent may ask to see at once.
const FRAMES_MAX: usize = 8;
/// The longest side of a frame shown to an agent.
const FRAME_SIDE: u32 = 1280;

fn gpu() -> Result<&'static znimok_play::Gpu, String> {
    static GPU: OnceLock<Result<znimok_play::Gpu, String>> = OnceLock::new();
    GPU.get_or_init(znimok_play::headless_gpu)
        .as_ref()
        .map_err(|e| format!("no GPU to decode the video: {e}"))
}

fn open(path: &Path) -> Result<(Document, VideoPart), String> {
    let (doc, part) =
        znimok_format::open_parts(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let part = part.ok_or("this is a screenshot, not a recording")?;
    Ok((doc, part))
}

fn source(part: &VideoPart) -> znimok_play::Source {
    znimok_play::Source::of_part(part, crate::data_dir().join("Cache").join("video"))
}

/// The settings the person set, or the defaults.
fn prefs() -> znimok_settings::Settings {
    znimok_settings::Store::open_default()
        .map(|s| s.get())
        .unwrap_or_default()
}

/// The frame at `ms` of the recording (as recorded, before the cuts), with the marks shown at
/// that moment, at most [`FRAME_SIDE`] on its longest side unless `full`.
fn frame_at(
    frames: &mut znimok_play::Frames,
    doc: &Document,
    part: &VideoPart,
    ms: i64,
    full: bool,
) -> Result<(i64, Raster), String> {
    let fps = (f64::from(part.video.info.fps_milli) / 1000.0).max(1.0);
    let last = i64::from(part.video.info.frames).max(1) - 1;
    let want = ((ms.max(0) as f64 / 1000.0 * fps).round() as i64).min(last);
    frames.seek(want)?;
    let mut got = None;
    while let Some((a, n, r)) = frames.next_frame()? {
        if a + n.max(1) > want || a >= last {
            got = Some(r);
            break;
        }
        got = Some(r);
    }
    let raster = got.ok_or("no frame at that time")?;
    let (frame, mut out) = znimok_export::out_geometry(doc, &part.video);
    let side = out.0.max(out.1);
    if !full && side > FRAME_SIDE {
        let k = f64::from(FRAME_SIDE) / f64::from(side);
        let even = |v: u32| ((f64::from(v) * k).round() as u32).max(2) & !1;
        out = (even(out.0), even(out.1));
    }
    let mut comp = znimok_export::Composer::new(doc, frame, out);
    let px = comp.compose(Arc::new(raster), want);
    Ok((want, Raster::new(out.0, out.1, px)))
}

/// `export` of a recording to `out` in `format` (one of [`FORMATS`]).
pub(crate) fn export(
    path: &Path,
    format: &str,
    args: &Value,
    out: &Path,
) -> Result<Output, String> {
    let (doc, part) = open(path)?;
    let gpu = gpu()?;
    // One frame as a picture.
    if let Some(fmt) = znimok_export_image_format(format) {
        let mut frames = znimok_play::Frames::open(gpu, &source(&part))?;
        let ms = args["at_ms"].as_i64().unwrap_or(0);
        let (i, r) = frame_at(&mut frames, &doc, &part, ms, true)?;
        let shot = Document::from_raster(doc.name.clone(), r);
        crate::library::export(&shot, fmt, out)?;
        return Ok(Output::ok(
            json!({"path": out.display().to_string(), "format": format, "frame": i}),
            vec![],
        ));
    }
    let prefs = prefs();
    let has_log = part.video.devlog.is_some();
    let report = |zip: bool| {
        // Hidden as the person set it; when it is left to the export, hidden unless the agent
        // says otherwise.
        let hide = match prefs.video.hide_on_export {
            znimok_settings::HideOnExport::Never => false,
            znimok_settings::HideOnExport::Always => true,
            znimok_settings::HideOnExport::Ask => args["hide"].as_bool().unwrap_or(true),
        };
        let lang = match arg_str(args, "language") {
            Some("uk") => "uk",
            Some("en") => "en",
            _ => match prefs.video.report_lang.as_str() {
                "uk" => "uk",
                "en" => "en",
                _ => znimok_i18n::choose_language(
                    prefs.general.language.as_deref(),
                    znimok_i18n::system_language().as_deref(),
                ),
            },
        };
        let tr = znimok_i18n::Localizer::new(lang);
        znimok_export::Kind::Report(Box::new(znimok_export::report_options(
            &doc,
            &part.video,
            &prefs,
            &tr,
            None,
            zip,
            hide,
        )))
    };
    let kind = match format {
        "mp4" => znimok_export::Kind::Mp4 {
            sound: args["sound"].as_bool().unwrap_or(true),
        },
        "gif" => {
            let (_, size) = znimok_export::out_geometry(&doc, &part.video);
            let width = args["gif_width"]
                .as_u64()
                .map(|w| w.clamp(80, 1920) as u32)
                .unwrap_or(size.0.min(640));
            znimok_export::Kind::Gif {
                width,
                fps: args["gif_fps"].as_f64().unwrap_or(10.0).clamp(1.0, 30.0),
                dither: true,
            }
        }
        // A recording with the browser's log: its page is the report's (ZK-226).
        "html" if !has_log => znimok_export::Kind::Html,
        "html" | "report" => report(false),
        "zreport" => report(true),
        f => return Err(format!("unknown format «{f}» for a recording")),
    };
    let job = znimok_export::Job {
        source: source(&part),
        doc: doc.clone(),
        video: part.video.clone(),
        kind,
        dest: out.to_path_buf(),
    };
    let progress = znimok_export::Progress::default();
    let done = znimok_export::run(&job, gpu, &progress).map_err(|e| {
        if e == znimok_export::REPORT_TOO_BIG {
            "the report is over 100 MB as one page — export it as zreport".to_string()
        } else {
            e
        }
    })?;
    Ok(Output::ok(
        json!({
            "path": out.display().to_string(),
            "format": format,
            "bytes": done.bytes,
            "frames": done.frames,
            "with_devtools_log": matches!(format, "report" | "zreport") || (format == "html" && has_log),
        }),
        vec![],
    ))
}

fn znimok_export_image_format(f: &str) -> Option<crate::library::ExportFormat> {
    match f {
        "png" | "jpeg" | "webp" => crate::library::ExportFormat::parse(f),
        _ => None,
    }
}

/// `video_frames`: frames of a recording to look at, with the marks of their moment.
/// One frame of a recording as a PNG, full size, with the marks of its moment — the resource
/// `znimok://library/<id>/frame/<n>` (ZK-239); `n` counts frames as recorded.
pub(crate) fn frame_png(path: &Path, n: i64) -> Result<Vec<u8>, String> {
    let (doc, part) = open(path)?;
    let frames_in = i64::from(part.video.info.frames);
    if n < 0 || n >= frames_in {
        return Err(format!("the recording has frames 0…{}", frames_in - 1));
    }
    let fps = (f64::from(part.video.info.fps_milli) / 1000.0).max(1.0);
    let ms = (n as f64 * 1000.0 / fps).round() as i64;
    let mut frames = znimok_play::Frames::open(gpu()?, &source(&part))?;
    let (_, r) = frame_at(&mut frames, &doc, &part, ms, true)?;
    crate::library::encode_png(&r)
}

pub(crate) fn frames(agent: &Agent, args: &Value) -> Result<Output, String> {
    let d = arg_str(args, "document").ok_or("«document» is required")?;
    let path: PathBuf = agent
        .lib
        .resolve(d)
        .ok_or_else(|| format!("no document «{d}» in the library"))?;
    let (doc, part) = open(&path)?;
    let length = part.video.info.duration_hns / 10_000;
    let times: Vec<i64> = if let Some(list) = args["at_ms"].as_array() {
        list.iter()
            .filter_map(Value::as_i64)
            .take(FRAMES_MAX)
            .collect()
    } else {
        let n = args["count"]
            .as_u64()
            .unwrap_or(4)
            .clamp(1, FRAMES_MAX as u64) as i64;
        (0..n).map(|i| length * (2 * i + 1) / (2 * n)).collect()
    };
    if times.is_empty() {
        return Err("give at_ms (a list of times) or count".into());
    }
    let mut frames = znimok_play::Frames::open(gpu()?, &source(&part))?;
    let fps = (f64::from(part.video.info.fps_milli) / 1000.0).max(1.0);
    let mut shown = Vec::new();
    let mut pictures = Vec::new();
    for ms in times {
        let (i, r) = frame_at(&mut frames, &doc, &part, ms, false)?;
        shown.push(json!({"at_ms": (i as f64 / fps * 1000.0).round() as i64, "frame": i, "width": r.width, "height": r.height}));
        pictures.extend(image(&r));
    }
    Ok(Output::ok(
        json!({"duration_ms": length, "frames": shown}),
        pictures,
    ))
}
