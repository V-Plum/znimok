//! A bounded MP4 (ISO BMFF) reader and the container checks of the LH harness (ZK-99).
//!
//! Sources:
//! - `mp4boxes.py` — walk `moov/trak/mdia/minf/stbl/edts`, `mdhd` timescale, `stss` key frames
//!   ("first: (1, 31, 61…)"), `stts`, `stsz` sample count;
//! - the summary line of `vidcheck.cpp` (`codec=H264 WxH fps=N/D duration=…`,
//!   `frames= first= last= maxGap= keyframes=`) — here from the sample tables instead of a decoder;
//! - `evprobe.cpp` `Check` — audio length within 50 ms of the video, AAC priming searched ±30 ms;
//! - `inventory_video.md` §7: item 1 (MP4 counts time by sample DURATIONS), item 2 (composition
//!   offsets without an edit list shift the video by a frame), item 3 (GOP = fps: `stss` 1, 31,
//!   61…), item 7 (even sides), item 8 (no `moov` = unfinalised file), item 12 (chunk offsets must
//!   stay valid when metadata is appended), item 18 (a sample may cover several frames).
//!
//! Only `moov` is read into memory (capped by [`MAX_MOOV`]); `mdat` is skipped by seeking, so a
//! multi-gigabyte recording costs a few reads. Every table size is validated against the bytes of
//! its box before anything is allocated; sample tables are never expanded per sample — all
//! checks run over run-length entries.

use super::read::Cursor;
use super::{Check, CheckError, CheckResult};
use std::fmt;
use std::io::{Read, Seek, SeekFrom};

/// Largest `moov` payload read into memory (an hour at 60 fps with audio is ~5 MB).
pub const MAX_MOOV: u64 = 64 << 20;
/// Top-level boxes before the reader gives up (a real file has 3–6).
pub const MAX_TOP_BOXES: usize = 4096;
/// Children of one container box.
pub const MAX_CHILDREN: usize = 1 << 16;
/// Tracks in `moov`.
pub const MAX_TRACKS: usize = 64;

/// Audio length against video length, seconds (`evprobe` `Check`: `fabs(aDur − expect) > 0.05`).
pub const AUDIO_DRIFT_MAX_S: f64 = 0.05;
/// First audio sample against zero, seconds: `evprobe` searches the AAC priming offset in
/// ±1440 frames at 48 kHz = ±30 ms; more than that and its level check would fail.
pub const AUDIO_START_MAX_S: f64 = 0.030;
/// First video frame against zero, seconds (§7 item 2: B-frames without an edit list put the
/// first frame at 33 ms).
pub const FIRST_PTS_TOL_S: f64 = 0.001;
/// Duration against the expected one, seconds (`vid_test.ps1`: "тривалість відповідає реальному
/// часу (±0.6 с)").
pub const DURATION_TOL_S: f64 = 0.6;

type FourCC = [u8; 4];

fn cc(f: &FourCC) -> String {
    f.iter()
        .map(|&b| {
            if b.is_ascii_graphic() || b == b' ' {
                b as char
            } else {
                '?'
            }
        })
        .collect()
}

/// A top-level box: where it is in the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TopBox {
    pub kind: FourCC,
    pub offset: u64,
    pub header: u64,
    pub size: u64,
}

impl TopBox {
    pub fn payload(&self) -> (u64, u64) {
        (self.offset + self.header, self.offset + self.size)
    }
}

/// One `elst` entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Edit {
    /// Segment duration, movie timescale.
    pub duration: u64,
    /// Start in the media, media timescale; −1 = empty edit (a delay).
    pub media_time: i64,
    /// Rate, 16.16.
    pub rate: i32,
}

/// Frame rate as a fraction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fps {
    pub num: u32,
    pub den: u32,
}

impl Fps {
    pub const fn new(num: u32, den: u32) -> Self {
        Self { num, den }
    }
    pub fn as_f64(&self) -> f64 {
        if self.den == 0 {
            0.0
        } else {
            self.num as f64 / self.den as f64
        }
    }
}

impl fmt::Display for Fps {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.num, self.den)
    }
}

/// A track as the sample tables describe it (run-length, not expanded).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Track {
    pub id: u32,
    /// `hdlr` handler: `vide`, `soun`…
    pub handler: FourCC,
    /// First `stsd` entry: `avc1`, `hvc1`, `mp4a`…
    pub codec: Option<FourCC>,
    /// `tkhd` duration, movie timescale.
    pub tkhd_duration: u64,
    /// `tkhd` width/height, integer part of 16.16.
    pub width: u32,
    pub height: u32,
    /// Width/height of the visual sample entry (the coded picture).
    pub coded_size: Option<(u16, u16)>,
    /// `mdhd`.
    pub timescale: u32,
    pub media_duration: u64,
    pub edits: Vec<Edit>,
    /// `stts` runs: (count, delta).
    pub stts: Vec<(u32, u32)>,
    /// `ctts` runs: (count, offset); empty when absent.
    pub ctts: Vec<(u32, i64)>,
    pub has_stsz: bool,
    /// `stsz`: constant size (0 = per-sample sizes in `sizes`).
    pub sample_size: u32,
    pub sample_count: u32,
    pub sizes: Vec<u32>,
    /// `stsc` runs: (first chunk, samples per chunk).
    pub stsc: Vec<(u32, u32)>,
    /// `stco`/`co64`.
    pub chunks: Vec<u64>,
    /// `stss`; `None` = every sample is a key frame.
    pub stss: Option<Vec<u32>>,
}

