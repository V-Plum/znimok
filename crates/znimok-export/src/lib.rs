//! Video export (ZK-95).
//!
//! ```text
//! the recording ─► znimok-play Frames (decoded and converted on the GPU, read back as RGBA)
//!   ─► compose: the document with this frame as its picture and the marks live on it
//!      (znimok-render: frame, tone, Hide on the real pixels, scaled to the size on export)
//!   ─► MP4: RGBA → NV12 (BT.709, studio range) → Media Foundation H.264 (Windows),
//!           the audio tracks mixed (volume, offset, muted), cut by time with 10 ms fades
//!   ─► GIF: palette from up to 12 probes, frames at the GIF's rate, differences
//! ```
//!
//! A video without any edit is copied as it is (LH: `CopyFile`) — its MP4 already sits in the
//! document. Each job runs on the caller's thread with a progress counter and a cancel flag.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use znimok_core::{Document, IRect, Raster};
use znimok_format::video::Video;
use znimok_play::{Frames, Gpu, Source};
use znimok_render::{Renderer, View, vello_cpu::Pixmap};
use znimok_video::edit::VideoEdit;
use znimok_video::export::{KeepSeg, kept_frames, src_of_out};
use znimok_video::gifenc;

/// What to make.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Mp4 {
        /// Mix the tracks into an AAC track (the tracks' own `muted` still count).
        sound: bool,
    },
    Gif {
        /// Width of the GIF; the height follows the frame's proportions.
        width: u32,
        fps: f64,
        dither: bool,
    },
    /// One HTML page: the MP4 inside it, the marks a live layer shown in their time (Hide and
    /// the marker, which work on the pixels, go into the video itself).
    Html,
    /// The developer report (ZK-98): the HTML page's video and marks with the DevTools log
    /// beside them, as one page or a `.zreport`.
    Report(Box<ReportOptions>),
}

/// What a report is made with.
#[derive(Clone, Debug, PartialEq)]
pub struct ReportOptions {
    /// A `.zreport` (the video beside the page), else one HTML page.
    pub zip: bool,
    /// Hide the log's sensitive values: the keys to hide (and the secrets found by their look).
    pub mask: Option<Vec<String>>,
    /// The header; `masked` is a template with `{n}` for the count of hidden values. Size, rate
    /// and length are filled in here.
    pub meta: znimok_report::Meta,
    /// The viewer's words by their keys.
    pub strings: std::collections::BTreeMap<String, String>,
}

/// The report is over [`znimok_report::HTML_LIMIT`] as one page: the `.zreport` takes it.
pub const REPORT_TOO_BIG: &str = "report-too-big";

/// A video document to export.
#[derive(Clone)]
pub struct Job {
    pub source: Source,
    /// The poster document: marks, their times (`timeline.marks`), the cuts, the frame, the tone.
    pub doc: Document,
    pub video: Video,
    pub kind: Kind,
    pub dest: PathBuf,
}

/// Progress 0…1000 and a cancel flag, shared with the UI.
#[derive(Default)]
pub struct Progress {
    pub done: AtomicU32,
    pub cancel: AtomicBool,
}

impl Progress {
    fn set(&self, part: f64) {
        self.done
            .store((part.clamp(0.0, 1.0) * 1000.0) as u32, Ordering::Relaxed);
    }

    fn cancelled(&self) -> Result<(), String> {
        if self.cancel.load(Ordering::Relaxed) {
            Err(CANCELLED.into())
        } else {
            Ok(())
        }
    }
}

/// The error of a cancelled job.
pub const CANCELLED: &str = "cancelled";

/// What a finished job made.
#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub frames: i64,
    pub bytes: u64,
    /// Copied without re-encoding.
    pub copied: bool,
    pub hardware: bool,
}

/// The cuts as kept segments (the whole video when the timeline is missing or broken).
pub fn keep_of(doc: &Document, frames: i64) -> Vec<KeepSeg> {
    doc.timeline
        .as_ref()
        .and_then(VideoEdit::from_timeline)
        .unwrap_or_else(|| VideoEdit::new(frames))
        .keep_segs()
}

/// Nothing changes the pictures or the length: the MP4 can go out as it is.
pub fn untouched(doc: &Document, video: &Video) -> bool {
    let frames = i64::from(video.info.frames);
    let keep = keep_of(doc, frames);
    keep.len() == 1
        && keep[0] == KeepSeg::new(0, frames)
        && doc.objects.iter().all(|o| o.hidden)
        && doc.crop.is_none()
        && video.out_size.is_none()
        && znimok_render::develop::tone_is_default(&doc.recipe)
        && video
            .audio
            .iter()
            .all(|t| !t.muted && t.volume == 100 && t.offset_ms == 0)
}

