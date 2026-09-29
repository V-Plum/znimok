//! `.znimok` v1.1 — the document format (specification: `docs/FORMAT.md`).
//!
//! In short: 8-byte magic `ZNIMOK\x1A\n`, `u16 major`, `u16 minor`, then blocks
//! `tag[4] + u32 len + value` to the end of the file, little-endian throughout. Unknown blocks
//! and unknown object fields are skipped by length; a newer `major` is refused, `minor` is not
//! checked. Descriptive blocks come before the pixels so the library can read a record by
//! parsing only its head ([`peek`]). Objects are TLV records whose kind is a text tag — no enum
//! ordinals in the file. Fields are written only when they differ from the default.
//!
//! A document is a screenshot or a video ([`DocKind`], in `INFO`). A video document is the
//! poster document plus the video blocks and the encoded stream ([`video`], format 1.1).

mod codec;
mod image;
pub mod video;

#[doc(hidden)]
pub use codec::Writer;

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::Path;

use codec::{Reader, Tag, tag_str};
use znimok_core::{
    Align, BankId, Corners, CounterShape, Dash, Data, Document, Effect, Head, HideMode, IRect,
    Kind, Meta, Object, Raster, Recipe, Rgb, Style,
};

pub use image::{decode_png, encode_png};
pub use video::{
    AudioSource, AudioTrack, DevEvent, DevLog, DocKind, Edit, MouseButton, MouseEvent, Part,
    Payload, PayloadReader, Video, VideoInfo, extension_for,
};

pub const MAGIC: [u8; 8] = *b"ZNIMOK\x1A\n";
pub const MAJOR: u16 = 1;
/// The newest minor version this crate writes. A file is stamped with the lowest minor that
/// describes its content: screenshots stay 1.0 (byte-identical to older writers), video
/// documents are 1.1.
pub const MINOR: u16 = 1;
/// Minor version that introduced video documents.
pub const MINOR_VIDEO: u16 = 1;
/// File extension, without the dot.
pub const EXTENSION: &str = "znimok";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FormatError {
    /// Not a `.znimok` file (magic, or too short / too long).
    NotZnimok,
    /// Written by a newer, incompatible version.
    TooNew {
        major: u16,
        minor: u16,
    },
    /// Damaged or out of the reader's limits; the message says what.
    Corrupt(String),
    Io(String),
}

impl std::fmt::Display for FormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FormatError::NotZnimok => write!(f, "not a Znimok document"),
            FormatError::TooNew { major, minor } => write!(
                f,
                "made by a newer Znimok (format {major}.{minor}); update Znimok to open it"
            ),
            FormatError::Corrupt(m) => write!(f, "the document is damaged: {m}"),
            FormatError::Io(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for FormatError {}

/// Reader limits: a hostile or broken file fails cleanly instead of exhausting memory.
#[derive(Clone, Debug)]
pub struct Limits {
    pub max_file: usize,
    pub max_string: usize,
    pub max_objects: usize,
    pub max_points: usize,
    pub max_banks: usize,
    pub max_groups: usize,
    pub max_tags: usize,
    /// Video: parts of the edit list, mouse log entries, browser events, audio tracks, and
    /// `MP4 ` chunks of the encoded stream.
    pub max_cuts: usize,
    pub max_mouse_events: usize,
    pub max_dev_events: usize,
    pub max_audio_tracks: usize,
    pub max_payload_chunks: usize,
    pub max_image_side: u32,
    pub max_image_pixels: u64,
    /// Allocation budget of the PNG decoder per image.
    pub max_image_bytes: usize,
}

impl Limits {
    /// For a document's stored thumbnail: the app writes at most 320×240, so a file claiming
    /// more is not ours, and decoding it unbounded (in Explorer's, Finder's or the library's
    /// process) would be a memory bomb (ZK-113, ZK-121).
    pub fn thumbnail() -> Self {
        Self {
            max_image_side: 1024,
            max_image_pixels: 1 << 20,
            max_image_bytes: 16 << 20,
            ..Default::default()
        }
    }
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_file: 512 << 20,
            max_string: 1 << 20,
            max_objects: 100_000,
            max_points: 1 << 20,
            max_banks: 4096,
            max_groups: 10_000,
            max_tags: 1000,
            max_cuts: 100_000,
            max_mouse_events: 1 << 22,
            max_dev_events: 200_000,
            max_audio_tracks: 16,
            max_payload_chunks: 65_536,
            max_image_side: 32767,
            max_image_pixels: 1 << 28,
            max_image_bytes: 1 << 30,
        }
    }
}

pub struct WriteOptions {
    /// Written into `META` for diagnostics; readers ignore it.
    pub app_version: String,
    /// Composed thumbnail (≤ 320×240) for the library; the caller renders it.
    pub thumbnail: Option<Raster>,
    pub compression: png::Compression,
}

impl Default for WriteOptions {
    fn default() -> Self {
        Self {
            app_version: format!("Znimok {}", env!("CARGO_PKG_VERSION")),
            thumbnail: None,
            compression: png::Compression::Fast,
        }
    }
}

// ---- stable codes (never enum ordinals, §7 п.48) ---------------------------------------------

fn kind_tag(k: Kind) -> Tag {
    *match k {
        Kind::Rect => b"rect",
        Kind::Ellipse => b"elps",
        Kind::Line => b"line",
        Kind::Pen => b"pen ",
        Kind::Text => b"text",
        Kind::Hide => b"hide",
        Kind::Mark => b"mark",
        Kind::Counter => b"cnt ",
        Kind::Stamp => b"stmp",
        Kind::Image => b"img ",
    }
}