/// Presentation timing of a track, media ticks, computed over runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Timing {
    pub samples: u64,
    /// Sum of `stts` deltas.
    pub duration: u64,
    /// Earliest composition time (decode time + `ctts`).
    pub first_ct: i64,
    /// Latest composition time of a sample start.
    pub last_ct: i64,
    pub min_delta: u32,
    pub max_delta: u32,
}

impl Track {
    pub fn is_video(&self) -> bool {
        &self.handler == b"vide"
    }

    pub fn is_audio(&self) -> bool {
        &self.handler == b"soun"
    }

    /// Samples by `stts`.
    pub fn stts_samples(&self) -> u64 {
        self.stts.iter().map(|&(c, _)| c as u64).sum()
    }

    /// Samples by `ctts`.
    pub fn ctts_samples(&self) -> u64 {
        self.ctts.iter().map(|&(c, _)| c as u64).sum()
    }

    /// Codec as `vidcheck` names it.
    pub fn codec_name(&self) -> String {
        match self.codec.as_ref() {
            Some(b"avc1") | Some(b"avc3") => "H264".into(),
            Some(b"hvc1") | Some(b"hev1") => "HEVC".into(),
            Some(b"mp4a") => "AAC".into(),
            Some(c) => cc(c),
            None => "none".into(),
        }
    }

    /// Frame size: the coded one when the sample entry has it, else `tkhd`.
    pub fn size(&self) -> (u32, u32) {
        self.coded_size
            .map_or((self.width, self.height), |(w, h)| (w as u32, h as u32))
    }

    /// Walk `stts` and `ctts` runs together (never per sample).
    pub fn timing(&self) -> Timing {
        let mut t = Timing {
            min_delta: u32::MAX,
            ..Timing::default()
        };
        let mut first: Option<i64> = None;
        let mut last = i64::MIN;
        let mut d: i128 = 0;
        let mut ci = 0usize;
        let mut c_left: u64 = self.ctts.first().map_or(0, |&(c, _)| c as u64);
        for &(count, delta) in &self.stts {
            let mut left = count as u64;
            if left > 0 {
                t.min_delta = t.min_delta.min(delta);
                t.max_delta = t.max_delta.max(delta);
            }
            while left > 0 {
                while c_left == 0 && ci + 1 < self.ctts.len() {
                    ci += 1;
                    c_left = self.ctts[ci].0 as u64;
                }
                let (k, off) = if c_left > 0 {
                    let k = left.min(c_left);
                    c_left -= k;
                    (k, self.ctts[ci].1)
                } else {
                    (left, 0)
                };
                let f = d + off as i128;
                let l = d + (k as i128 - 1) * delta as i128 + off as i128;
                let f = f.clamp(i64::MIN as i128, i64::MAX as i128) as i64;
                let l = l.clamp(i64::MIN as i128, i64::MAX as i128) as i64;
                first = Some(first.map_or(f, |x: i64| x.min(f)));
                last = last.max(l);
                d += k as i128 * delta as i128;
                left -= k;
                t.samples += k;
            }
        }
        if t.min_delta == u32::MAX {
            t.min_delta = 0;
        }
        t.duration = d.clamp(0, u64::MAX as i128) as u64;
        t.first_ct = first.unwrap_or(0);
        t.last_ct = if last == i64::MIN { 0 } else { last };
        t
    }

    /// Empty edits (a delay before the track starts), movie timescale.
    fn empty_edits(&self) -> u64 {
        self.edits
            .iter()
            .take_while(|e| e.media_time < 0)
            .map(|e| e.duration)
            .fold(0u64, u64::saturating_add)
    }

    /// Where the media starts according to the edit list (first non-empty edit), media ticks.
    fn edit_media_time(&self) -> i64 {
        self.edits
            .iter()
            .find(|e| e.media_time >= 0)
            .map_or(0, |e| e.media_time)
    }

    /// When the first sample is shown, seconds: empty edits + (earliest composition time − the
    /// edit's media time).
    pub fn first_pts_s(&self, movie_timescale: u32) -> f64 {
        let t = self.timing();
        let delay = if movie_timescale > 0 {
            self.empty_edits() as f64 / movie_timescale as f64
        } else {
            0.0
        };
        delay + secs(t.first_ct - self.edit_media_time(), self.timescale)
    }