/// The frame shown (the crop, or the whole picture) and the size on export, both even.
pub fn out_geometry(doc: &Document, video: &Video) -> (IRect, (u32, u32)) {
    let f = doc.frame();
    let even = |v: i32| (v.max(2) as u32) & !1;
    let size = video
        .out_size
        .map(|(w, h)| ((w.max(2) + 1) & !1, (h.max(2) + 1) & !1))
        .unwrap_or((even(f.w), even(f.h)));
    (f, size)
}

/// Draws output frames: the document with a decoded frame as its picture.
pub struct Composer {
    renderer: Renderer,
    doc: Document,
    frame: IRect,
    out: (u32, u32),
    pix: Pixmap,
}

impl Composer {
    pub fn new(doc: &Document, frame: IRect, out: (u32, u32)) -> Self {
        Self {
            renderer: Renderer::new(),
            doc: doc.clone(),
            frame,
            out,
            pix: Pixmap::new(1, 1),
        }
    }

    /// Source frame `src` (pixels `raster`) with the marks live on it, at the size on export, as
    /// straight RGBA.
    pub fn compose(&mut self, raster: Arc<Raster>, src: i64) -> Vec<u8> {
        let i = self.doc.source as usize;
        if self
            .doc
            .banks
            .get(i)
            .is_some_and(|b| (b.width, b.height) == (raster.width, raster.height))
        {
            self.doc.banks[i] = raster;
        }
        self.doc.shown_frame = Some(src);
        let (ow, oh) = self.out;
        let sx = f64::from(ow) / f64::from(self.frame.w.max(1));
        let sy = f64::from(oh) / f64::from(self.frame.h.max(1));
        // One scale for the renderer; a different height (proportions unlocked) is resampled.
        let h1 = (f64::from(self.frame.h) * sx).round().max(1.0) as u32;
        let view = View {
            scale: sx,
            origin: znimok_render::vello_cpu::kurbo::Point::new(
                f64::from(self.frame.x),
                f64::from(self.frame.y),
            ),
            width: ow.min(65535) as u16,
            height: h1.min(65535) as u16,
        };
        self.renderer.render(&self.doc, view, &mut self.pix);
        let mut rgba = self.pix.data_as_u8_slice().to_vec();
        // The picture is opaque; whatever the marks left translucent is un-premultiplied.
        for p in rgba.as_chunks_mut::<4>().0 {
            let a = u32::from(p[3]);
            if a != 0 && a != 255 {
                for c in &mut p[..3] {
                    *c = ((u32::from(*c) * 255 + a / 2) / a).min(255) as u8;
                }
            }
            p[3] = 255;
        }
        if h1 != oh {
            rgba = resize_rows(&rgba, ow, h1, oh);
        }
        let _ = sy;
        rgba
    }
}

/// Rows resampled from `h0` to `h1` (linear), the width unchanged.
fn resize_rows(src: &[u8], w: u32, h0: u32, h1: u32) -> Vec<u8> {
    let (w, h0, h1) = (w as usize, h0 as usize, h1 as usize);
    let mut out = vec![0u8; w * h1 * 4];
    for y in 0..h1 {
        let fy = ((y as f64 + 0.5) * h0 as f64 / h1 as f64 - 0.5).clamp(0.0, (h0 - 1) as f64);
        let (y0, t) = (fy.floor() as usize, fy - fy.floor());
        let y1 = (y0 + 1).min(h0 - 1);
        for x in 0..w * 4 {
            let a = f64::from(src[y0 * w * 4 + x]);
            let b = f64::from(src[y1 * w * 4 + x]);
            out[y * w * 4 + x] = (a + (b - a) * t).round() as u8;
        }
    }
    out
}

/// Straight RGBA → NV12 (BT.709, studio range), chroma averaged over 2 × 2.
pub fn rgba_to_nv12(rgba: &[u8], w: u32, h: u32) -> Vec<u8> {
    let (w, h) = (w as usize, h as usize);
    let mut out = vec![0u8; w * h * 3 / 2];
    let (yp, uvp) = out.split_at_mut(w * h);
    let lum = |r: f64, g: f64, b: f64| 0.2126 * r + 0.7152 * g + 0.0722 * b;
    for y in 0..h {
        for x in 0..w {
            let o = (y * w + x) * 4;
            let (r, g, b) = (
                f64::from(rgba[o]),
                f64::from(rgba[o + 1]),
                f64::from(rgba[o + 2]),
            );
            yp[y * w + x] = (16.0 + lum(r, g, b) * 219.0 / 255.0)
                .round()
                .clamp(16.0, 235.0) as u8;
        }
    }
    for y in (0..h).step_by(2) {
        for x in (0..w).step_by(2) {
            let (mut r, mut g, mut b) = (0.0, 0.0, 0.0);
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let o = ((y + dy).min(h - 1) * w + (x + dx).min(w - 1)) * 4;
                r += f64::from(rgba[o]);
                g += f64::from(rgba[o + 1]);
                b += f64::from(rgba[o + 2]);
            }
            let (r, g, b) = (r / 4.0, g / 4.0, b / 4.0);
            let yy = lum(r, g, b);
            let cb = (b - yy) / 1.8556;
            let cr = (r - yy) / 1.5748;
            let o = (y / 2) * w + x;
            uvp[o] = (128.0 + cb * 224.0 / 255.0).round().clamp(16.0, 240.0) as u8;
            uvp[o + 1] = (128.0 + cr * 224.0 / 255.0).round().clamp(16.0, 240.0) as u8;
        }
    }
    out
}

