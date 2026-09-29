//! A bounded GIF reader and the GIF checks of the LH harness (ZK-99).
//!
//! Port of `gifcheck.py` (CAPS-89): header, global palette, the `NETSCAPE2.0` loop, frames
//! (graphic control: delay, disposal, transparency; descriptor: rectangle, local palette,
//! interlace) and FULL LZW decoding of every frame — so a broken encoder does not pass as "the
//! file is there". With [`GifOptions::bars`] the frames are composed the way `composite` did and
//! the synthetic slot number (16 bits in bars at `(bit·64+32, 32)`) is read from every composed
//! frame. The expectations of `write_geom_test.py` and the rules of the LH writer (`GifStream`,
//! `GifDelayFor`, inventory §6.4) become [`check_gif`].
//!
//! Differences from the Python, all towards "error instead of a crash": sizes are capped
//! ([`MAX_PIXELS`]); a frame outside the logical screen is clipped and reported (Python raised
//! `IndexError`); interlaced frames are de-interlaced when composing (Python composed their rows
//! in file order; the LH writer never interlaces). The LZW table stops growing at 4096 entries
//! (Python kept appending unreachable entries — same output for valid streams).

use super::read::Cursor;
use super::{Check, CheckError, CheckResult};
use std::path::Path;

/// Pixels of the logical screen and of one frame (8192 × 4096).
pub const MAX_PIXELS: u64 = 1 << 25;
/// Largest GIF file [`read_gif_file`] reads into memory.
pub const MAX_FILE: u64 = 1 << 30;
/// Shortest delay browsers honour, hundredths: 0 and 1 are played as 10 (Chrome, Firefox,
/// Safari), so a 60 fps GIF (`GifDelayFor`: 2, 2, 1, …) stutters at 10 fps on its 1-cs frames.
pub const MIN_DELAY_CS: u16 = 2;
/// Sum of delays against the expected duration, hundredths (`write_geom_test.py`:
/// `abs(total_cs − N·100/30) ≤ 2`).
pub const TOTAL_CS_TOL: i64 = 2;
/// Palette size the harness accepts ("палітра є": 4..=256 colours).
pub const MIN_COLORS: usize = 4;

/// Delay of output frame `m` at `fps`, hundredths — the accumulated rounding of `GifDelayFor`
/// (15 fps is not 7·N): `round((m+1)·100/fps) − round(m·100/fps)`.
pub fn delay_for(m: u32, fps: f64) -> u16 {
    let r = |k: f64| (k * 100.0 / fps + 0.5) as i64;
    (r(m as f64 + 1.0) - r(m as f64)).clamp(0, u16::MAX as i64) as u16
}

/// Total the LH writer gives the last frame to reach (`GifStream::Finish(totalOut·100/fps)`).
pub fn total_cs_for(frames: u64, fps: f64) -> i64 {
    (frames as f64 * 100.0 / fps + 0.5) as i64
}

/// One frame as the file describes it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GifFrame {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
    /// Hundredths of a second.
    pub delay: u16,
    /// 0 unspecified, 1 leave in place, 2 restore background, 3 restore previous.
    pub disposal: u8,
    pub transparent: bool,
    pub transparent_index: u8,
    /// Entries of the local palette (0 = global).
    pub local_colors: usize,
    pub interlace: bool,
}

/// What `gifcheck.py` printed, and the frames.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GifInfo {
    pub w: u16,
    pub h: u16,
    /// Entries of the global palette (0 = none).
    pub colors: usize,
    /// `NETSCAPE2.0` repeat count: `Some(0)` forever, `None` = play once (no extension).
    pub loop_count: Option<u16>,
    pub frames: Vec<GifFrame>,
    pub bytes: usize,
    /// The file ends with the `0x3B` trailer (`GifStream::Finish` wrote it).
    pub trailer: bool,
    /// Frames (partly) outside the logical screen.
    pub frames_outside: usize,
    /// Slot numbers of the composed frames (with [`GifOptions::bars`]).
    pub bars: Option<Vec<u16>>,
}