    /// Presentation duration, seconds: the edit list when there is one, else the samples.
    pub fn duration_s(&self, movie_timescale: u32) -> f64 {
        if !self.edits.is_empty() && movie_timescale > 0 {
            let s = self
                .edits
                .iter()
                .map(|e| e.duration)
                .fold(0u64, u64::saturating_add);
            s as f64 / movie_timescale as f64
        } else {
            secs(self.timing().duration as i64, self.timescale)
        }
    }

    /// Frame rate from the shortest `stts` delta, snapped to an integer or an NTSC `N·1000/1001`
    /// rate when within 0.02 % (MF writes 1e7/30 as 333333/333334 alternately).
    pub fn infer_fps(&self) -> Option<Fps> {
        let t = self.timing();
        if t.min_delta == 0 || self.timescale == 0 {
            return None;
        }
        // 1e7/30 in 100 ns is off by 1e-6; NTSC is 0.1 % away from the integer rate.
        const SNAP: f64 = 2e-4;
        let f = self.timescale as f64 / t.min_delta as f64;
        let r = f.round();
        if r >= 1.0 && ((f - r) / r).abs() < SNAP && r <= u32::MAX as f64 {
            return Some(Fps::new(r as u32, 1));
        }
        let n = (f * 1.001).round();
        if n >= 1.0 && ((f - n / 1.001) / f).abs() < SNAP && n * 1000.0 <= u32::MAX as f64 {
            return Some(Fps::new(n as u32 * 1000, 1001));
        }
        Some(Fps::new(self.timescale, t.min_delta))
    }

    /// Chunks as (offset, bytes, samples) from `stsc` + `stco` + `stsz`; errors describe how
    /// the tables disagree.
    pub fn chunk_layout(&self) -> std::result::Result<Vec<(u64, u64, u32)>, String> {
        if !self.has_stsz {
            return Err("немає stsz".into());
        }
        if self.sample_size == 0 && self.sizes.len() != self.sample_count as usize {
            return Err("stsz: розмірів менше, ніж семплів".into());
        }
        if let Some(&(f, _)) = self.stsc.first()
            && f != 1
        {
            return Err(format!("stsc починається з фрагмента {f}, не з 1"));
        }
        if self.stsc.windows(2).any(|w| w[1].0 <= w[0].0) {
            return Err("stsc: номери фрагментів не зростають".into());
        }
        let mut out = Vec::with_capacity(self.chunks.len());
        let mut si: u64 = 0;
        let mut e = 0usize;
        for (i, &off) in self.chunks.iter().enumerate() {
            let chunk = i as u64 + 1;
            while e + 1 < self.stsc.len() && self.stsc[e + 1].0 as u64 <= chunk {
                e += 1;
            }
            let spc = self.stsc.get(e).map_or(0, |s| s.1) as u64;
            if si + spc > self.sample_count as u64 {
                return Err(format!(
                    "stsc/stco описують більше семплів, ніж stsz ({})",
                    self.sample_count
                ));
            }
            let bytes = if self.sample_size > 0 {
                spc * self.sample_size as u64
            } else {
                self.sizes[si as usize..(si + spc) as usize]
                    .iter()
                    .map(|&s| s as u64)
                    .sum()
            };
            out.push((off, bytes, spc as u32));
            si += spc;
        }
        if si != self.sample_count as u64 {
            return Err(format!(
                "stsc/stco покривають {si} із {} семплів",
                self.sample_count
            ));
        }
        Ok(out)
    }
}

fn secs(ticks: i64, timescale: u32) -> f64 {
    if timescale == 0 {
        0.0
    } else {
        ticks as f64 / timescale as f64
    }
}

/// What `moov` says, plus where the top-level boxes are.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Mp4Info {
    pub file_len: u64,
    pub major_brand: Option<FourCC>,
    pub top: Vec<TopBox>,
    /// `mvhd`.
    pub timescale: u32,
    pub duration: u64,
    pub tracks: Vec<Track>,
}

impl Mp4Info {
    pub fn video(&self) -> Option<&Track> {
        self.tracks.iter().find(|t| t.is_video())
    }

    pub fn audio(&self) -> Option<&Track> {
        self.tracks.iter().find(|t| t.is_audio())
    }

    /// `moov` is the last top-level box — the precondition of appending `udta/meta` without
    /// touching `stco` (§7 item 12, `LhMp4AddMeta`).
    pub fn moov_last(&self) -> bool {
        self.top.last().is_some_and(|b| &b.kind == b"moov")
    }

    /// `mvhd` duration, seconds (what `MF_PD_DURATION` reports).
    pub fn duration_s(&self) -> f64 {
        secs(self.duration.min(i64::MAX as u64) as i64, self.timescale)
    }

    fn mdat_ranges(&self) -> Vec<(u64, u64)> {
        self.top
            .iter()
            .filter(|b| &b.kind == b"mdat")
            .map(TopBox::payload)
            .collect()
    }
}

