//! Video documents (format 1.1, ZK-96; `docs/FORMAT.md`, "Video documents").
//!
//! A video document is an ordinary `.znimok` document whose `INFO` says *video*: the poster (the
//! first frame, `SRC `) carries the marks exactly as a screenshot does, so all the geometry of the
//! editor works without branches, and the video blocks add what a recording has on top — the
//! stream itself (`MP4 ` payload chunks, stored last), its parameters (`VINF`), the edit list
//! (`CUTS`), the frame and export size (`GEOM`), the audio tracks (`AUDI`), the mouse log
//! (`MOUS`) and the browser log (`DEVT`). The time span of a mark is a field of its `OBJ `
//! record (`vspn`).
//!
//! The encoded stream is never loaded into memory by the reader: [`crate::read_from`] seeks over
//! the payload and returns where it lies ([`Payload`]); [`PayloadReader`] plays it straight from
//! the file.

use std::collections::BTreeMap;
use std::io::{Read, Seek, SeekFrom};
use std::ops::Range;

use znimok_core::{IRect, ObjectId};

use crate::codec::{Reader, Tag, Writer};
use crate::{FormatError, Limits};

/// What a document is (`INFO`, last byte). Readers, the library, thumbnails and Quick Look tell
/// the two apart by this code, never by the file extension.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DocKind {
    #[default]
    Image,
    Video,
}

impl DocKind {
    pub(crate) fn code(self) -> u8 {
        match self {
            DocKind::Image => 0,
            DocKind::Video => 1,
        }
    }

    /// Unknown codes (a kind from a newer minor version) fall back to an image: the poster and
    /// its marks are still there.
    pub(crate) fn from_code(c: u8) -> Self {
        match c {
            1 => DocKind::Video,
            _ => DocKind::Image,
        }
    }
}

/// File extension (without the dot) for a kind of document. PLAN decision 28, confirmed for the
/// video stage: one extension for both — the kind lives inside the file. Should the owner choose
/// two, only this function changes; readers recognise documents by magic anyway.
pub fn extension_for(kind: DocKind) -> &'static str {
    match kind {
        DocKind::Image | DocKind::Video => crate::EXTENSION,
    }
}

/// Codec tags of `VINF` (the MP4 sample entry names). Unknown tags are kept as they are.
pub const CODEC_H264: Tag = *b"avc1";
pub const CODEC_HEVC: Tag = *b"hvc1";
pub const CODEC_AV1: Tag = *b"av01";

/// Largest `MP4 ` chunk the writer makes (the block length is a `u32`).
pub const PAYLOAD_CHUNK: u64 = 1 << 30;

/// Parameters of the encoded stream (`VINF`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VideoInfo {
    /// Size of the encoded frames — also the size of the poster and the coordinate space of the
    /// marks, the crop and the mouse log.
    pub width: u32,
    pub height: u32,
    /// Frame rate × 1000 (30 000, 60 000, 29 970…).
    pub fps_milli: u32,
    /// Frames in the stream, `N`: frame numbers of the edit list run over `[0, N)`.
    pub frames: u32,
    /// Duration, 100 ns.
    pub duration_hns: i64,
    /// Video codec, e.g. [`CODEC_H264`].
    pub codec: Tag,
}

impl VideoInfo {
    pub fn fps(&self) -> f64 {
        self.fps_milli as f64 / 1000.0
    }
}

/// One part of the edit list: frames `[a, b)`; `off` = cut out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Part {
    pub a: u32,
    pub b: u32,
    pub off: bool,
}

/// The edit list (`CUTS`, inventory §6.2): parts cover `[0, N)` without gaps; `in`/`out` are
/// separate trim handles that do not split parts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    pub parts: Vec<Part>,
    pub in_frame: u32,
    pub out_frame: u32,
}

impl Edit {
    /// As the document's timeline (ZK-144: the editor keeps the edits there, one undo history
    /// with the marks).
    pub fn to_timeline(&self) -> znimok_core::Timeline {
        znimok_core::Timeline {
            parts: self
                .parts
                .iter()
                .map(|p| znimok_core::TimelinePart {
                    a: p.a as i64,
                    b: p.b as i64,
                    off: p.off,
                })
                .collect(),
            in_point: self.in_frame as i64,
            out_point: self.out_frame as i64,
        }
    }