impl GifInfo {
    pub fn delays(&self) -> Vec<u16> {
        self.frames.iter().map(|f| f.delay).collect()
    }
    pub fn total_cs(&self) -> i64 {
        self.frames.iter().map(|f| f.delay as i64).sum()
    }
    pub fn trans_frames(&self) -> usize {
        self.frames.iter().filter(|f| f.transparent).count()
    }
    pub fn full_frames(&self) -> usize {
        self.frames
            .iter()
            .filter(|f| f.w == self.w && f.h == self.h)
            .count()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GifOptions {
    /// Compose the frames and read the synthetic bar number from each (`--bars`).
    pub bars: bool,
}

/// Read a GIF file (at most [`MAX_FILE`] bytes).
pub fn read_gif_file(path: &Path, opts: GifOptions) -> CheckResult<GifInfo> {
    let len = std::fs::metadata(path)?.len();
    if len > MAX_FILE {
        return Err(CheckError::TooLarge("файл GIF", len));
    }
    parse_gif(&std::fs::read(path)?, opts)
}

fn palette(c: &mut Cursor, packed: u8) -> CheckResult<Vec<[u8; 3]>> {
    if packed & 0x80 == 0 {
        return Ok(Vec::new());
    }
    let n = 2usize << (packed & 7);
    let raw = c.take(n * 3)?;
    Ok(raw.as_chunks::<3>().0.to_vec())
}

/// Concatenated data sub-blocks up to the zero terminator (`read_blocks`).
fn blocks(c: &mut Cursor) -> CheckResult<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let n = c.u8()? as usize;
        if n == 0 {
            return Ok(out);
        }
        out.extend_from_slice(c.take(n)?);
    }
}

/// Order of rows in an interlaced frame: file row `i` → frame row.
fn interlace_rows(h: usize) -> Vec<usize> {
    let mut v = Vec::with_capacity(h);
    for (start, step) in [(0, 8), (4, 8), (2, 4), (1, 2)] {
        v.extend((start..h).step_by(step));
    }
    v
}