/// Runs a job on this thread. The result goes to `dest` through `dest.part`.
pub fn run(job: &Job, gpu: &Gpu, progress: &Progress) -> Result<Outcome, String> {
    let part = job.dest.with_extension(format!(
        "{}.part",
        job.dest
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("out")
    ));
    let _ = std::fs::remove_file(&part);
    let r = match &job.kind {
        Kind::Mp4 { sound } if untouched(&job.doc, &job.video) && *sound => {
            copy_mp4(&job.source, &part).map(|bytes| Outcome {
                frames: i64::from(job.video.info.frames),
                bytes,
                copied: true,
                hardware: false,
            })
        }
        Kind::Mp4 { sound } => mp4(job, gpu, progress, &part, *sound),
        Kind::Gif { width, fps, dither } => gif(job, gpu, progress, &part, *width, *fps, *dither),
        Kind::Html => html(job, gpu, progress, &part),
        Kind::Report(o) => report(job, gpu, progress, &part, o),
    };
    match r {
        Ok(o) => {
            let _ = std::fs::remove_file(&job.dest);
            std::fs::rename(&part, &job.dest).map_err(|e| e.to_string())?;
            progress.set(1.0);
            Ok(o)
        }
        Err(e) => {
            let _ = std::fs::remove_file(&part);
            Err(e)
        }
    }
}

/// The MP4 as it lies in the document.
fn copy_mp4(source: &Source, to: &Path) -> Result<u64, String> {
    let from = source.as_file()?;
    std::fs::copy(&from, to).map_err(|e| e.to_string())
}

/// Bits per second for an export of `out` from a recording of `bytes` over `secs`: the source's
/// rate × 1.25, scaled by the area, in [1, 100] Mbit/s (LH).
pub fn bitrate(bytes: u64, secs: f64, src: (u32, u32), out: (u32, u32), fps: f64) -> u32 {
    let src_rate = if secs > 0.0 {
        bytes as f64 * 8.0 / secs
    } else {
        f64::from(out.0) * f64::from(out.1) * fps * 0.15
    };
    let area =
        (f64::from(out.0) * f64::from(out.1)) / (f64::from(src.0.max(1)) * f64::from(src.1.max(1)));
    (src_rate * 1.25 * area.min(1.0)).clamp(1e6, 100e6) as u32
}