// ---------------------------------------------------------------------------------------------
// Reading

/// Read an MP4 from a seekable stream: headers of the top-level boxes, `moov` into memory.
pub fn read_mp4<R: Read + Seek>(r: &mut R) -> CheckResult<Mp4Info> {
    let len = r.seek(SeekFrom::End(0))?;
    r.seek(SeekFrom::Start(0))?;
    let mut info = Mp4Info {
        file_len: len,
        ..Mp4Info::default()
    };
    let mut moov: Option<Vec<u8>> = None;
    let mut off = 0u64;
    while off < len {
        if info.top.len() >= MAX_TOP_BOXES {
            return Err(CheckError::TooLarge(
                "кількість боксів верхнього рівня",
                info.top.len() as u64,
            ));
        }
        if len - off < 8 {
            return Err(CheckError::Truncated("заголовок бокса"));
        }
        r.seek(SeekFrom::Start(off))?;
        let mut h = [0u8; 8];
        r.read_exact(&mut h)?;
        let size32 = u32::from_be_bytes([h[0], h[1], h[2], h[3]]) as u64;
        let kind: FourCC = [h[4], h[5], h[6], h[7]];
        let (size, header) = match size32 {
            1 => {
                if len - off < 16 {
                    return Err(CheckError::Truncated("64-бітний розмір бокса"));
                }
                let mut l = [0u8; 8];
                r.read_exact(&mut l)?;
                (u64::from_be_bytes(l), 16)
            }
            0 => (len - off, 8),
            s => (s, 8),
        };
        if size < header {
            return Err(CheckError::Malformed(format!(
                "бокс {} розміром {size} менший за заголовок",
                cc(&kind)
            )));
        }
        if size > len - off {
            return Err(CheckError::Truncated("бокс верхнього рівня"));
        }
        let b = TopBox {
            kind,
            offset: off,
            header,
            size,
        };
        if &kind == b"ftyp" && size >= header + 4 {
            let mut m = [0u8; 4];
            r.read_exact(&mut m)?;
            info.major_brand = Some(m);
        }
        if &kind == b"moov" {
            if moov.is_some() {
                return Err(CheckError::Malformed("два moov".into()));
            }
            let n = size - header;
            if n > MAX_MOOV {
                return Err(CheckError::TooLarge("moov", n));
            }
            let mut buf = vec![0u8; n as usize];
            r.read_exact(&mut buf)?;
            moov = Some(buf);
        }
        info.top.push(b);
        off += size;
    }
    let moov = moov.ok_or(CheckError::Missing("moov"))?;
    parse_moov(&moov, &mut info)?;
    Ok(info)
}

/// [`read_mp4`] over a file (buffered; `mdat` is seeked over, not read).
pub fn read_mp4_file(path: &std::path::Path) -> CheckResult<Mp4Info> {
    read_mp4(&mut std::io::BufReader::new(std::fs::File::open(path)?))
}

/// [`read_mp4`] over bytes in memory.
pub fn parse_mp4(bytes: &[u8]) -> CheckResult<Mp4Info> {
    read_mp4(&mut std::io::Cursor::new(bytes))
}

/// Children of a container payload: (type, payload).
fn children(b: &[u8]) -> CheckResult<Vec<(FourCC, &[u8])>> {
    let mut out = Vec::new();
    let mut c = Cursor::new(b, "бокс");
    while !c.at_end() {
        if out.len() >= MAX_CHILDREN {
            return Err(CheckError::TooLarge(
                "кількість вкладених боксів",
                out.len() as u64,
            ));
        }
        let start = c.pos();
        let size32 = c.be32()? as u64;
        let kind = c.fourcc()?;
        let size = match size32 {
            1 => c.be64()?,
            0 => (b.len() - start) as u64,
            s => s,
        };
        let header = (c.pos() - start) as u64;
        if size < header {
            return Err(CheckError::Malformed(format!(
                "бокс {} розміром {size} менший за заголовок",
                cc(&kind)
            )));
        }
        let body = size - header;
        if body > c.remaining() as u64 {
            return Err(CheckError::Truncated("вкладений бокс"));
        }
        out.push((kind, c.take(body as usize)?));
    }
    Ok(out)
}

/// Skip version + flags of a full box, return the version.
fn full(c: &mut Cursor) -> CheckResult<u8> {
    let v = c.u8()?;
    c.skip(3)?;
    Ok(v)
}