    /// From the document's timeline; `None` when a frame number does not fit.
    pub fn from_timeline(t: &znimok_core::Timeline) -> Option<Self> {
        let u = |v: i64| u32::try_from(v).ok();
        Some(Self {
            parts: t
                .parts
                .iter()
                .map(|p| {
                    Some(Part {
                        a: u(p.a)?,
                        b: u(p.b)?,
                        off: p.off,
                    })
                })
                .collect::<Option<Vec<_>>>()?,
            in_frame: u(t.in_point)?,
            out_frame: u(t.out_point)?,
        })
    }

    /// Nothing cut: one part `[0, N)`, handles at the ends.
    pub fn whole(frames: u32) -> Self {
        Self {
            parts: vec![Part {
                a: 0,
                b: frames,
                off: false,
            }],
            in_frame: 0,
            out_frame: frames,
        }
    }

    pub fn is_whole(&self, frames: u32) -> bool {
        *self == Self::whole(frames)
    }

    /// Parts cover `[0, frames)` contiguously, each non-empty; `in < out ≤ frames`.
    pub fn is_valid(&self, frames: u32) -> bool {
        let mut at = 0;
        for p in &self.parts {
            if p.a != at || p.b <= p.a {
                return false;
            }
            at = p.b;
        }
        !self.parts.is_empty()
            && at == frames
            && self.in_frame < self.out_frame
            && self.out_frame <= frames
    }

    /// What export keeps: parts not cut, within `[in, out)`, adjacent ones merged
    /// (`EvKeepSegs`; the input of `znimok_video::export::KeepSeg`).
    pub fn keep_segments(&self) -> Vec<Range<u32>> {
        let mut out: Vec<Range<u32>> = Vec::new();
        for p in self.parts.iter().filter(|p| !p.off) {
            let a = p.a.max(self.in_frame);
            let b = p.b.min(self.out_frame);
            if a >= b {
                continue;
            }
            match out.last_mut() {
                Some(last) if last.end == a => last.end = b,
                _ => out.push(a..b),
            }
        }
        out
    }
}

/// What a `MOUS` record is: a button going down or up, or a cursor position sample.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    /// No button: where the cursor was at that moment.
    Move,
}

fn button_code(b: MouseButton) -> u8 {
    match b {
        MouseButton::Left => 0,
        MouseButton::Right => 1,
        MouseButton::Middle => 2,
        MouseButton::Move => 15,
    }
}

/// Unlike other codes, an unknown button is not replaced by a default (a click of a button this
/// version does not know must not become a left click): the record is skipped.
fn button_from(c: u8) -> Option<MouseButton> {
    Some(match c {
        0 => MouseButton::Left,
        1 => MouseButton::Right,
        2 => MouseButton::Middle,
        15 => MouseButton::Move,
        _ => return None,
    })
}

/// One entry of the mouse log, in pixels of the video frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MouseEvent {
    /// Milliseconds of video time (pauses already removed).
    pub ms: i32,
    pub x: i32,
    pub y: i32,
    pub button: MouseButton,
    /// Pressed (for [`MouseButton::Move`]: whether the left button is held while moving).
    pub down: bool,
}

/// Size of one `MOUS` record as this version writes it.
const MOUSE_RECORD: u32 = 13;

/// One browser event (`DEVT`): the extension's JSON as it came, placed on video time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DevEvent {
    pub ms: i32,
    pub json: String,
}

/// The browser log of a recording (`DEVT`, inventory §5.5).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DevLog {
    /// Wall clock of the first frame, Unix ms UTC.
    pub wall0_ms: i64,
    /// In time order.
    pub events: Vec<DevEvent>,
}

/// Where an audio track was recorded from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum AudioSource {
    /// What the computer played (loopback / ScreenCaptureKit audio).
    #[default]
    System,
    Microphone,
}

fn source_code(s: AudioSource) -> u8 {
    match s {
        AudioSource::System => 0,
        AudioSource::Microphone => 1,
    }
}

fn source_from(c: u8) -> AudioSource {
    match c {
        1 => AudioSource::Microphone,
        _ => AudioSource::System,
    }
}

/// One audio track of the project (PLAN decision 16: separate tracks, mixed at export). Track
/// `i` of `AUDI` describes the `i`-th audio track of the MP4, in track order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioTrack {
    pub source: AudioSource,
    /// Device name as the user saw it ("Microphone (USB Audio)"); may be empty.
    pub label: String,
    /// Gain at export, % (0…200).
    pub volume: u8,
    /// Left out of the export mix.
    pub muted: bool,
    /// Shift against the video at export, ms (±60 000; positive = later).
    pub offset_ms: i32,
}

impl Default for AudioTrack {
    fn default() -> Self {
        Self {
            source: AudioSource::System,
            label: String::new(),
            volume: 100,
            muted: false,
            offset_ms: 0,
        }
    }
}