fn kind_from_tag(t: &Tag) -> Option<Kind> {
    Some(match t {
        b"rect" => Kind::Rect,
        b"elps" => Kind::Ellipse,
        b"line" => Kind::Line,
        b"pen " => Kind::Pen,
        b"text" => Kind::Text,
        b"hide" => Kind::Hide,
        b"mark" => Kind::Mark,
        b"cnt " => Kind::Counter,
        b"stmp" => Kind::Stamp,
        b"img " => Kind::Image,
        _ => return None,
    })
}

macro_rules! codes {
    ($to:ident, $from:ident, $t:ty { $($v:path = $c:literal),* $(,)? }) => {
        fn $to(v: $t) -> u8 { match v { $($v => $c),* } }
        /// Unknown codes (from a newer minor version) fall back to the default.
        fn $from(c: u8) -> $t { match c { $($c => $v,)* _ => <$t>::default() } }
    };
}

codes!(dash_code, dash_from, Dash { Dash::Solid = 0, Dash::Dashed = 1, Dash::DashDot = 2 });
codes!(corners_code, corners_from, Corners { Corners::Sharp = 0, Corners::Soft = 1, Corners::Round = 2 });
codes!(effect_code, effect_from, Effect { Effect::None = 0, Effect::Light = 1, Effect::Strong = 2 });
codes!(align_code, align_from, Align { Align::Left = 0, Align::Center = 1, Align::Right = 2 });
codes!(hide_code, hide_from, HideMode { HideMode::Blur = 0, HideMode::Pixelate = 1, HideMode::Plate = 2 });
codes!(head_code, head_from, Head { Head::None = 0, Head::Triangle = 1, Head::Chevron = 2, Head::Dot = 3 });
codes!(shape_code, shape_from, CounterShape { CounterShape::Circle = 0, CounterShape::RoundedBox = 1, CounterShape::Pin = 2 });

fn rgb_bytes(c: Rgb) -> [u8; 4] {
    [c.r, c.g, c.b, 255]
}

// ---- writing --------------------------------------------------------------------------------

/// Serialises the document. Only the current original and the banks that image marks use are
/// stored; bank numbers are renumbered in the file (the reader puts the original first).
pub fn write(doc: &Document, opts: &WriteOptions) -> Vec<u8> {
    write_doc(doc, opts, None).buf
}

/// Everything but the encoded stream; `video` makes it a video document (the payload chunks
/// are appended by the caller).
fn write_doc(doc: &Document, opts: &WriteOptions, video: Option<&Video>) -> Writer {
    let mut w = Writer::default();
    w.bytes(&MAGIC);
    w.u16(MAJOR);
    w.u16(if video.is_some() { MINOR_VIDEO } else { 0 });
    let kind = if video.is_some() {
        DocKind::Video
    } else {
        DocKind::Image
    };

    w.record(b"META", |w| {
        w.bytes(doc.id.as_bytes());
        w.i64(doc.meta.created_ms);
        w.str(&doc.name);
        w.str(&doc.meta.source);
        w.str(&opts.app_version);
    });
    let m = &doc.meta;
    if !(m.description.is_empty()
        && m.author.is_empty()
        && m.copyright.is_empty()
        && m.tags.is_empty())
    {
        w.record(b"DESC", |w| {
            w.u8(1);
            w.str(&m.description);
            w.str(&m.author);
            w.str(&m.copyright);
            w.u32(m.tags.len() as u32);
            for t in &m.tags {
                w.str(t);
            }
        });
    }
    let frame = doc.frame();
    // What export produces: for a video with a resize, the export size.
    let (fw, fh) = video
        .and_then(|v| v.out_size)
        .unwrap_or((frame.w as u32, frame.h as u32));
    w.record(b"INFO", |w| {
        w.u32(fw);
        w.u32(fh);
        w.u32(doc.objects.len() as u32);
        w.u8(kind.code());
    });
    if let Some(v) = video {
        video::write_vinf(&mut w, &v.info);
    }
    if let Some(t) = &opts.thumbnail {
        w.record(b"THMB", |w| {
            w.bytes(&encode_png(t, png::Compression::Balanced))
        });
    }
    w.record(b"SRC ", |w| {
        w.bytes(&encode_png(doc.source(), opts.compression))
    });

    // Banks used by image marks, renumbered 0.. in order of first use.
    let mut bank_map: BTreeMap<BankId, u32> = BTreeMap::new();
    for o in &doc.objects {
        if let Data::Image { bank } = o.data
            && (bank as usize) < doc.banks.len()
        {
            let next = bank_map.len() as u32;
            bank_map.entry(bank).or_insert(next);
        }
    }
    if !bank_map.is_empty() {
        let mut ordered: Vec<(u32, BankId)> = bank_map.iter().map(|(b, f)| (*f, *b)).collect();
        ordered.sort();
        w.record(b"BANK", |w| {
            w.u32(ordered.len() as u32);
            for (file_id, bank) in &ordered {
                w.u32(*file_id);
                let png = encode_png(&doc.banks[*bank as usize], opts.compression);
                w.u32(png.len() as u32);
                w.bytes(&png);
            }
        });
    }
    // A video has no tone/turn recipe, and its frame is in GEOM.
    if doc.recipe != Recipe::default() && video.is_none() {
        let r = doc.recipe;
        w.record(b"RCPE", |w| {
            w.f32(r.exposure);
            w.f32(r.gamma);
            w.i32(r.contrast);
            w.u8(r.rot_quarters % 4);
            w.u8(r.mirror as u8);
        });
    }
    if let Some(c) = doc.crop.filter(|_| video.is_none()) {
        let c = c.normalized();
        w.record(b"CROP", |w| {
            w.i32(c.x);
            w.i32(c.y);
            w.i32(c.w);
            w.i32(c.h);
        });
    }
    if doc.shot_scale != 1000 {
        w.record(b"SCAL", |w| w.u32(doc.shot_scale as u32));
    }
    w.record(b"OBJS", |w| {
        w.u32(doc.objects.len() as u32);
        for o in &doc.objects {
            let span = video.and_then(|v| v.mark_spans.get(&o.id).copied());
            write_object(w, o, &bank_map, span);
        }
    });
    if !doc.group_names.is_empty() {
        w.record(b"GRPN", |w| {
            w.u32(doc.group_names.len() as u32);
            for (g, n) in &doc.group_names {
                w.u32(*g);
                w.str(n);
            }
        });
    }
    if let Some(v) = video {
        video::write_blocks(&mut w, v, doc.crop);
    }
    w
}