#[cfg(windows)]
fn mp4(
    job: &Job,
    gpu: &Gpu,
    progress: &Progress,
    part: &Path,
    sound: bool,
) -> Result<Outcome, String> {
    use znimok_video::export::{AudioCut, split_video_frames};
    use znimok_video_win::export::{AudioTrackReader, CHANNELS, Mp4Config, Mp4Writer, RATE};
    let info = job.video.info;
    let fps = info.fps();
    let frames = i64::from(info.frames);
    let keep = keep_of(&job.doc, frames);
    let total = kept_frames(&keep).max(1);
    let (frame, out) = out_geometry(&job.doc, &job.video);
    let mp4_file = job.source.as_file()?;
    let bytes = std::fs::metadata(&mp4_file).map(|m| m.len()).unwrap_or(0);
    // The sound: every track that is on, at its volume and offset, mixed.
    let mut mixed: Vec<i16> = Vec::new();
    if sound {
        let readers = AudioTrackReader::open_all(&mp4_file)?;
        let mut tracks: Vec<Vec<i16>> = Vec::new();
        for (i, mut r) in readers.into_iter().enumerate() {
            let t = job.video.audio.get(i).cloned().unwrap_or_default();
            if t.muted {
                continue;
            }
            let gain = f32::from(t.volume) / 100.0;
            let shift = i64::from(t.offset_ms) * i64::from(RATE) / 1000;
            let mut buf: Vec<i16> = Vec::new();
            while let Some((ts, pcm)) = r.next_block()? {
                progress.cancelled()?;
                let at = (ts as f64 * f64::from(RATE) / 1e7).round() as i64 + shift;
                if at < 0 {
                    continue;
                }
                let o = at as usize * CHANNELS as usize;
                if buf.len() < o + pcm.len() {
                    buf.resize(o + pcm.len(), 0);
                }
                for (k, v) in pcm.iter().enumerate() {
                    buf[o + k] = (f32::from(*v) * gain).clamp(-32768.0, 32767.0) as i16;
                }
            }
            tracks.push(buf);
        }
        let refs: Vec<&[i16]> = tracks.iter().map(Vec::as_slice).collect();
        mixed = znimok_video::audio::mix_tracks_s16(&refs);
    }
    let has_sound = !mixed.is_empty();
    let mut writer = Mp4Writer::open(
        part,
        &Mp4Config {
            width: out.0,
            height: out.1,
            fps: fps.round().max(1.0) as u32,
            bitrate: bitrate(
                bytes,
                info.duration_hns as f64 / 1e7,
                (info.width, info.height),
                out,
                fps,
            ),
            audio: has_sound,
        },
    )?;
    let audio = if has_sound {
        AudioCut::new(&keep, fps, i64::from(RATE), CHANNELS as usize, frames).cut(0, &mixed)
    } else {
        Vec::new()
    };
    drop(mixed);
    // Audio is written ahead of the video by up to a second, so the file stays interleaved.
    let mut next_audio = 0usize;
    let mut audio_up_to = |w: &mut Mp4Writer, t: i64| -> Result<(), String> {
        while next_audio < audio.len() && audio[next_audio].time_hns <= t + 10_000_000 {
            let a = &audio[next_audio];
            // Blocks of at most a second.
            let step = (RATE * CHANNELS) as usize;
            for (k, chunk) in a.pcm.chunks(step).enumerate() {
                w.audio(chunk, a.time_hns + k as i64 * 10_000_000)?;
            }
            next_audio += 1;
        }
        Ok(())
    };
    let mut src = Frames::open(gpu, &job.source)?;
    let mut comp = Composer::new(&job.doc, frame, out);
    let first = keep.first().map_or(0, |s| s.a);
    src.seek(first)?;
    let mut written = 0i64;
    while let Some((idx, n, raster)) = src.next_frame()? {
        progress.cancelled()?;
        if keep.last().is_some_and(|s| idx >= s.b) {
            break;
        }
        let raster = Arc::new(raster);
        for o in split_video_frames(&keep, idx, n, fps) {
            let rgba = comp.compose(raster.clone(), o.src_frame);
            let nv12 = rgba_to_nv12(&rgba, out.0, out.1);
            audio_up_to(&mut writer, o.time_hns)?;
            writer.video(&nv12, o.time_hns, o.duration_hns)?;
            written += 1;
            progress.set(written as f64 / total as f64 * 0.99);
        }
    }
    audio_up_to(&mut writer, i64::MAX / 2)?;
    let hardware = writer.hardware;
    writer.finish()?;
    Ok(Outcome {
        frames: written,
        bytes: std::fs::metadata(part).map(|m| m.len()).unwrap_or(0),
        copied: false,
        hardware,
    })
}

#[cfg(not(windows))]
fn mp4(
    _job: &Job,
    _gpu: &Gpu,
    _progress: &Progress,
    _part: &Path,
    _sound: bool,
) -> Result<Outcome, String> {
    Err("MP4 export on this system comes with its video encoder (ZK-88)".into())
}

/// The size of a GIF `width` wide for the frame (even height not needed; at least 2).
pub fn gif_size(frame: IRect, width: u32) -> (u32, u32) {
    let w = width.clamp(16, 4096);
    let h = ((f64::from(w) * f64::from(frame.h.max(1)) / f64::from(frame.w.max(1))).round() as u32)
        .max(2);
    (w, h)
}

/// Source frames the GIF shows, in order: one per GIF frame at `fps` over what is kept.
pub fn gif_frames(keep: &[KeepSeg], src_fps: f64, fps: f64) -> Vec<i64> {
    let total = kept_frames(keep);
    let n = ((total as f64 / src_fps.max(0.01)) * fps).floor().max(1.0) as i64;
    (0..n)
        .map(|m| src_of_out(keep, (m as f64 * src_fps / fps).floor() as i64))
        .collect()
}

/// Up to 12 source frames spread over what is kept (the palette's and the estimate's probes).
pub fn probe_frames(wanted: &[i64]) -> Vec<i64> {
    let n = wanted.len();
    let k = n.min(12);
    let mut v: Vec<i64> = (0..k).map(|i| wanted[i * n / k.max(1)]).collect();
    v.dedup();
    v
}