/// Parse a GIF and decode every frame.
pub fn parse_gif(b: &[u8], opts: GifOptions) -> CheckResult<GifInfo> {
    let mut c = Cursor::new(b, "GIF");
    let sig = c
        .take(6)
        .map_err(|_| CheckError::Malformed("не GIF".into()))?;
    if sig != b"GIF89a" && sig != b"GIF87a" {
        return Err(CheckError::Malformed("не GIF".into()));
    }
    let w = c.le16()?;
    let h = c.le16()?;
    let packed = c.u8()?;
    c.skip(2)?; // background, aspect
    if w as u64 * h as u64 > MAX_PIXELS {
        return Err(CheckError::TooLarge("екран GIF", w as u64 * h as u64));
    }
    let gct = palette(&mut c, packed)?;
    let mut info = GifInfo {
        w,
        h,
        colors: gct.len(),
        bytes: b.len(),
        ..GifInfo::default()
    };
    let (sw, sh) = (w as usize, h as usize);
    let mut canvas = if opts.bars {
        vec![[0u8; 3]; sw * sh]
    } else {
        Vec::new()
    };
    let mut bars = Vec::new();
    let mut gce: Option<(u8, u16, u8)> = None; // packed, delay, transparent index
    let mut idx: Vec<u8> = Vec::new();
    while !c.at_end() {
        let t = c.u8()?;
        match t {
            0x3B => {
                info.trailer = true;
                break;
            }
            0x21 => {
                let label = c.u8()?;
                match label {
                    0xF9 => {
                        let sz = c.u8()? as usize;
                        if sz < 4 {
                            return Err(CheckError::Malformed(format!(
                                "розширення керування графікою розміром {sz}"
                            )));
                        }
                        let body = c.take(sz)?;
                        let delay = u16::from_le_bytes([body[1], body[2]]);
                        gce = Some((body[0], delay, body[3]));
                        if c.u8()? != 0 {
                            return Err(CheckError::Malformed(
                                "розширення керування графікою без термінатора".into(),
                            ));
                        }
                    }
                    0xFF => {
                        let sz = c.u8()? as usize;
                        let app = c.take(sz)?;
                        let data = blocks(&mut c)?;
                        if app.starts_with(b"NETSCAPE2.0") && data.len() >= 3 {
                            info.loop_count = Some(u16::from_le_bytes([data[1], data[2]]));
                        }
                    }
                    _ => {
                        blocks(&mut c)?;
                    }
                }
            }
            0x2C => {
                let x = c.le16()?;
                let y = c.le16()?;
                let fw = c.le16()?;
                let fh = c.le16()?;
                let pk = c.u8()?;
                let lct = palette(&mut c, pk)?;
                let n = fw as u64 * fh as u64;
                if n > MAX_PIXELS {
                    return Err(CheckError::TooLarge("кадр GIF", n));
                }
                let min_code = c.u8()?;
                if !(2..=8).contains(&min_code) {
                    return Err(CheckError::Malformed(format!(
                        "кадр {}: мінімальний код LZW {min_code}",
                        info.frames.len()
                    )));
                }
                let data = blocks(&mut c)?;
                lzw_decode(&data, min_code, n as usize, &mut idx)?;
                if idx.len() != n as usize {
                    return Err(CheckError::Malformed(format!(
                        "кадр {}: розкодовано {} із {n}",
                        info.frames.len(),
                        idx.len()
                    )));
                }
                let (gpk, delay, tidx) = gce.take().unwrap_or((0, 0, 0));
                let f = GifFrame {
                    x,
                    y,
                    w: fw,
                    h: fh,
                    delay,
                    disposal: (gpk >> 2) & 7,
                    transparent: gpk & 1 != 0,
                    transparent_index: tidx,
                    local_colors: lct.len(),
                    interlace: pk & 0x40 != 0,
                };
                if x as usize + fw as usize > sw || y as usize + fh as usize > sh {
                    info.frames_outside += 1;
                }
                if opts.bars {
                    compose(
                        &mut canvas,
                        sw,
                        sh,
                        &f,
                        &idx,
                        if lct.is_empty() { &gct } else { &lct },
                    );
                    bars.push(read_bars(&canvas, sw, sh));
                }
                info.frames.push(f);
            }
            _ => {
                return Err(CheckError::Malformed(format!(
                    "невідомий блок 0x{t:02X} на {}",
                    c.pos() - 1
                )));
            }
        }
    }
    if opts.bars {
        info.bars = Some(bars);
    }
    Ok(info)
}

/// `composite`: draw a frame over the canvas ("leave previous" for every frame), transparent
/// pixels skipped, indices past the palette black.
fn compose(
    canvas: &mut [[u8; 3]],
    sw: usize,
    sh: usize,
    f: &GifFrame,
    idx: &[u8],
    pal: &[[u8; 3]],
) {
    let (fw, fh) = (f.w as usize, f.h as usize);
    let rows = if f.interlace {
        interlace_rows(fh)
    } else {
        (0..fh).collect()
    };
    let tr = f.transparent.then_some(f.transparent_index);
    for (i, &row) in rows.iter().enumerate() {
        let y = f.y as usize + row;
        if y >= sh {
            continue;
        }
        for xx in 0..fw {
            let x = f.x as usize + xx;
            if x >= sw {
                break;
            }
            let ci = idx[i * fw + xx];
            if Some(ci) == tr {
                continue;
            }
            canvas[y * sw + x] = pal.get(ci as usize).copied().unwrap_or([0, 0, 0]);
        }
    }
}

/// 16 bits at `(bit·64+32, 32)`, a bit is set when the mean of R, G, B is over 128.
fn read_bars(canvas: &[[u8; 3]], sw: usize, sh: usize) -> u16 {
    let mut num = 0u16;
    for bit in 0..16 {
        let x = bit * 64 + 32;
        if x < sw && 32 < sh {
            let [r, g, b] = canvas[32 * sw + x];
            if r as u32 + g as u32 + b as u32 > 384 {
                num |= 1 << bit;
            }
        }
    }
    num
}