fn write_object(
    w: &mut Writer,
    o: &Object,
    bank_map: &BTreeMap<BankId, u32>,
    span: Option<(u32, u32)>,
) {
    let d = Style::default();
    let s = &o.style;
    w.record(b"OBJ ", |w| {
        w.record(b"id  ", |w| w.u32(o.id));
        w.record(b"kind", |w| w.bytes(&kind_tag(o.kind())));
        w.record(b"rect", |w| {
            w.i32(o.rect.x);
            w.i32(o.rect.y);
            w.i32(o.rect.w);
            w.i32(o.rect.h);
        });
        if s.color != d.color {
            w.record(b"colr", |w| w.bytes(&rgb_bytes(s.color)));
        }
        if s.thick != d.thick {
            w.record(b"thck", |w| w.i32(s.thick));
        }
        if s.alpha != d.alpha {
            w.record(b"alph", |w| w.u8(s.alpha));
        }
        if s.no_main {
            w.record(b"nomn", |w| w.u8(1));
        }
        if let Some(c2) = s.color2 {
            w.record(b"col2", |w| w.bytes(&rgb_bytes(c2)));
        }
        if s.alpha2 != d.alpha2 {
            w.record(b"alp2", |w| w.u8(s.alpha2));
        }
        if s.dash != d.dash {
            w.record(b"dash", |w| w.u8(dash_code(s.dash)));
        }
        if s.corners != d.corners {
            w.record(b"crnr", |w| w.u8(corners_code(s.corners)));
        }
        if s.corner_px != d.corner_px {
            w.record(b"crpx", |w| w.i32(s.corner_px));
        }
        if s.shadow != d.shadow {
            w.record(b"shdw", |w| w.u8(effect_code(s.shadow)));
        }
        if s.glow != d.glow {
            w.record(b"glow", |w| w.u8(effect_code(s.glow)));
        }
        if o.rot != 0 {
            w.record(b"rot ", |w| w.u16(o.rot % 360));
        }
        if o.group != 0 {
            w.record(b"grp ", |w| w.u32(o.group));
        }
        if let Some(n) = &o.name {
            w.record(b"name", |w| w.str(n));
        }
        if o.hidden {
            w.record(b"hidn", |w| w.u8(1));
        }
        if let Some((a, b)) = span {
            w.record(b"vspn", |w| {
                w.u32(a);
                w.u32(b);
            });
        }
        match &o.data {
            Data::Rect | Data::Ellipse | Data::Mark => {}
            Data::Line {
                head_front,
                head_back,
                head_size,
            } => {
                w.record(b"hdf ", |w| w.u8(head_code(*head_front)));
                w.record(b"hdb ", |w| w.u8(head_code(*head_back)));
                w.record(b"hds ", |w| w.u8(*head_size));
            }
            Data::Pen {
                points,
                head_front,
                head_back,
            } => {
                w.record(b"pts ", |w| {
                    w.u32(points.len() as u32);
                    for (x, y) in points {
                        w.i32(*x);
                        w.i32(*y);
                    }
                });
                // Heads on a pen trail are optional: older readers skip the records.
                if *head_front != Head::None {
                    w.record(b"hdf ", |w| w.u8(head_code(*head_front)));
                }
                if *head_back != Head::None {
                    w.record(b"hdb ", |w| w.u8(head_code(*head_back)));
                }
            }
            Data::Text {
                text,
                size,
                bold,
                italic,
                align,
                box_w,
            } => {
                w.record(b"text", |w| w.str(text));
                w.record(b"size", |w| w.i32(*size));
                if *bold {
                    w.record(b"bold", |w| w.u8(1));
                }
                if *italic {
                    w.record(b"ital", |w| w.u8(1));
                }
                if *align != Align::Left {
                    w.record(b"algn", |w| w.u8(align_code(*align)));
                }
                if *box_w != 0 {
                    w.record(b"boxw", |w| w.i32(*box_w));
                }
            }
            Data::Hide { mode, strength } => {
                w.record(b"mode", |w| w.u8(hide_code(*mode)));
                w.record(b"strn", |w| w.u8(*strength));
            }
            Data::Counter {
                seq,
                group,
                start,
                shape,
            } => {
                w.record(b"cseq", |w| w.u32(*seq));
                w.record(b"cgrp", |w| w.u32(*group));
                w.record(b"cstr", |w| w.i32(*start));
                if *shape != CounterShape::Circle {
                    w.record(b"cshp", |w| w.u8(shape_code(*shape)));
                }
            }
            Data::Stamp { id } => w.record(b"stmp", |w| w.u32(*id)),
            Data::Image { bank } => {
                let file_id = bank_map.get(bank).copied().unwrap_or(u32::MAX);
                w.record(b"img ", |w| w.u32(file_id));
            }
        }
    });
}