/// Decoded and composed source frames `wanted` (sorted), in one pass.
fn compose_frames(
    src: &mut Frames,
    comp: &mut Composer,
    wanted: &[i64],
    scale_to: (u32, u32),
    progress: &Progress,
    span: (f64, f64),
    mut each: impl FnMut(usize, Vec<u8>) -> Result<(), String>,
) -> Result<(), String> {
    let Some(&first) = wanted.first() else {
        return Ok(());
    };
    src.seek(first)?;
    let mut k = 0usize;
    let mut last: Option<(i64, Vec<u8>)> = None;
    while k < wanted.len() {
        progress.cancelled()?;
        let Some((idx, n, raster)) = src.next_frame()? else {
            break;
        };
        let raster = Arc::new(raster);
        while k < wanted.len() && wanted[k] < idx + n {
            let f = wanted[k];
            let rgba = match &last {
                Some((lf, px)) if *lf == f => px.clone(),
                _ => {
                    let px = comp.compose(raster.clone(), f.max(idx));
                    let px = scale_rgba(&px, comp.out, scale_to);
                    last = Some((f, px.clone()));
                    px
                }
            };
            each(k, rgba)?;
            k += 1;
            progress.set(span.0 + (span.1 - span.0) * k as f64 / wanted.len() as f64);
        }
    }
    // Past the end of the stream: the last picture repeats.
    if let Some((_, px)) = last {
        while k < wanted.len() {
            each(k, px.clone())?;
            k += 1;
        }
    }
    Ok(())
}

/// Area-averaged downscale (or nearest upscale) of straight RGBA.
pub fn scale_rgba(src: &[u8], from: (u32, u32), to: (u32, u32)) -> Vec<u8> {
    if from == to {
        return src.to_vec();
    }
    let (fw, fh) = (from.0 as usize, from.1 as usize);
    let (tw, th) = (to.0 as usize, to.1 as usize);
    let mut out = vec![0u8; tw * th * 4];
    for y in 0..th {
        let y0 = y * fh / th;
        let y1 = ((y + 1) * fh / th).max(y0 + 1).min(fh);
        for x in 0..tw {
            let x0 = x * fw / tw;
            let x1 = ((x + 1) * fw / tw).max(x0 + 1).min(fw);
            let mut acc = [0u32; 4];
            for yy in y0..y1 {
                for xx in x0..x1 {
                    let o = (yy * fw + xx) * 4;
                    for c in 0..4 {
                        acc[c] += u32::from(src[o + c]);
                    }
                }
            }
            let n = ((y1 - y0) * (x1 - x0)) as u32;
            let o = (y * tw + x) * 4;
            for c in 0..4 {
                out[o + c] = (acc[c] / n.max(1)) as u8;
            }
        }
    }
    out
}

fn gif(
    job: &Job,
    gpu: &Gpu,
    progress: &Progress,
    part: &Path,
    width: u32,
    fps: f64,
    dither: bool,
) -> Result<Outcome, String> {
    let info = job.video.info;
    let frames = i64::from(info.frames);
    let keep = keep_of(&job.doc, frames);
    let (frame, out) = out_geometry(&job.doc, &job.video);
    let size = gif_size(IRect::new(0, 0, out.0 as i32, out.1 as i32), width);
    let wanted = gif_frames(&keep, info.fps(), fps);
    let mut src = Frames::open(gpu, &job.source)?;
    let mut comp = Composer::new(&job.doc, frame, out);
    // Pass 1: the palette from the probes.
    let probes = probe_frames(&wanted);
    let mut samples: Vec<Vec<u8>> = Vec::new();
    compose_frames(
        &mut src,
        &mut comp,
        &probes,
        size,
        progress,
        (0.0, 0.1),
        |_, px| {
            samples.push(px);
            Ok(())
        },
    )?;
    let refs: Vec<&[u8]> = samples.iter().map(Vec::as_slice).collect();
    let palette = gifenc::build_palette(&refs, 256);
    drop(samples);
    // Pass 2: every frame.
    let file = std::io::BufWriter::new(std::fs::File::create(part).map_err(|e| e.to_string())?);
    let mut g = gifenc::GifWriter::new(file, size.0 as u16, size.1 as u16, palette, dither)?;
    compose_frames(
        &mut src,
        &mut comp,
        &wanted,
        size,
        progress,
        (0.1, 0.99),
        |m, px| g.push(&px, gifenc::delay_for(m as i64, fps)),
    )?;
    let n = g.frames;
    g.finish()?;
    Ok(Outcome {
        frames: n as i64,
        bytes: std::fs::metadata(part).map(|m| m.len()).unwrap_or(0),
        copied: false,
        hardware: false,
    })
}

/// Marks that stay in the video of an HTML page: they work on the pixels below.
fn burned(o: &znimok_core::Object) -> bool {
    matches!(
        o.data,
        znimok_core::Data::Hide { .. } | znimok_core::Data::Mark
    )
}

