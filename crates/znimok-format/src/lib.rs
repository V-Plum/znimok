//! `.znimok` v1.0 — the document format (specification: `docs/FORMAT.md`).
//!
//! In short: 8-byte magic `ZNIMOK\x1A\n`, `u16 major`, `u16 minor`, then blocks
//! `tag[4] + u32 len + value` to the end of the file, little-endian throughout. Unknown blocks
//! and unknown object fields are skipped by length; a newer `major` is refused, `minor` is not
//! checked. Descriptive blocks come before the pixels so the library can read a record by
//! parsing only its head ([`peek`]). Objects are TLV records whose kind is a text tag — no enum
//! ordinals in the file. Fields are written only when they differ from the default.

mod codec;
mod image;

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

pub const MAGIC: [u8; 8] = *b"ZNIMOK\x1A\n";
pub const MAJOR: u16 = 1;
pub const MINOR: u16 = 0;
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
    pub max_image_side: u32,
    pub max_image_pixels: u64,
    /// Allocation budget of the PNG decoder per image.
    pub max_image_bytes: usize,
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
    let mut w = Writer::default();
    w.bytes(&MAGIC);
    w.u16(MAJOR);
    w.u16(MINOR);

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
    w.record(b"INFO", |w| {
        w.u32(frame.w as u32);
        w.u32(frame.h as u32);
        w.u32(doc.objects.len() as u32);
        w.u8(0); // document kind: 0 = image (v2 adds video)
    });
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
    if doc.recipe != Recipe::default() {
        let r = doc.recipe;
        w.record(b"RCPE", |w| {
            w.f32(r.exposure);
            w.f32(r.gamma);
            w.i32(r.contrast);
            w.u8(r.rot_quarters % 4);
            w.u8(r.mirror as u8);
        });
    }
    if let Some(c) = doc.crop {
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
            write_object(w, o, &bank_map);
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
    w.buf
}

fn write_object(w: &mut Writer, o: &Object, bank_map: &BTreeMap<BankId, u32>) {
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
            Data::Pen { points } => w.record(b"pts ", |w| {
                w.u32(points.len() as u32);
                for (x, y) in points {
                    w.i32(*x);
                    w.i32(*y);
                }
            }),
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
    let mut part = path.as_os_str().to_owned();
    part.push(".part");
    let part = std::path::PathBuf::from(part);
    let io = |e: std::io::Error| FormatError::Io(format!("{}: {e}", path.display()));
    {
        let mut f = std::fs::File::create(&part).map_err(io)?;
        f.write_all(&bytes).map_err(io)?;
        f.sync_all().map_err(io)?;
    }
    std::fs::rename(&part, path).map_err(|e| {
        let _ = std::fs::remove_file(&part);
        io(e)
    })
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
            }
            b"THMB" => p.thumbnail_png = Some(b.take(b.remaining())?.to_vec()),
            _ => {}
        }
    }
    Ok(p)
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
pub fn read(data: &[u8]) -> Result<Document, FormatError> {
    read_with_limits(data, &Limits::default())
}

pub fn read_with_limits(data: &[u8], limits: &Limits) -> Result<Document, FormatError> {
    let mut r = header(data, limits)?;
    let mut peek = Peek::default();
    let mut src: Option<Raster> = None;
    let mut banks: Vec<(u32, Raster)> = Vec::new();
    let mut recipe = Recipe::default();
    let mut crop = None;
    let mut scale = 1000u16;
    let mut objects: Vec<Object> = Vec::new();
    let mut group_names = BTreeMap::new();

    while !r.is_empty() {
        let (tag, mut b) = r.record()?;
        match &tag {
            b"META" => read_meta(&mut b, &mut peek)?,
            b"DESC" => read_desc(&mut b, &mut peek.meta, limits)?,
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
            _ => {} // INFO, THMB and unknown blocks
        }
    }

    let src = src.ok_or_else(|| FormatError::Corrupt("no picture (SRC block)".into()))?;
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
    for mut o in objects {
        if let Data::Image { bank } = &mut o.data {
            match map.get(bank) {
                Some(b) => *bank = *b,
                None => continue, // the image is missing: drop the mark rather than fail
            }
        }
        doc.push(o);
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
    Ok(doc)
}

/// One mark; `None` for kinds this version does not know (written by a newer minor version).
fn read_object(b: &mut Reader<'_>, limits: &Limits) -> Result<Option<Object>, FormatError> {
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
        Kind::Pen => Data::Pen { points },
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
    Ok(Some(Object {
        id,
        rect,
        style: s,
        rot,
        group,
        name,
        hidden,
        data,
    }))
}

#[cfg(test)]
mod tests;