/// Writes atomically: `<path>.part`, flushed to disk, then renamed over the target (§7 п.50).
pub fn save(path: &Path, doc: &Document, opts: &WriteOptions) -> Result<(), FormatError> {
    let bytes = write(doc, opts);
    save_atomic(path, |f| f.write_all(&bytes))
}

fn save_atomic(
    path: &Path,
    body: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>,
) -> Result<(), FormatError> {
    let mut part = path.as_os_str().to_owned();
    part.push(".part");
    let part = std::path::PathBuf::from(part);
    let io = |e: std::io::Error| FormatError::Io(format!("{}: {e}", path.display()));
    let written = (|| {
        let mut f = std::fs::File::create(&part)?;
        body(&mut f)?;
        f.sync_all()
    })();
    if let Err(e) = written {
        let _ = std::fs::remove_file(&part);
        return Err(io(e));
    }
    std::fs::rename(&part, path).map_err(|e| {
        let _ = std::fs::remove_file(&part);
        io(e)
    })
}

/// A video document in memory, the encoded stream `mp4` included (tests, small files; the app
/// streams with [`write_video_to`] / [`save_video`]).
///
/// The poster (`doc`'s current original) must be the first frame at the video's size; the
/// document's recipe is not written (video has none) and its crop goes to `GEOM`.
///
/// Panics when the poster is not the size of the video (see [`write_video_to`]).
pub fn write_video(doc: &Document, video: &Video, mp4: &[u8], opts: &WriteOptions) -> Vec<u8> {
    let mut out = Vec::new();
    if let Err(e) = write_video_to(&mut out, doc, video, mp4, mp4.len() as u64, opts) {
        panic!("{e}");
    }
    out
}

/// Streams a video document into `out`: the blocks, then `mp4_len` bytes of `mp4` as `MP4 `
/// chunks of at most [`video::PAYLOAD_CHUNK`] bytes. The stream is copied, never held in
/// memory whole. Fails when `mp4` ends before `mp4_len` bytes, and — before writing anything —
/// when the poster is not the size of the video (a reader would refuse the file).
pub fn write_video_to(
    out: &mut impl std::io::Write,
    doc: &Document,
    video: &Video,
    mut mp4: impl std::io::Read,
    mp4_len: u64,
    opts: &WriteOptions,
) -> Result<(), FormatError> {
    let io = |e: std::io::Error| FormatError::Io(e.to_string());
    let (pw, ph) = (doc.source().width, doc.source().height);
    if (pw, ph) != (video.info.width, video.info.height) {
        return Err(FormatError::Io(format!(
            "the poster {pw}×{ph} is not a frame of the {}×{} video",
            video.info.width, video.info.height
        )));
    }
    out.write_all(&write_doc(doc, opts, Some(video)).buf)
        .map_err(io)?;
    let mut left = mp4_len;
    while left > 0 {
        let n = left.min(video::PAYLOAD_CHUNK);
        out.write_all(b"MP4 ").map_err(io)?;
        out.write_all(&(n as u32).to_le_bytes()).map_err(io)?;
        let copied = std::io::copy(&mut std::io::Read::take(&mut mp4, n), out).map_err(io)?;
        if copied != n {
            return Err(FormatError::Io(format!(
                "the video stream ended after {} of {mp4_len} bytes",
                mp4_len - left + copied
            )));
        }
        left -= n;
    }
    Ok(())
}

/// Saves a video document atomically (like [`save`]). `mp4` is read to the end of `mp4_len`
/// and dropped before the rename, so it may be a [`PayloadReader`] over the very file being
/// replaced (saving a project again).
pub fn save_video(
    path: &Path,
    doc: &Document,
    video: &Video,
    mp4: impl std::io::Read,
    mp4_len: u64,
    opts: &WriteOptions,
) -> Result<(), FormatError> {
    let mut err = None;
    let r = save_atomic(path, |f| {
        let mut buf = std::io::BufWriter::with_capacity(1 << 20, f);
        match write_video_to(&mut buf, doc, video, mp4, mp4_len, opts) {
            Ok(()) => buf.flush(),
            Err(e) => {
                let msg = e.to_string();
                err = Some(e);
                Err(std::io::Error::other(msg))
            }
        }
    });
    match (r, err) {
        (Err(_), Some(e)) => Err(e),
        (r, _) => r,
    }
}

// ---- reading --------------------------------------------------------------------------------

fn header<'a>(data: &'a [u8], limits: &'a Limits) -> Result<Reader<'a>, FormatError> {
    if data.len() < 12 || data.len() > limits.max_file || data[..8] != MAGIC {
        return Err(FormatError::NotZnimok);
    }
    let mut r = Reader::new(data, limits);
    r.take(8)?;
    let major = r.u16()?;
    let minor = r.u16()?;
    if major > MAJOR {
        return Err(FormatError::TooNew { major, minor });
    }
    if major == 0 {
        return Err(FormatError::Corrupt("format version 0".into()));
    }
    Ok(r)
}