/// Seconds of the output in which source frames `[a, b)` show, over the cuts.
pub fn out_intervals(keep: &[KeepSeg], a: i64, b: i64, fps: f64) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    let mut before = 0i64;
    for s in keep {
        let (x, y) = (a.max(s.a), b.min(s.b));
        if y > x {
            let o0 = before + (x - s.a);
            out.push((o0 as f64 / fps, (o0 + (y - x)) as f64 / fps));
        }
        before += s.len();
    }
    out
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        s.push(T[(n >> 18) as usize & 63] as char);
        s.push(T[(n >> 12) as usize & 63] as char);
        s.push(if c.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        s.push(if c.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    s
}

fn png_of(rgba: &[u8], w: u32, h: u32) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut e = png::Encoder::new(&mut out, w, h);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        let mut wr = e.write_header().map_err(|e| e.to_string())?;
        wr.write_image_data(rgba).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

const PAGE_SCRIPT: &str = "const v=document.querySelector('video'),m=[...document.querySelectorAll('.m')].map(e=>[e,e.dataset.t.split(';').map(s=>s.split(',').map(Number))]);\n(function f(){const t=v.currentTime;for(const[e,r]of m)e.style.display=r.some(([a,b])=>t>=a&&t<b)?'block':'none';requestAnimationFrame(f)})();";

const PAGE_STYLE: &str = ":root{--bg:#f4f5f7;--fg:#16181d;--muted:#5c6270}\n@media (prefers-color-scheme:dark){:root{--bg:#0f1115;--fg:#e8eaee;--muted:#9aa1ad}}\nbody{margin:0;background:var(--bg);color:var(--fg);font:15px/1.5 system-ui,sans-serif}\nmain{margin:0 auto;padding:24px 16px}\nh1{font-size:20px;margin:0 0 12px}\n.v{position:relative;line-height:0;border-radius:10px;overflow:hidden;background:#000}\nvideo{width:100%;height:auto;display:block}\n.m{position:absolute;inset:0;width:100%;height:100%;display:none;pointer-events:none}\np{color:var(--muted);font-size:13px}";

/// What the HTML page and the report share: the MP4 with only the marks that work on its pixels,
/// and each other mark as a picture layer with its times.
struct Parts {
    mp4: Vec<u8>,
    layers: String,
    keep: Vec<KeepSeg>,
    fps: f64,
    frame: IRect,
    out: (u32, u32),
}

fn html(job: &Job, gpu: &Gpu, progress: &Progress, part: &Path) -> Result<Outcome, String> {
    let Parts {
        mp4: mp4_bytes,
        layers,
        keep,
        out,
        ..
    } = parts(job, gpu, progress, part)?;
    let title = esc(&job.doc.name);
    let mut page = String::new();
    page.push_str("<!doctype html>\n<html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>");
    page.push_str(&title);
    page.push_str("</title>\n<style>\n");
    page.push_str(PAGE_STYLE);
    page.push_str(&format!("\nmain{{max-width:{}px}}\n", out.0.max(320)));
    page.push_str("</style></head><body><main>\n<h1>");
    page.push_str(&title);
    page.push_str("</h1>\n<div class=\"v\"><video controls playsinline preload=\"auto\" src=\"data:video/mp4;base64,");
    page.push_str(&base64(&mp4_bytes));
    page.push_str("\"></video>\n");
    page.push_str(&layers);
    page.push_str("</div>\n<p>Znimok</p>\n</main><script>\n");
    page.push_str(PAGE_SCRIPT);
    page.push_str("\n</script></body></html>\n");
    std::fs::write(part, page.as_bytes()).map_err(|e| e.to_string())?;
    Ok(Outcome {
        frames: kept_frames(&keep),
        bytes: page.len() as u64,
        copied: false,
        hardware: false,
    })
}

/// The developer report (ZK-98): the page's video and marks, the log on the exported time
/// (masked when chosen), as one page or a `.zreport` with the poster.
fn report(
    job: &Job,
    gpu: &Gpu,
    progress: &Progress,
    part: &Path,
    o: &ReportOptions,
) -> Result<Outcome, String> {
    let Parts {
        mp4,
        layers,
        keep,
        fps,
        frame,
        out,
    } = parts(job, gpu, progress, part)?;
    let mut events = job
        .video
        .devlog
        .as_ref()
        .map(|log| {
            znimok_report::log_events(log, |ms| {
                let t = f64::from(ms) / 1000.0;
                let f = (t * fps).floor() as i64;
                out_intervals(&keep, f, f + 1, fps)
                    .first()
                    .map(|(a, _)| a + (t - f as f64 / fps).max(0.0))
            })
        })
        .unwrap_or_default();
    let hidden = o.mask.as_ref().map(|keys| {
        znimok_report::mask::events(&mut events, &znimok_report::mask::Rules::new(keys))
    });
    let mut meta = o.meta.clone();
    meta.width = out.0;
    meta.height = out.1;
    meta.fps = fps;
    meta.seconds = kept_frames(&keep) as f64 / fps.max(1e-6);
    meta.audio = job.video.audio.iter().any(|t| !t.muted);
    meta.masked = match hidden {
        Some(n) => o.meta.masked.replace("{n}", &n.to_string()),
        None => String::new(),
    };
    let bytes = if o.zip {
        let poster = poster_png(&job.doc, frame, out)?;
        let f = std::fs::File::create(part).map_err(|e| e.to_string())?;
        let mut w = std::io::BufWriter::new(f);
        znimok_report::write_zreport(
            &mut w,
            &meta,
            &o.strings,
            &events,
            &mp4,
            Some(&poster),
            &layers,
        )
        .map_err(|e| e.to_string())?;
        std::io::Write::flush(&mut w).map_err(|e| e.to_string())?;
        drop(w);
        std::fs::metadata(part).map(|m| m.len()).unwrap_or(0)
    } else {
        let page = znimok_report::page(
            &meta,
            &o.strings,
            &events,
            znimok_report::VideoSrc::Inline(&mp4),
            &layers,
        );
        if page.len() as u64 > znimok_report::HTML_LIMIT {
            return Err(REPORT_TOO_BIG.into());
        }
        std::fs::write(part, page.as_bytes()).map_err(|e| e.to_string())?;
        page.len() as u64
    };
    Ok(Outcome {
        frames: kept_frames(&keep),
        bytes,
        copied: false,
        hardware: false,
    })
}

/// The recording's picture in the exported frame, without marks, as PNG (the `.zreport`'s
/// poster).
fn poster_png(doc: &Document, frame: IRect, out: (u32, u32)) -> Result<Vec<u8>, String> {
    let mut d = doc.clone();
    for o in &mut d.objects {
        o.hidden = true;
    }
    d.shown_frame = None;
    let view = View {
        scale: f64::from(out.0) / f64::from(frame.w.max(1)),
        origin: znimok_render::vello_cpu::kurbo::Point::new(f64::from(frame.x), f64::from(frame.y)),
        width: out.0.min(65535) as u16,
        height: out.1.min(65535) as u16,
    };
    let mut pix = Pixmap::new(1, 1);
    Renderer::new().render(&d, view, &mut pix);
    let mut rgba = pix.data_as_u8_slice().to_vec();
    for p in rgba.as_chunks_mut::<4>().0 {
        p[3] = 255;
    }
    png_of(&rgba, u32::from(view.width), u32::from(view.height))
}

fn parts(job: &Job, gpu: &Gpu, progress: &Progress, part: &Path) -> Result<Parts, String> {
    let info = job.video.info;
    let fps = info.fps();
    let frames = i64::from(info.frames);
    let keep = keep_of(&job.doc, frames);
    let (frame, out) = out_geometry(&job.doc, &job.video);
    // 1. The video, with only the marks that work on its pixels.
    let mut inner = job.clone();
    for o in &mut inner.doc.objects {
        if !burned(o) {
            o.hidden = true;
        }
    }
    let tmp = part.with_extension("video.part");
    let r = if untouched(&inner.doc, &inner.video) {
        copy_mp4(&inner.source, &tmp).map(|_| ())
    } else {
        // The inner export's progress shows as ours (the page itself is quick).
        let sub = Progress::default();
        std::thread::scope(|sc| {
            let h = sc.spawn(|| mp4(&inner, gpu, &sub, &tmp, true));
            while !h.is_finished() {
                if progress.cancel.load(Ordering::Relaxed) {
                    sub.cancel.store(true, Ordering::Relaxed);
                }
                progress.set(f64::from(sub.done.load(Ordering::Relaxed)) / 1000.0 * 0.9);
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            h.join()
                .map_err(|_| "the export thread failed".to_string())?
                .map(|_| ())
        })
    };
    if let Err(e) = r {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    let mp4_bytes = std::fs::read(&tmp).map_err(|e| e.to_string());
    let _ = std::fs::remove_file(&tmp);
    let mp4_bytes = mp4_bytes?;
    // 2. Each other mark as a picture of the whole frame, transparent around it.
    let mut layers = String::new();
    let mut renderer = Renderer::new();
    renderer.set_picture(false);
    let mut pix = Pixmap::new(1, 1);
    for o in job.doc.objects.iter().filter(|o| !o.hidden && !burned(o)) {
        progress.cancelled()?;
        let (a, b) = job
            .doc
            .timeline
            .as_ref()
            .and_then(|t| t.marks.get(&o.id).copied())
            .unwrap_or((0, frames));
        let times = out_intervals(&keep, a, b, fps);
        if times.is_empty() {
            continue;
        }
        let mut one = job.doc.clone();
        one.objects = vec![o.clone()];
        one.shown_frame = None;
        let view = View {
            scale: f64::from(out.0) / f64::from(frame.w.max(1)),
            origin: znimok_render::vello_cpu::kurbo::Point::new(
                f64::from(frame.x),
                f64::from(frame.y),
            ),
            width: out.0.min(65535) as u16,
            height: out.1.min(65535) as u16,
        };
        renderer.render(&one, view, &mut pix);
        let mut rgba = pix.data_as_u8_slice().to_vec();
        for p in rgba.as_chunks_mut::<4>().0 {
            let a = u32::from(p[3]);
            if a != 0 && a != 255 {
                for c in &mut p[..3] {
                    *c = ((u32::from(*c) * 255 + a / 2) / a).min(255) as u8;
                }
            }
        }
        let png = png_of(&rgba, u32::from(view.width), u32::from(view.height))?;
        let t: Vec<String> = times
            .iter()
            .map(|(x, y)| format!("{x:.3},{y:.3}"))
            .collect();
        layers.push_str("<img class=\"m\" alt=\"\" data-t=\"");
        layers.push_str(&t.join(";"));
        layers.push_str("\" src=\"data:image/png;base64,");
        layers.push_str(&base64(&png));
        layers.push_str("\">\n");
    }
    Ok(Parts {
        mp4: mp4_bytes,
        layers,
        keep,
        fps,
        frame,
        out,
    })
}

/// The estimated size of a GIF (LH: probe pairs of neighbouring output frames, the first whole,
/// the second as a difference): bytes.
pub fn estimate_gif(
    job: &Job,
    gpu: &Gpu,
    width: u32,
    fps: f64,
    dither: bool,
) -> Result<u64, String> {
    let info = job.video.info;
    let keep = keep_of(&job.doc, i64::from(info.frames));
    let (frame, out) = out_geometry(&job.doc, &job.video);
    let size = gif_size(IRect::new(0, 0, out.0 as i32, out.1 as i32), width);
    let wanted = gif_frames(&keep, info.fps(), fps);
    // Pairs: a probe and the GIF frame after it.
    let n = wanted.len();
    let k = n.min(6);
    let mut idx: Vec<usize> = Vec::new();
    for i in 0..k {
        let a = i * n / k.max(1);
        idx.push(a);
        idx.push((a + 1).min(n - 1));
    }
    let picks: Vec<i64> = idx.iter().map(|i| wanted[*i]).collect();
    let mut sorted = picks.clone();
    sorted.sort_unstable();
    let mut src = Frames::open(gpu, &job.source)?;
    let mut comp = Composer::new(&job.doc, frame, out);
    let mut px: Vec<(i64, Vec<u8>)> = Vec::new();
    compose_frames(
        &mut src,
        &mut comp,
        &sorted,
        size,
        &Progress::default(),
        (0.0, 1.0),
        |k, p| {
            px.push((sorted[k], p));
            Ok(())
        },
    )?;
    let get = |f: i64| px.iter().find(|(g, _)| *g == f).map(|(_, p)| p.as_slice());
    let refs: Vec<&[u8]> = px.iter().map(|(_, p)| p.as_slice()).collect();
    let palette = gifenc::build_palette(&refs, 256);
    let (mut header, mut full, mut diff) = (0, Vec::new(), Vec::new());
    for pair in picks.chunks(2) {
        let (Some(a), Some(b)) = (get(pair[0]), get(pair[1])) else {
            continue;
        };
        let (h, f, d) =
            gifenc::sizes_of_pair(a, b, size.0 as u16, size.1 as u16, &palette, dither)?;
        header = h;
        full.push(f);
        diff.push(d);
    }
    Ok(gifenc::estimate(header, &full, &diff, n))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nv12_of_white_black_and_red() {
        let mut rgba = vec![255u8; 4 * 4 * 4];
        for p in rgba.as_chunks_mut::<4>().0.iter_mut().skip(8) {
            p.copy_from_slice(&[255, 0, 0, 255]);
        }
        let nv = rgba_to_nv12(&rgba, 4, 4);
        assert_eq!(nv[0], 235, "white is 235");
        assert_eq!(nv[15], 63, "red's luma (BT.709)");
        assert_eq!((nv[16], nv[17]), (128, 128), "white has no chroma");
        assert_eq!((nv[20], nv[21]), (102, 240), "red's Cb, Cr");
    }

    #[test]
    fn gif_frames_follow_the_cuts_and_the_rate() {
        let keep = [KeepSeg::new(0, 30), KeepSeg::new(60, 90)];
        let f = gif_frames(&keep, 30.0, 10.0);
        assert_eq!(f.len(), 20);
        assert_eq!(&f[..3], &[0, 3, 6]);
        assert_eq!(f[10], 60, "after the cut");
        assert_eq!(probe_frames(&f).len(), 12);
    }

    #[test]
    fn bitrate_is_bounded() {
        let b = bitrate(10_000_000, 10.0, (1920, 1080), (960, 540), 30.0);
        assert_eq!(b, 2_500_000, "8 Mbit/s × 1.25 × a quarter of the area");
        assert_eq!(bitrate(10, 10.0, (100, 100), (100, 100), 30.0), 1_000_000);
    }
}