/// The LZW string table as prefix links: entry = (prefix entry, last byte), with its first byte
/// and length cached. At most 4096 entries (12-bit codes).
struct LzwTable {
    prefix: Vec<u16>,
    suffix: Vec<u8>,
    first: Vec<u8>,
    len: Vec<u16>,
    size: usize,
}

impl LzwTable {
    const MAX: usize = 4096;

    fn new(clear: usize) -> Self {
        let mut t = Self {
            prefix: vec![0; Self::MAX],
            suffix: vec![0; Self::MAX],
            first: vec![0; Self::MAX],
            len: vec![0; Self::MAX],
            size: clear + 2,
        };
        for i in 0..clear {
            t.suffix[i] = i as u8;
            t.first[i] = i as u8;
            t.len[i] = 1;
        }
        t
    }

    fn add(&mut self, prefix: usize, byte: u8) {
        if self.size < Self::MAX {
            let s = self.size;
            self.prefix[s] = prefix as u16;
            self.suffix[s] = byte;
            self.first[s] = self.first[prefix];
            self.len[s] = self.len[prefix] + 1;
            self.size += 1;
        }
    }

    /// Append the string of `code` to `out`, at most up to `expect` bytes in total.
    fn emit(&self, code: usize, out: &mut Vec<u8>, expect: usize, stack: &mut [u8; Self::MAX]) {
        let l = self.len[code] as usize;
        let mut c = code;
        for i in (0..l).rev() {
            stack[i] = self.suffix[c];
            c = self.prefix[c] as usize;
        }
        let room = expect.saturating_sub(out.len());
        out.extend_from_slice(&stack[..l.min(room)]);
    }
}

/// `lzw_decode`: variable code size from `min_code + 1` to 12, clear and end codes; stops at
/// `expect` indices. `out` is cleared first and never grows past `expect`.
fn lzw_decode(data: &[u8], min_code: u8, expect: usize, out: &mut Vec<u8>) -> CheckResult<()> {
    out.clear();
    out.reserve(expect);
    let clear = 1usize << min_code;
    let eoi = clear + 1;
    let mut t = LzwTable::new(clear);
    let mut stack = [0u8; LzwTable::MAX];
    let mut code_size = min_code as u32 + 1;
    let mut prev: Option<usize> = None;
    let (mut acc, mut bits, mut pos) = (0u32, 0u32, 0usize);
    let bad = |code: usize, size: usize| {
        CheckError::Malformed(format!("поганий код LZW {code} (таблиця {size})"))
    };
    while out.len() < expect {
        while bits < code_size && pos < data.len() {
            acc |= (data[pos] as u32) << bits;
            bits += 8;
            pos += 1;
        }
        if bits < code_size {
            break;
        }
        let code = (acc & ((1 << code_size) - 1)) as usize;
        acc >>= code_size;
        bits -= code_size;
        if code == clear {
            code_size = min_code as u32 + 1;
            t.size = clear + 2;
            prev = None;
            continue;
        }
        if code == eoi {
            break;
        }
        match prev {
            None => {
                if code >= clear {
                    return Err(bad(code, t.size));
                }
                t.emit(code, out, expect, &mut stack);
            }
            Some(p) => {
                if code < t.size {
                    t.emit(code, out, expect, &mut stack);
                    t.add(p, t.first[code]);
                } else if code == t.size && t.size < LzwTable::MAX {
                    t.add(p, t.first[p]);
                    t.emit(code, out, expect, &mut stack);
                } else {
                    return Err(bad(code, t.size));
                }
                if t.size == (1 << code_size) && code_size < 12 {
                    code_size += 1;
                }
            }
        }
        prev = Some(code);
    }
    Ok(())
}