/// Everything a video document has on top of the poster document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Video {
    pub info: VideoInfo,
    pub edit: Edit,
    /// Export size (resize), `None` = the size of the frame. The frame itself (crop, in video
    /// pixels) is `Document::crop` of the poster document.
    pub out_size: Option<(u32, u32)>,
    pub audio: Vec<AudioTrack>,
    pub mouse: Vec<MouseEvent>,
    pub devlog: Option<DevLog>,
    /// Frames `[from, to)` in which a mark is shown, by object id; a mark without an entry is
    /// shown for the whole video.
    pub mark_spans: BTreeMap<ObjectId, (u32, u32)>,
}

impl Video {
    /// A fresh recording: nothing cut, no resize, no logs.
    pub fn new(info: VideoInfo) -> Self {
        Self {
            info,
            edit: Edit::whole(info.frames),
            out_size: None,
            audio: Vec::new(),
            mouse: Vec::new(),
            devlog: None,
            mark_spans: BTreeMap::new(),
        }
    }
}

/// Where the encoded stream lies: file ranges whose concatenation, in order, is the MP4.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Payload {
    pub ranges: Vec<Range<u64>>,
}

impl Payload {
    /// Length of the stream in bytes.
    pub fn len(&self) -> u64 {
        self.ranges.iter().map(|r| r.end - r.start).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The stream copied out of an in-memory file (tests, small files). `None` when a range lies
    /// outside `data`.
    pub fn bytes(&self, data: &[u8]) -> Option<Vec<u8>> {
        let mut out = Vec::new();
        for r in &self.ranges {
            let s = usize::try_from(r.start).ok()?;
            let e = usize::try_from(r.end).ok()?;
            out.extend_from_slice(data.get(s..e)?);
        }
        Some(out)
    }
}

/// The encoded stream read straight from the document file, without copying it anywhere: a
/// byte stream over the payload ranges for a player or decoder (Media Foundation
/// `IMFByteStream`, AVFoundation resource loader), or the source when a project is saved again.
pub struct PayloadReader<R> {
    inner: R,
    ranges: Vec<Range<u64>>,
    /// Offset of each range in the stream.
    starts: Vec<u64>,
    len: u64,
    pos: u64,
}

impl<R: Read + Seek> PayloadReader<R> {
    pub fn new(inner: R, payload: &Payload) -> Self {
        let ranges: Vec<Range<u64>> = payload
            .ranges
            .iter()
            .filter(|r| r.end > r.start)
            .cloned()
            .collect();
        let mut starts = Vec::with_capacity(ranges.len());
        let mut at = 0;
        for r in &ranges {
            starts.push(at);
            at += r.end - r.start;
        }
        Self {
            inner,
            ranges,
            starts,
            len: at,
            pos: 0,
        }
    }

    pub fn len(&self) -> u64 {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn into_inner(self) -> R {
        self.inner
    }
}

impl<R: Read + Seek> Read for PayloadReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.pos >= self.len || buf.is_empty() {
            return Ok(0);
        }
        // The range holding `pos`: the last one starting at or before it.
        let i = self.starts.partition_point(|&s| s <= self.pos) - 1;
        let r = &self.ranges[i];
        let within = self.pos - self.starts[i];
        let left = (r.end - r.start) - within;
        let n = (buf.len() as u64).min(left) as usize;
        self.inner.seek(SeekFrom::Start(r.start + within))?;
        let got = self.inner.read(&mut buf[..n])?;
        if got == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "the video stream ends before its recorded length",
            ));
        }
        self.pos += got as u64;
        Ok(got)
    }
}

impl<R: Read + Seek> Seek for PayloadReader<R> {
    fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
        let target = match to {
            SeekFrom::Start(p) => Some(p),
            SeekFrom::End(d) => self.len.checked_add_signed(d),
            SeekFrom::Current(d) => self.pos.checked_add_signed(d),
        };
        let p = target.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "seek before the start")
        })?;
        self.pos = p;
        Ok(p)
    }
}

// ---- writing --------------------------------------------------------------------------------

/// `VINF` flag: the file carries a browser log (`DEVT`) — for the icon of a «video with a
/// DevTools log», read from the head of the file (ZK-150).
pub(crate) const VINF_DEVTOOLS: u8 = 1;

pub(crate) fn write_vinf(w: &mut Writer, i: &VideoInfo, devtools: bool) {
    w.record(b"VINF", |w| {
        w.u32(i.width);
        w.u32(i.height);
        w.u32(i.fps_milli);
        w.u32(i.frames);
        w.i64(i.duration_hns);
        w.bytes(&i.codec);
        w.u8(if devtools { VINF_DEVTOOLS } else { 0 });
    });
}