fn parse_moov(b: &[u8], info: &mut Mp4Info) -> CheckResult<()> {
    let mut mvhd = false;
    for (kind, body) in children(b)? {
        match &kind {
            b"mvhd" => {
                let mut c = Cursor::new(body, "mvhd");
                if full(&mut c)? == 1 {
                    c.skip(16)?;
                    info.timescale = c.be32()?;
                    info.duration = c.be64()?;
                } else {
                    c.skip(8)?;
                    info.timescale = c.be32()?;
                    info.duration = c.be32()? as u64;
                }
                mvhd = true;
            }
            b"trak" => {
                if info.tracks.len() >= MAX_TRACKS {
                    return Err(CheckError::TooLarge(
                        "кількість доріжок",
                        info.tracks.len() as u64,
                    ));
                }
                let mut t = Track::default();
                parse_trak(body, &mut t)?;
                info.tracks.push(t);
            }
            _ => {}
        }
    }
    if !mvhd {
        return Err(CheckError::Missing("mvhd"));
    }
    Ok(())
}

fn parse_trak(b: &[u8], t: &mut Track) -> CheckResult<()> {
    for (kind, body) in children(b)? {
        match &kind {
            b"tkhd" => {
                let mut c = Cursor::new(body, "tkhd");
                if full(&mut c)? == 1 {
                    c.skip(16)?;
                    t.id = c.be32()?;
                    c.skip(4)?;
                    t.tkhd_duration = c.be64()?;
                } else {
                    c.skip(8)?;
                    t.id = c.be32()?;
                    c.skip(4)?;
                    t.tkhd_duration = c.be32()? as u64;
                }
                // reserved 8, layer, alternate group, volume, reserved, matrix 36
                c.skip(8 + 2 + 2 + 2 + 2 + 36)?;
                t.width = c.be32()? >> 16;
                t.height = c.be32()? >> 16;
            }
            b"edts" => {
                for (k, e) in children(body)? {
                    if &k == b"elst" {
                        parse_elst(e, t)?;
                    }
                }
            }
            b"mdia" => parse_mdia(body, t)?,
            _ => {}
        }
    }
    Ok(())
}

fn parse_elst(b: &[u8], t: &mut Track) -> CheckResult<()> {
    let mut c = Cursor::new(b, "elst");
    let v = full(&mut c)?;
    let n = c.be32()? as u64;
    c.need(n, if v == 1 { 20 } else { 12 })?;
    t.edits = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let (duration, media_time) = if v == 1 {
            (c.be64()?, c.be64()? as i64)
        } else {
            (c.be32()? as u64, c.be32()? as i32 as i64)
        };
        let rate = c.be32()? as i32;
        t.edits.push(Edit {
            duration,
            media_time,
            rate,
        });
    }
    Ok(())
}