/// What the GIF is expected to be; `None` / `false` = not checked.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GifExpect {
    pub size: Option<(u16, u16)>,
    /// Frames and tolerance (`write_geom_test.py`: `== N`, or `≈ N/3 ± 2` at 10 fps from 30).
    pub frames: Option<(usize, usize)>,
    /// Sum of delays and tolerance, hundredths (`≈ N·100/30 ± 2`, `± 3`).
    pub total_cs: Option<(i64, i64)>,
    /// `Some(Some(0))` loop forever, `Some(None)` no `NETSCAPE2.0` (play once).
    pub loop_count: Option<Option<u16>>,
    /// Palette at most this big (`colors ≤ 64` for a 64-colour export).
    pub max_colors: Option<usize>,
    /// Composed frame `i` shows slot `i` (`bars[i] == i`) — needs [`GifOptions::bars`].
    pub bars_sequence: bool,
    /// After the full first frame, differences with transparency (`trans_frames ≥ 1`).
    pub diff_frames: bool,
}

/// All GIF checks.
pub fn check_gif(g: &GifInfo, e: &GifExpect) -> Vec<Check> {
    let mut out = vec![
        Check::new(
            "lzw",
            !g.frames.is_empty(),
            format!("{} кадрів розкодовано повністю", g.frames.len()),
        ),
        Check::new(
            "trailer",
            g.trailer,
            if g.trailer {
                "0x3B є"
            } else {
                "немає 0x3B — файл не дописано"
            },
        ),
        Check::new(
            "palette",
            (MIN_COLORS..=256).contains(&g.colors) && e.max_colors.is_none_or(|m| g.colors <= m),
            format!(
                "{} кольорів{}",
                g.colors,
                e.max_colors
                    .map_or(String::new(), |m| format!(" (не більше {m})"))
            ),
        ),
        Check::new(
            "firstFrameFull",
            g.frames
                .first()
                .is_some_and(|f| f.x == 0 && f.y == 0 && f.w == g.w && f.h == g.h),
            format!("повних кадрів {}", g.full_frames()),
        ),
        Check::new(
            "framesInside",
            g.frames_outside == 0,
            format!("{} кадрів за межами екрана", g.frames_outside),
        ),
        Check::new(
            "disposal",
            g.frames.iter().all(|f| f.disposal <= 1),
            "усі кадри «лишити попереднє» (як складає gifcheck)",
        ),
    ];
    let short = g.frames.iter().filter(|f| f.delay < MIN_DELAY_CS).count();
    out.push(Check::new(
        "minDelay",
        short == 0,
        format!("{short} кадрів із затримкою < {MIN_DELAY_CS} сотих (браузери грають їх як 10)"),
    ));
    if let Some((w, h)) = e.size {
        out.push(Check::new(
            "size",
            g.w == w && g.h == h,
            format!("{}x{} (очікується {w}x{h})", g.w, g.h),
        ));
    }
    if let Some((n, tol)) = e.frames {
        out.push(Check::new(
            "frames",
            g.frames.len().abs_diff(n) <= tol,
            format!("{} (очікується {n} ± {tol})", g.frames.len()),
        ));
    }
    if let Some((cs, tol)) = e.total_cs {
        out.push(Check::new(
            "totalCs",
            (g.total_cs() - cs).abs() <= tol,
            format!("{} сотих (очікується {cs} ± {tol})", g.total_cs()),
        ));
    }
    if let Some(l) = e.loop_count {
        let show = |l: Option<u16>| l.map_or("немає".to_string(), |v| v.to_string());
        out.push(Check::new(
            "loop",
            g.loop_count == l,
            format!("{} (очікується {})", show(g.loop_count), show(l)),
        ));
    }
    if e.bars_sequence {
        let (ok, detail) = match &g.bars {
            Some(b) => {
                let bad = b
                    .iter()
                    .enumerate()
                    .filter(|&(i, &v)| v as usize != i)
                    .count();
                (bad == 0, format!("розбіжностей {bad}"))
            }
            None => (
                false,
                "номери не читались (потрібно GifOptions::bars)".into(),
            ),
        };
        out.push(Check::new("barsBad", ok, detail));
    }
    if e.diff_frames {
        out.push(Check::new(
            "diffFrames",
            g.full_frames() >= 1 && (g.frames.len() < 2 || g.trans_frames() >= 1),
            format!("повних {}, прозорих {}", g.full_frames(), g.trans_frames()),
        ));
    }
    out
}