/// Whether the bytes start like a `.znimok` file (by magic, not by extension, §7 п.52).
pub fn is_znimok(data: &[u8]) -> bool {
    data.len() >= 8 && data[..8] == MAGIC
}

/// What the library needs from a record, read from the head of the file only.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Peek {
    pub id: Option<uuid::Uuid>,
    pub name: String,
    pub meta: Meta,
    pub app_version: String,
    /// Frame size (what export produces).
    pub width: u32,
    pub height: u32,
    pub object_count: u32,
    /// PNG bytes of the thumbnail, if the file has one.
    pub thumbnail_png: Option<Vec<u8>>,
    /// Screenshot or video — the library, thumbnails and Quick Look tell them apart by this.
    pub kind: DocKind,
    /// For a video: size, frame rate, frames and duration of the stream (`VINF`).
    pub video: Option<VideoInfo>,
}

/// Reads descriptive blocks up to the pixels (`SRC `), without decoding any image.
pub fn peek(data: &[u8]) -> Result<Peek, FormatError> {
    let limits = Limits::default();
    let mut r = header(data, &limits)?;
    let mut p = Peek::default();
    while !r.is_empty() {
        // Stop at the pixels before reading their length: the library may hand us only the
        // first kilobytes of a file.
        if matches!(r.peek_tag().as_ref(), Some(b"SRC " | b"OBJS" | b"BANK")) {
            break;
        }
        let (tag, mut b) = r.record()?;
        match &tag {
            b"META" => read_meta(&mut b, &mut p)?,
            b"DESC" => read_desc(&mut b, &mut p.meta, &limits)?,
            b"INFO" => {
                p.width = b.u32()?;
                p.height = b.u32()?;
                p.object_count = b.u32()?;
                p.kind = read_kind(&mut b)?;
            }
            b"VINF" => p.video = Some(video::read_vinf(&mut b, &limits)?),
            b"THMB" => p.thumbnail_png = Some(b.take(b.remaining())?.to_vec()),
            _ => {}
        }
    }
    if p.kind != DocKind::Video {
        p.video = None;
    }
    Ok(p)
}

/// The kind byte closing `INFO` (absent in a truncated block of an old writer → image).
fn read_kind(b: &mut Reader<'_>) -> Result<DocKind, FormatError> {
    Ok(if b.is_empty() {
        DocKind::Image
    } else {
        DocKind::from_code(b.u8()?)
    })
}

fn read_meta(b: &mut Reader<'_>, p: &mut Peek) -> Result<(), FormatError> {
    let id: [u8; 16] = b.take(16)?.try_into().expect("16 bytes");
    p.id = Some(uuid::Uuid::from_bytes(id));
    p.meta.created_ms = b.i64()?;
    p.name = b.str()?;
    p.meta.source = b.str()?;
    p.app_version = b.str()?;
    Ok(())
}

fn read_desc(b: &mut Reader<'_>, m: &mut Meta, limits: &Limits) -> Result<(), FormatError> {
    let _ver = b.u8()?;
    m.description = b.str()?;
    m.author = b.str()?;
    m.copyright = b.str()?;
    let n = b.u32()? as usize;
    if n > limits.max_tags {
        return Err(FormatError::Corrupt(format!("{n} tags exceed the limit")));
    }
    m.tags = (0..n).map(|_| b.str()).collect::<Result<_, _>>()?;
    Ok(())
}

/// Parses a whole document. It is built only after the entire file parsed successfully.
///
/// A video document reads as its poster document (the first frame with the marks); use
/// [`read_any`] or [`open`] to get the video too — saving the result of `read` with [`save`]
/// would turn a video into a screenshot.
pub fn read(data: &[u8]) -> Result<Document, FormatError> {
    read_with_limits(data, &Limits::default())
}

pub fn read_with_limits(data: &[u8], limits: &Limits) -> Result<Document, FormatError> {
    parse(data, limits).map(|p| p.doc)
}

/// A document of either kind.
// One value per opened file: the size difference of the variants does not matter.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug)]
pub enum Loaded {
    Image(Document),
    Video(VideoDocument),
}

impl Loaded {
    pub fn kind(&self) -> DocKind {
        match self {
            Loaded::Image(_) => DocKind::Image,
            Loaded::Video(_) => DocKind::Video,
        }
    }

    /// The document with the marks (for a video, its poster).
    pub fn document(&self) -> &Document {
        match self {
            Loaded::Image(d) => d,
            Loaded::Video(v) => &v.doc,
        }
    }
}

/// A video document: the poster document with the marks, the video blocks, and where the
/// encoded stream lies in the file.
#[derive(Clone, Debug)]
pub struct VideoDocument {
    pub doc: Document,
    pub video: Video,
    /// Offsets are in the bytes given to [`read_any`], or in the file given to [`open`] /
    /// [`read_from`].
    pub payload: Payload,
}

/// Parses a document of either kind from memory.
pub fn read_any(data: &[u8]) -> Result<Loaded, FormatError> {
    read_any_with_limits(data, &Limits::default())
}

pub fn read_any_with_limits(data: &[u8], limits: &Limits) -> Result<Loaded, FormatError> {
    parse(data, limits).map(Parsed::into_loaded)
}