/// The optional flags byte after the codec (absent in files written before it → none).
pub(crate) fn read_vinf_flags(b: &mut Reader<'_>) -> Result<u8, FormatError> {
    if b.is_empty() { Ok(0) } else { b.u8() }
}

/// The video blocks that follow the marks (everything but `VINF` and the payload).
pub(crate) fn write_blocks(w: &mut Writer, v: &Video, crop: Option<IRect>) {
    if crop.is_some() || v.out_size.is_some() {
        let (ow, oh) = v.out_size.unwrap_or((0, 0));
        w.record(b"GEOM", |w| {
            w.u32(ow);
            w.u32(oh);
            if let Some(c) = crop {
                let c = c.normalized();
                w.i32(c.x);
                w.i32(c.y);
                w.i32(c.w);
                w.i32(c.h);
            }
        });
    }
    if !v.edit.is_whole(v.info.frames) {
        w.record(b"CUTS", |w| {
            w.u32(v.edit.parts.len() as u32);
            for p in &v.edit.parts {
                w.u32(p.a);
                w.u32(p.b);
                w.u8(p.off as u8);
            }
            w.u32(v.edit.in_frame);
            w.u32(v.edit.out_frame);
        });
    }
    if !v.audio.is_empty() {
        w.record(b"AUDI", |w| {
            w.u32(v.audio.len() as u32);
            let d = AudioTrack::default();
            for t in &v.audio {
                w.record(b"TRK ", |w| {
                    w.record(b"srce", |w| w.u8(source_code(t.source)));
                    if !t.label.is_empty() {
                        w.record(b"labl", |w| w.str(&t.label));
                    }
                    if t.volume != d.volume {
                        w.record(b"volm", |w| w.u8(t.volume));
                    }
                    if t.muted {
                        w.record(b"mute", |w| w.u8(1));
                    }
                    if t.offset_ms != 0 {
                        w.record(b"offs", |w| w.i32(t.offset_ms));
                    }
                });
            }
        });
    }
    if !v.mouse.is_empty() {
        w.record(b"MOUS", |w| {
            w.u32(v.mouse.len() as u32);
            w.u32(MOUSE_RECORD);
            for e in &v.mouse {
                w.i32(e.ms);
                w.i32(e.x);
                w.i32(e.y);
                w.u8(button_code(e.button) | ((e.down as u8) << 4));
            }
        });
    }
    if let Some(d) = &v.devlog {
        w.record(b"DEVT", |w| {
            w.u8(1);
            w.i64(d.wall0_ms);
            w.u32(d.events.len() as u32);
            for e in &d.events {
                w.i32(e.ms);
                w.str(&e.json);
            }
        });
    }
}

// ---- reading --------------------------------------------------------------------------------

/// A count followed by records of at least `min_size` bytes each: refused before allocating when
/// it exceeds `max` or cannot fit in what is left of the block.
fn count(
    b: &mut Reader<'_>,
    max: usize,
    min_size: usize,
    what: &str,
) -> Result<usize, FormatError> {
    let n = b.u32()? as usize;
    if n > max {
        return Err(FormatError::Corrupt(format!("{n} {what} exceed the limit")));
    }
    if n.saturating_mul(min_size) > b.remaining() {
        return Err(FormatError::Corrupt(format!(
            "{n} {what} run past their block"
        )));
    }
    Ok(n)
}

pub(crate) fn read_vinf(b: &mut Reader<'_>, limits: &Limits) -> Result<VideoInfo, FormatError> {
    let i = VideoInfo {
        width: b.u32()?,
        height: b.u32()?,
        fps_milli: b.u32()?,
        frames: b.u32()?,
        duration_hns: b.i64()?,
        codec: b.take(4)?.try_into().expect("4 bytes"),
    };
    let side = 1..=limits.max_image_side;
    if !side.contains(&i.width) || !side.contains(&i.height) {
        return Err(FormatError::Corrupt(format!(
            "video {}×{} exceeds the limits",
            i.width, i.height
        )));
    }
    if !(1..=1_000_000).contains(&i.fps_milli) {
        return Err(FormatError::Corrupt(format!(
            "frame rate {}/1000 is out of range",
            i.fps_milli
        )));
    }
    if i.frames == 0 || i.frames > i32::MAX as u32 || i.duration_hns < 0 {
        return Err(FormatError::Corrupt(format!(
            "video of {} frames, {} × 100 ns",
            i.frames, i.duration_hns
        )));
    }
    Ok(i)
}