fn parse_mdia(b: &[u8], t: &mut Track) -> CheckResult<()> {
    for (kind, body) in children(b)? {
        match &kind {
            b"mdhd" => {
                let mut c = Cursor::new(body, "mdhd");
                if full(&mut c)? == 1 {
                    c.skip(16)?;
                    t.timescale = c.be32()?;
                    t.media_duration = c.be64()?;
                } else {
                    c.skip(8)?;
                    t.timescale = c.be32()?;
                    t.media_duration = c.be32()? as u64;
                }
            }
            b"hdlr" => {
                let mut c = Cursor::new(body, "hdlr");
                full(&mut c)?;
                c.skip(4)?;
                t.handler = c.fourcc()?;
            }
            b"minf" => {
                for (k, m) in children(body)? {
                    if &k == b"stbl" {
                        parse_stbl(m, t)?;
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn parse_stbl(b: &[u8], t: &mut Track) -> CheckResult<()> {
    for (kind, body) in children(b)? {
        match &kind {
            b"stsd" => {
                let mut c = Cursor::new(body, "stsd");
                full(&mut c)?;
                if c.be32()? > 0 {
                    let size = c.be32()? as usize;
                    t.codec = Some(c.fourcc()?);
                    // VisualSampleEntry: 6 reserved + 2 data ref + 16 pre-defined/reserved,
                    // then width and height (`hdlr` precedes `minf` in `mdia`).
                    if t.is_video() && size >= 8 + 28 && c.remaining() >= 28 {
                        c.skip(24)?;
                        let w = c.be16()?;
                        let h = c.be16()?;
                        t.coded_size = Some((w, h));
                    }
                }
            }
            b"stts" => {
                let mut c = Cursor::new(body, "stts");
                full(&mut c)?;
                let n = c.be32()? as u64;
                c.need(n, 8)?;
                t.stts = (0..n)
                    .map(|_| Ok((c.be32()?, c.be32()?)))
                    .collect::<CheckResult<_>>()?;
            }
            b"ctts" => {
                let mut c = Cursor::new(body, "ctts");
                let v = full(&mut c)?;
                let n = c.be32()? as u64;
                c.need(n, 8)?;
                t.ctts = (0..n)
                    .map(|_| {
                        let k = c.be32()?;
                        let o = c.be32()?;
                        Ok((k, if v == 1 { o as i32 as i64 } else { o as i64 }))
                    })
                    .collect::<CheckResult<_>>()?;
            }
            b"stsz" => {
                let mut c = Cursor::new(body, "stsz");
                full(&mut c)?;
                t.sample_size = c.be32()?;
                t.sample_count = c.be32()?;
                t.has_stsz = true;
                if t.sample_size == 0 {
                    let n = t.sample_count as u64;
                    c.need(n, 4)?;
                    t.sizes = (0..n).map(|_| c.be32()).collect::<CheckResult<_>>()?;
                }
            }
            b"stsc" => {
                let mut c = Cursor::new(body, "stsc");
                full(&mut c)?;
                let n = c.be32()? as u64;
                c.need(n, 12)?;
                t.stsc = (0..n)
                    .map(|_| {
                        let f = c.be32()?;
                        let s = c.be32()?;
                        c.skip(4)?;
                        Ok((f, s))
                    })
                    .collect::<CheckResult<_>>()?;
            }
            b"stco" | b"co64" => {
                let mut c = Cursor::new(body, "stco");
                full(&mut c)?;
                let n = c.be32()? as u64;
                let wide = &kind == b"co64";
                c.need(n, if wide { 8 } else { 4 })?;
                t.chunks = (0..n)
                    .map(|_| {
                        if wide {
                            c.be64()
                        } else {
                            c.be32().map(u64::from)
                        }
                    })
                    .collect::<CheckResult<_>>()?;
            }
            b"stss" => {
                let mut c = Cursor::new(body, "stss");
                full(&mut c)?;
                let n = c.be32()? as u64;
                c.need(n, 4)?;
                t.stss = Some((0..n).map(|_| c.be32()).collect::<CheckResult<_>>()?);
            }
            _ => {}
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Checks

/// CFR through durations (§2.3, §7 items 1 and 18): every sample starts and ends on the frame
/// grid; a sample may cover several frames (the recorder stretched it).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CfrReport {
    /// Samples in runs whose edges are off the grid by more than the tolerance.
    pub off_grid: u64,
    /// Largest distance of a sample edge from the grid, media ticks.
    pub max_dev_ticks: i64,
    /// Samples that cover more than one frame, and the most frames one covers.
    pub stretched: u64,
    pub max_frames_per_sample: i64,
    /// Output frames (`evprobe`'s `slots`): total duration in frames.
    pub slots: i64,
    pub tolerance_ticks: i64,
    /// The timescale is too coarse to tell a frame from rounding.
    pub coarse: bool,
}

impl Track {
    /// Check the frame grid of `fps`. Arithmetic is exact (`i128`, scaled by `fps.num`); per
    /// run the edge deviation is linear, so its two ends bound it — no per-sample expansion.
    /// Tolerance: 1 media tick + the 100 ns rounding of MF sample times.
    pub fn cfr(&self, fps: Fps) -> CfrReport {
        let mut r = CfrReport::default();
        let ts = self.timescale as i128;
        let (num, den) = (fps.num as i128, fps.den as i128);
        let tol_ticks = 1 + (self.timescale as u64).div_ceil(10_000_000) as i64;
        r.tolerance_ticks = tol_ticks;
        let frame = ts * den; // frame duration × num
        let tol = tol_ticks as i128 * num;
        if num == 0 || frame <= 4 * tol {
            r.coarse = true;
            return r;
        }
        let round = |x: i128| (x + frame / 2).div_euclid(frame);
        let mut d: i128 = 0;
        let mut max_dev: i128 = 0;
        for &(count, delta) in &self.stts {
            if count == 0 {
                continue;
            }
            let dd = delta as i128 * num;
            let n = round(dd);
            let dev0 = d * num - round(d * num) * frame;
            let e = dd - n * frame;
            let dev1 = dev0 + count as i128 * e;
            let worst = dev0.abs().max(dev1.abs());
            max_dev = max_dev.max(worst);
            if n == 0 || worst > tol {
                r.off_grid += count as u64;
            }
            if n > 1 {
                r.stretched += count as u64;
            }
            r.max_frames_per_sample = r.max_frames_per_sample.max(n.min(i64::MAX as i128) as i64);
            d += count as i128 * delta as i128;
        }
        r.max_dev_ticks = (max_dev / num).min(i64::MAX as i128) as i64;
        r.slots = round(d * num).min(i64::MAX as i128) as i64;
        r
    }
}

/// The `vidcheck` summary of the video track, from the container.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VidSummary {
    pub codec: String,
    pub width: u32,
    pub height: u32,
    pub fps: Option<Fps>,
    /// `mvhd` duration, seconds.
    pub duration_s: f64,
    /// Samples (what a decoder returns one by one).
    pub frames: u64,
    pub first_s: f64,
    pub last_s: f64,
    /// Largest step between samples, ms (decode order).
    pub max_gap_ms: f64,
    pub keyframes: u64,
}

impl fmt::Display for VidSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let fps = self.fps.map_or("?".to_string(), |x| x.to_string());
        writeln!(
            f,
            "codec={} {}x{} fps={fps} duration={:.3} s",
            self.codec, self.width, self.height, self.duration_s
        )?;
        write!(
            f,
            "frames={} first={:.3} last={:.3} maxGap={:.1} ms keyframes={}",
            self.frames, self.first_s, self.last_s, self.max_gap_ms, self.keyframes
        )
    }
}

impl Mp4Info {
    /// The two lines `vidcheck` printed, for the video track.
    pub fn summary(&self) -> Option<VidSummary> {
        let v = self.video()?;
        let t = v.timing();
        let (w, h) = v.size();
        let shift = v.edit_media_time();
        Some(VidSummary {
            codec: v.codec_name(),
            width: w,
            height: h,
            fps: v.infer_fps(),
            duration_s: self.duration_s(),
            frames: t.samples,
            first_s: v.first_pts_s(self.timescale),
            last_s: secs(t.last_ct - shift, v.timescale),
            max_gap_ms: secs(t.max_delta as i64, v.timescale) * 1000.0,
            keyframes: v.stss.as_ref().map_or(t.samples, |s| s.len() as u64),
        })
    }
}

/// What the file is expected to be; `None` = not checked.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mp4Expect {
    pub fps: Option<Fps>,
    /// Output frames (sum of durations in frames).
    pub slots: Option<i64>,
    pub duration_s: Option<f64>,
    pub duration_tol_s: f64,
}

impl Default for Mp4Expect {
    fn default() -> Self {
        Self {
            fps: None,
            slots: None,
            duration_s: None,
            duration_tol_s: DURATION_TOL_S,
        }
    }
}

fn table_problems(t: &Track) -> Vec<String> {
    let mut p = Vec::new();
    let n = t.sample_count as u64;
    if !t.has_stsz {
        p.push("немає stsz".into());
    }
    if t.stts_samples() != n {
        p.push(format!("stts {} ≠ stsz {n}", t.stts_samples()));
    }
    if !t.ctts.is_empty() && t.ctts_samples() != n {
        p.push(format!("ctts {} ≠ stsz {n}", t.ctts_samples()));
    }
    if let Some(s) = &t.stss
        && (s.first().is_some_and(|&x| x == 0)
            || s.windows(2).any(|w| w[1] <= w[0])
            || s.last().is_some_and(|&x| x as u64 > n))
    {
        p.push("stss не зростає або виходить за семпли".into());
    }
    if t.has_stsz
        && let Err(e) = t.chunk_layout()
    {
        p.push(e);
    }
    p
}

fn track_name(t: &Track) -> String {
    format!("{}#{}", cc(&t.handler), t.id)
}

/// All container checks. Names: `moovLast` is informational only (not a check).
pub fn check_mp4(info: &Mp4Info, e: &Mp4Expect) -> Vec<Check> {
    let mut out = Vec::new();
    let Some(v) = info.video() else {
        out.push(Check::new("videoTrack", false, "немає відеодоріжки"));
        return out;
    };
    out.push(Check::new(
        "videoTrack",
        true,
        format!("{} {}", track_name(v), v.codec_name()),
    ));

    // Sample tables agree with each other (mp4boxes: stsz count, stts, stss).
    let mut bad = Vec::new();
    for t in &info.tracks {
        for p in table_problems(t) {
            bad.push(format!("{}: {p}", track_name(t)));
        }
    }
    out.push(Check::new(
        "sampleTables",
        bad.is_empty(),
        if bad.is_empty() {
            format!("{} семплів відео", v.sample_count)
        } else {
            bad.join("; ")
        },
    ));

    // §7 item 12: chunk offsets point into mdat (appending metadata must not shift them).
    let mdat = info.mdat_ranges();
    let (mut chunks, mut outside) = (0u64, 0u64);
    for t in &info.tracks {
        if let Ok(l) = t.chunk_layout() {
            for (off, bytes, _) in l {
                chunks += 1;
                let end = off.saturating_add(bytes);
                if !mdat.iter().any(|&(a, b)| off >= a && end <= b) {
                    outside += 1;
                }
            }
        }
    }
    out.push(Check::new(
        "chunksInMdat",
        outside == 0 && !mdat.is_empty(),
        format!(
            "{outside} із {chunks} фрагментів поза mdat (moovLast={})",
            info.moov_last()
        ),
    ));

    // §7 item 1: MP4 counts time by sample durations — mdhd must equal the sum of stts.
    let mut bad = Vec::new();
    for t in &info.tracks {
        let s = t.timing().duration;
        if s.abs_diff(t.media_duration) > 1 {
            bad.push(format!(
                "{}: stts {s} ≠ mdhd {} (шкала {})",
                track_name(t),
                t.media_duration,
                t.timescale
            ));
        }
    }
    out.push(Check::new(
        "mediaDuration",
        bad.is_empty(),
        if bad.is_empty() {
            "сума тривалостей семплів = mdhd".into()
        } else {
            bad.join("; ")
        },
    ));

    // tkhd = edit list or media duration in movie units; mvhd = the longest track.
    let mut bad = Vec::new();
    let mut longest = 0u64;
    for t in &info.tracks {
        let want = if t.edits.is_empty() {
            if t.timescale == 0 {
                0
            } else {
                (t.media_duration as u128 * info.timescale as u128 / t.timescale as u128) as u64
            }
        } else {
            t.edits
                .iter()
                .map(|e| e.duration)
                .fold(0u64, u64::saturating_add)
        };
        if t.tkhd_duration.abs_diff(want) > 1 {
            bad.push(format!(
                "{}: tkhd {} ≠ {want}",
                track_name(t),
                t.tkhd_duration
            ));
        }
        longest = longest.max(t.tkhd_duration);
    }
    if info.duration.abs_diff(longest) > 1 {
        bad.push(format!(
            "mvhd {} ≠ найдовша доріжка {longest}",
            info.duration
        ));
    }
    out.push(Check::new(
        "movieDuration",
        bad.is_empty(),
        if bad.is_empty() {
            format!("{:.3} с", info.duration_s())
        } else {
            bad.join("; ")
        },
    ));

    // CFR through durations (§2.3, §7 items 1, 18).
    let fps = e.fps.or_else(|| v.infer_fps());
    match fps {
        Some(f) => {
            let c = v.cfr(f);
            let ok = !c.coarse && c.off_grid == 0;
            out.push(Check::new(
                "cfr",
                ok,
                if c.coarse {
                    format!("шкала {} надто груба для {f} к/с", v.timescale)
                } else {
                    format!(
                        "fps {f}: поза сіткою {} семплів (відхил до {} тіків, допуск {}), розтягнутих {} (до {} кадрів), кадрів {}",
                        c.off_grid,
                        c.max_dev_ticks,
                        c.tolerance_ticks,
                        c.stretched,
                        c.max_frames_per_sample,
                        c.slots
                    )
                },
            ));
            if let Some(want) = e.fps {
                let got = v.infer_fps();
                let ok = got
                    .is_some_and(|g| ((g.as_f64() - want.as_f64()) / want.as_f64()).abs() < 1e-4);
                out.push(Check::new(
                    "fps",
                    ok,
                    format!(
                        "{} (очікується {want})",
                        got.map_or("?".into(), |g| g.to_string())
                    ),
                ));
            }
            if let Some(want) = e.slots {
                out.push(Check::new(
                    "slots",
                    c.slots == want,
                    format!("{} (очікується {want})", c.slots),
                ));
            }
            // §7 item 3: GOP = fps — key frames 1, 1+fps, 1+2·fps… (mp4boxes "first: (1, 31, 61").
            let gop = f.as_f64().round() as u64;
            match &v.stss {
                None => out.push(Check::new("gop", true, "stss немає: усі кадри ключові")),
                Some(s) => {
                    let ok = s.first() == Some(&1)
                        && s.windows(2)
                            .all(|w| (w[1] as u64).wrapping_sub(w[0] as u64) == gop);
                    let first: Vec<String> = s.iter().take(8).map(u32::to_string).collect();
                    out.push(Check::new(
                        "gop",
                        ok,
                        format!(
                            "{} ключових, first: ({}), крок має бути {gop}",
                            s.len(),
                            first.join(", ")
                        ),
                    ));
                }
            }
        }
        None => out.push(Check::new("cfr", false, "частоту кадрів не визначити")),
    }

    // §7 item 2: the first frame is shown at zero (no ctts without an edit list).
    let first = v.first_pts_s(info.timescale);
    out.push(Check::new(
        "firstFrameAtZero",
        first.abs() <= FIRST_PTS_TOL_S,
        format!("перший кадр о {:.4} с", first),
    ));

    // §7 item 7: even sides (4:2:0).
    let (w, h) = v.size();
    out.push(Check::new(
        "evenSize",
        w % 2 == 0 && h % 2 == 0 && w > 0 && h > 0,
        format!("{w}x{h}"),
    ));

    // evprobe Check: audio as long as the video (±50 ms), starting within the priming window.
    if let Some(a) = info.audio() {
        let vd = v.duration_s(info.timescale);
        let ad = a.duration_s(info.timescale);
        out.push(Check::new(
            "audioDrift",
            (ad - vd).abs() <= AUDIO_DRIFT_MAX_S,
            format!("звук {ad:.3} с, відео {vd:.3} с"),
        ));
        let s = a.first_pts_s(info.timescale);
        out.push(Check::new(
            "audioStart",
            s.abs() <= AUDIO_START_MAX_S,
            format!("звук починається о {:.4} с", s),
        ));
    }

    if let Some(want) = e.duration_s {
        let d = info.duration_s();
        out.push(Check::new(
            "duration",
            (d - want).abs() <= e.duration_tol_s,
            format!("{d:.3} с (очікується {want:.3} ± {:.3})", e.duration_tol_s),
        ));
    }
    out
}