/// Opens a document file of either kind. The encoded stream of a video is not read: the reader
/// seeks over it, so a recording of any length opens with memory bounded by the limits.
pub fn open(path: &Path) -> Result<Loaded, FormatError> {
    let f = std::fs::File::open(path)
        .map_err(|e| FormatError::Io(format!("{}: {e}", path.display())))?;
    read_from(&mut std::io::BufReader::new(f), &Limits::default())
}

/// Like [`open`], from any seekable source. Everything but the `MP4 ` chunks is read into memory
/// (at most `limits.max_file` bytes) and parsed as usual; the chunks are only located.
pub fn read_from<R: std::io::Read + std::io::Seek>(
    r: &mut R,
    limits: &Limits,
) -> Result<Loaded, FormatError> {
    use std::io::SeekFrom;
    let io = |e: std::io::Error| FormatError::Io(e.to_string());
    let file_len = r.seek(SeekFrom::End(0)).map_err(io)?;
    r.seek(SeekFrom::Start(0)).map_err(io)?;
    let mut head = [0u8; 12];
    if file_len < 12 {
        return Err(FormatError::NotZnimok);
    }
    r.read_exact(&mut head).map_err(io)?;
    if head[..8] != MAGIC {
        return Err(FormatError::NotZnimok);
    }
    let mut buf = head.to_vec();
    let mut ranges: Vec<std::ops::Range<u64>> = Vec::new();
    let mut pos = 12u64;
    while pos < file_len {
        if file_len - pos < 8 {
            return Err(FormatError::Corrupt(format!(
                "unexpected end at byte {pos} (need 8)"
            )));
        }
        let mut th = [0u8; 8];
        r.read_exact(&mut th).map_err(io)?;
        pos += 8;
        let tag: Tag = th[..4].try_into().expect("4 bytes");
        let len = u32::from_le_bytes(th[4..].try_into().expect("4 bytes")) as u64;
        if len > file_len - pos {
            return Err(FormatError::Corrupt(format!(
                "record {} runs past the end",
                tag_str(&tag)
            )));
        }
        if &tag == b"MP4 " {
            if ranges.len() >= limits.max_payload_chunks {
                return Err(FormatError::Corrupt(
                    "video stream chunks exceed the limit".into(),
                ));
            }
            ranges.push(pos..pos + len);
            r.seek(SeekFrom::Current(len as i64)).map_err(io)?;
        } else {
            if buf.len() as u64 + 8 + len > limits.max_file as u64 {
                return Err(FormatError::Corrupt(
                    "the document without its video stream exceeds the limit".into(),
                ));
            }
            buf.extend_from_slice(&th);
            let at = buf.len();
            buf.resize(at + len as usize, 0);
            r.read_exact(&mut buf[at..]).map_err(io)?;
        }
        pos += len;
    }
    parse_blocks(&buf, limits, ranges).map(Parsed::into_loaded)
}

/// Result of parsing: the document, and for a video its blocks and the stream's location.
struct Parsed {
    doc: Document,
    video: Option<(Video, Payload)>,
}

impl Parsed {
    fn into_loaded(self) -> Loaded {
        match self.video {
            None => Loaded::Image(self.doc),
            Some((video, payload)) => Loaded::Video(VideoDocument {
                doc: self.doc,
                video,
                payload,
            }),
        }
    }
}

fn parse(data: &[u8], limits: &Limits) -> Result<Parsed, FormatError> {
    parse_blocks(data, limits, Vec::new())
}