/// What `GEOM` holds: the export size and the frame (crop).
pub(crate) type Geom = (Option<(u32, u32)>, Option<IRect>);

/// `GEOM`: export size (0 × 0 = none) and, when cropped, the frame.
pub(crate) fn read_geom(b: &mut Reader<'_>, limits: &Limits) -> Result<Geom, FormatError> {
    let ow = b.u32()?;
    let oh = b.u32()?;
    let side = |v: u32| v.clamp(1, limits.max_image_side);
    let out = (ow != 0 || oh != 0).then(|| (side(ow), side(oh)));
    let crop = if b.remaining() >= 16 {
        Some(IRect::new(b.i32()?, b.i32()?, b.i32()?, b.i32()?).normalized())
    } else {
        None
    };
    Ok((out, crop))
}

/// `CUTS` as written; whether it fits the stream is checked once `VINF` is known.
pub(crate) fn read_cuts(b: &mut Reader<'_>, limits: &Limits) -> Result<Edit, FormatError> {
    let n = count(b, limits.max_cuts, 9, "parts")?;
    let parts = (0..n)
        .map(|_| {
            Ok(Part {
                a: b.u32()?,
                b: b.u32()?,
                off: b.bool()?,
            })
        })
        .collect::<Result<_, FormatError>>()?;
    Ok(Edit {
        parts,
        in_frame: b.u32()?,
        out_frame: b.u32()?,
    })
}

pub(crate) fn read_audi(
    b: &mut Reader<'_>,
    limits: &Limits,
) -> Result<Vec<AudioTrack>, FormatError> {
    let n = count(b, limits.max_audio_tracks, 8, "audio tracks")?;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let (t, mut r) = b.record()?;
        if &t != b"TRK " {
            return Err(FormatError::Corrupt(format!(
                "expected TRK, found {}",
                crate::codec::tag_str(&t)
            )));
        }
        let mut track = AudioTrack::default();
        while !r.is_empty() {
            let (tag, mut f) = r.record()?;
            match &tag {
                b"srce" => track.source = source_from(f.u8()?),
                b"labl" => track.label = f.str()?,
                b"volm" => track.volume = f.u8()?.min(200),
                b"mute" => track.muted = f.bool()?,
                b"offs" => track.offset_ms = f.i32()?.clamp(-60_000, 60_000),
                _ => {} // unknown field: skipped by length
            }
        }
        out.push(track);
    }
    Ok(out)
}

pub(crate) fn read_mous(
    b: &mut Reader<'_>,
    limits: &Limits,
) -> Result<Vec<MouseEvent>, FormatError> {
    let n = b.u32()? as usize;
    let size = b.u32()? as usize;
    if !(MOUSE_RECORD as usize..=256).contains(&size) {
        return Err(FormatError::Corrupt(format!(
            "mouse record of {size} bytes"
        )));
    }
    if n > limits.max_mouse_events {
        return Err(FormatError::Corrupt(format!(
            "{n} mouse events exceed the limit"
        )));
    }
    if n.saturating_mul(size) > b.remaining() {
        return Err(FormatError::Corrupt(
            "mouse events run past their block".into(),
        ));
    }
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let mut rec = Reader::new(b.take(size)?, limits);
        let (ms, x, y, code) = (rec.i32()?, rec.i32()?, rec.i32()?, rec.u8()?);
        // Bytes past the 13 known ones belong to a newer minor version: skipped.
        if let Some(button) = button_from(code & 0x0F) {
            out.push(MouseEvent {
                ms,
                x,
                y,
                button,
                down: code & 0x10 != 0,
            });
        }
    }
    Ok(out)
}

pub(crate) fn read_devt(b: &mut Reader<'_>, limits: &Limits) -> Result<DevLog, FormatError> {
    let _ver = b.u8()?;
    let wall0_ms = b.i64()?;
    let n = count(b, limits.max_dev_events, 8, "browser events")?;
    let events = (0..n)
        .map(|_| {
            Ok(DevEvent {
                ms: b.i32()?,
                json: b.str()?,
            })
        })
        .collect::<Result<_, FormatError>>()?;
    Ok(DevLog { wall0_ms, events })
}

/// The `vspn` field of an `OBJ `: `[from, to)`, `None` when empty.
pub(crate) fn read_span(f: &mut Reader<'_>) -> Result<Option<(u32, u32)>, FormatError> {
    let (a, b) = (f.u32()?, f.u32()?);
    Ok((a < b).then_some((a, b)))
}