/// The shared parser. `outside` are `MP4 ` chunks already located outside `data` (by
/// [`read_from`]); chunks inside `data` are added to them in file order.
fn parse_blocks(
    data: &[u8],
    limits: &Limits,
    outside: Vec<std::ops::Range<u64>>,
) -> Result<Parsed, FormatError> {
    let mut r = header(data, limits)?;
    let mut peek = Peek::default();
    let mut src: Option<Raster> = None;
    let mut banks: Vec<(u32, Raster)> = Vec::new();
    let mut recipe = Recipe::default();
    let mut crop = None;
    let mut scale = 1000u16;
    let mut objects: Vec<(Object, Option<(u32, u32)>)> = Vec::new();
    let mut group_names = BTreeMap::new();
    // Video blocks, applied once the kind is known.
    let mut vinf = None;
    let mut geom = None;
    let mut cuts = None;
    let mut audio = Vec::new();
    let mut mouse = Vec::new();
    let mut devlog = None;
    let mut payload = outside;

    while !r.is_empty() {
        let at = r.at as u64;
        let (tag, mut b) = r.record()?;
        match &tag {
            b"META" => read_meta(&mut b, &mut peek)?,
            b"DESC" => read_desc(&mut b, &mut peek.meta, limits)?,
            b"INFO" => {
                if b.remaining() >= 12 {
                    b.take(12)?;
                    peek.kind = read_kind(&mut b)?;
                }
            }
            b"SRC " => src = Some(decode_png(b.take(b.remaining())?, limits)?),
            b"BANK" => {
                let n = b.u32()? as usize;
                if n > limits.max_banks {
                    return Err(FormatError::Corrupt(format!("{n} images exceed the limit")));
                }
                for _ in 0..n {
                    let id = b.u32()?;
                    let len = b.u32()? as usize;
                    let png = b.take(len)?;
                    banks.push((id, decode_png(png, limits)?));
                }
            }
            b"RCPE" => {
                recipe = Recipe {
                    exposure: b.f32()?,
                    gamma: b.f32()?,
                    contrast: b.i32()?,
                    rot_quarters: b.u8()? % 4,
                    mirror: b.bool()?,
                }
                .clamped();
            }
            b"CROP" => {
                let c = IRect::new(b.i32()?, b.i32()?, b.i32()?, b.i32()?).normalized();
                crop = Some(c);
            }
            b"SCAL" => {
                let v = b.u32()?;
                if !(250..=8000).contains(&v) {
                    return Err(FormatError::Corrupt(format!(
                        "monitor scale {v} is out of range"
                    )));
                }
                scale = v as u16;
            }
            b"OBJS" => {
                let n = b.u32()? as usize;
                if n > limits.max_objects {
                    return Err(FormatError::Corrupt(format!("{n} marks exceed the limit")));
                }
                for _ in 0..n {
                    let (t, mut ob) = b.record()?;
                    if &t != b"OBJ " {
                        return Err(FormatError::Corrupt(format!(
                            "expected OBJ, found {}",
                            tag_str(&t)
                        )));
                    }
                    if let Some(o) = read_object(&mut ob, limits)? {
                        objects.push(o);
                    }
                }
            }
            b"GRPN" => {
                let n = b.u32()? as usize;
                if n > limits.max_groups {
                    return Err(FormatError::Corrupt(format!(
                        "{n} group names exceed the limit"
                    )));
                }
                for _ in 0..n {
                    let g = b.u32()?;
                    group_names.insert(g, b.str()?);
                }
            }
            b"VINF" => vinf = Some(video::read_vinf(&mut b, limits)?),
            b"GEOM" => geom = Some(video::read_geom(&mut b, limits)?),
            b"CUTS" => cuts = Some(video::read_cuts(&mut b, limits)?),
            b"AUDI" => audio = video::read_audi(&mut b, limits)?,
            b"MOUS" => mouse = video::read_mous(&mut b, limits)?,
            b"DEVT" => devlog = Some(video::read_devt(&mut b, limits)?),
            b"MP4 " => {
                if payload.len() >= limits.max_payload_chunks {
                    return Err(FormatError::Corrupt(
                        "video stream chunks exceed the limit".into(),
                    ));
                }
                let start = at + 8;
                payload.push(start..start + b.remaining() as u64);
            }
            _ => {} // THMB and unknown blocks
        }
    }

    let src = src.ok_or_else(|| FormatError::Corrupt("no picture (SRC block)".into()))?;
    let is_video = peek.kind == DocKind::Video;
    let video_info = if is_video {
        let info =
            vinf.ok_or_else(|| FormatError::Corrupt("a video without its VINF block".into()))?;
        if (src.width, src.height) != (info.width, info.height) {
            return Err(FormatError::Corrupt(format!(
                "poster {}×{} is not the size of the video {}×{}",
                src.width, src.height, info.width, info.height
            )));
        }
        if payload.iter().all(|r| r.end == r.start) {
            return Err(FormatError::Corrupt("a video without its stream".into()));
        }
        // A video has no recipe; its frame comes from GEOM.
        recipe = Recipe::default();
        crop = geom.and_then(|g| g.1);
        Some(info)
    } else {
        None
    };

    let mut doc = Document::from_raster(peek.name, src);
    if let Some(id) = peek.id {
        doc.id = id;
    }
    doc.meta = peek.meta;
    // File bank ids → document bank numbers (the original is 0).
    let mut map = BTreeMap::new();
    for (file_id, raster) in banks {
        let b = doc.add_bank(raster);
        map.insert(file_id, b);
    }
    doc.recipe = recipe;
    doc.shot_scale = scale;
    let mut spans = BTreeMap::new();
    for (mut o, span) in objects {
        if let Data::Image { bank } = &mut o.data {
            match map.get(bank) {
                Some(b) => *bank = *b,
                None => continue, // the image is missing: drop the mark rather than fail
            }
        }
        let i = doc.push(o);
        if let Some(s) = span {
            spans.insert(doc.objects[i].id, s);
        }
    }
    doc.group_names = group_names;
    let used: Vec<_> = doc.objects.iter().map(|o| o.group).collect();
    doc.group_names.retain(|g, _| used.contains(g));
    doc.compact_groups();
    let (w, h) = doc.image_size();
    doc.crop = crop.and_then(|c: IRect| {
        let x0 = c.x.clamp(0, w as i32);
        let y0 = c.y.clamp(0, h as i32);
        let x1 = c.right().clamp(0, w as i32);
        let y1 = c.bottom().clamp(0, h as i32);
        (x1 - x0 >= 1 && y1 - y0 >= 1).then(|| IRect::new(x0, y0, x1 - x0, y1 - y0))
    });

    let video = video_info.map(|info| {
        // An edit list that does not fit the stream is dropped, not fatal: the video still
        // opens, uncut.
        let edit = cuts
            .filter(|e| e.is_valid(info.frames))
            .unwrap_or_else(|| Edit::whole(info.frames));
        let v = Video {
            info,
            edit,
            out_size: geom.and_then(|g| g.0),
            audio,
            mouse,
            devlog,
            mark_spans: spans,
        };
        (v, Payload { ranges: payload })
    });
    Ok(Parsed { doc, video })
}

/// Result of [`read_object`]: the mark and its time span in a video.
type ReadObject = (Object, Option<(u32, u32)>);

/// One mark; `None` for kinds this version does not know (written by a newer minor version).
fn read_object(b: &mut Reader<'_>, limits: &Limits) -> Result<Option<ReadObject>, FormatError> {
    let mut id = 0;
    let mut kind = None;
    let mut unknown_kind = false;
    let mut rect = IRect::default();
    let mut s = Style::default();
    let (mut rot, mut group, mut name, mut hidden) = (0u16, 0u32, None, false);
    let (mut hdf, mut hdb, mut hds) = (Head::None, Head::None, 1u8);
    let mut points = Vec::new();
    let (mut text, mut size, mut bold, mut italic, mut align, mut box_w) =
        (String::new(), 24, false, false, Align::Left, 0);
    let (mut mode, mut strength) = (HideMode::Blur, 50u8);
    let (mut cseq, mut cgrp, mut cstr, mut cshp) = (0u32, 1u32, 1i32, CounterShape::Circle);
    let mut stamp = 0u32;
    let mut img = u32::MAX;
    let mut span = None;
    let rgb = |f: &mut Reader<'_>| -> Result<Rgb, FormatError> {
        let v = f.take(4)?;
        Ok(Rgb::new(v[0], v[1], v[2]))
    };

    while !b.is_empty() {
        let (tag, mut f) = b.record()?;
        match &tag {
            b"id  " => id = f.u32()?,
            b"kind" => {
                let t: Tag = f.take(4)?.try_into().expect("4 bytes");
                kind = kind_from_tag(&t);
                unknown_kind = kind.is_none();
            }
            b"rect" => rect = IRect::new(f.i32()?, f.i32()?, f.i32()?, f.i32()?),
            b"colr" => s.color = rgb(&mut f)?,
            b"thck" => s.thick = f.i32()?.clamp(1, 10_000),
            b"alph" => s.alpha = f.u8()?.clamp(10, 100),
            b"nomn" => s.no_main = f.bool()?,
            b"col2" => s.color2 = Some(rgb(&mut f)?),
            b"alp2" => s.alpha2 = f.u8()?.clamp(10, 100),
            b"dash" => s.dash = dash_from(f.u8()?),
            b"crnr" => s.corners = corners_from(f.u8()?),
            b"crpx" => s.corner_px = f.i32()?.clamp(0, 10_000),
            b"shdw" => s.shadow = effect_from(f.u8()?),
            b"glow" => s.glow = effect_from(f.u8()?),
            b"rot " => rot = f.u16()? % 360,
            b"grp " => group = f.u32()?,
            b"name" => name = Some(f.str()?).filter(|n: &String| !n.is_empty()),
            b"hidn" => hidden = f.bool()?,
            b"vspn" => span = video::read_span(&mut f)?,
            b"hdf " => hdf = head_from(f.u8()?),
            b"hdb " => hdb = head_from(f.u8()?),
            b"hds " => hds = f.u8()?.min(2),
            b"pts " => {
                let n = f.u32()? as usize;
                if n > limits.max_points {
                    return Err(FormatError::Corrupt(format!(
                        "{n} pen points exceed the limit"
                    )));
                }
                if n * 8 > f.remaining() {
                    return Err(FormatError::Corrupt(
                        "pen points run past their field".into(),
                    ));
                }
                points = (0..n)
                    .map(|_| Ok((f.i32()?, f.i32()?)))
                    .collect::<Result<_, FormatError>>()?;
            }
            b"text" => text = f.str()?,
            b"size" => size = f.i32()?.clamp(1, 1600),
            b"bold" => bold = f.bool()?,
            b"ital" => italic = f.bool()?,
            b"algn" => align = align_from(f.u8()?),
            b"boxw" => box_w = f.i32()?.clamp(0, 100_000),
            b"mode" => mode = hide_from(f.u8()?),
            b"strn" => strength = f.u8()?.clamp(10, 100),
            b"cseq" => cseq = f.u32()?,
            b"cgrp" => cgrp = f.u32()?,
            b"cstr" => cstr = f.i32()?,
            b"cshp" => cshp = shape_from(f.u8()?),
            b"stmp" => stamp = f.u32()?,
            b"img " => img = f.u32()?,
            _ => {} // unknown field: skipped by length
        }
    }
    if unknown_kind {
        return Ok(None);
    }
    let kind = kind.ok_or_else(|| FormatError::Corrupt("a mark without a kind".into()))?;
    let data = match kind {
        Kind::Rect => Data::Rect,
        Kind::Ellipse => Data::Ellipse,
        Kind::Mark => Data::Mark,
        Kind::Line => Data::Line {
            head_front: hdf,
            head_back: hdb,
            head_size: hds,
        },
        Kind::Pen => Data::Pen {
            points,
            head_front: hdf,
            head_back: hdb,
        },
        Kind::Text => Data::Text {
            text,
            size,
            bold,
            italic,
            align,
            box_w,
        },
        Kind::Hide => Data::Hide { mode, strength },
        Kind::Counter => Data::Counter {
            seq: cseq,
            group: cgrp,
            start: cstr,
            shape: cshp,
        },
        Kind::Stamp => Data::Stamp { id: stamp },
        Kind::Image => Data::Image { bank: img },
    };
    Ok(Some((
        Object {
            id,
            rect,
            style: s,
            rot,
            group,
            name,
            hidden,
            data,
        },
        span,
    )))
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod video_tests;
