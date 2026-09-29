//! ZK-99: `gifcheck` on GIFs written here by a port of the LH writer (`GifLzw`, `GifHeader`,
//! `GifFrame`, `GifStream`: first frame full, then the changed rectangle with transparency,
//! identical frames lengthen the previous delay, the last delay fitted to the duration) — the
//! good case, every failure, and hostile input.

use znimok_video::check::gif::*;
use znimok_video::check::{CheckError, all_ok, failed};

// ---------------------------------------------------------------------------------------------
// Writer (test side)

struct Bits {
    out: Vec<u8>,
    block: Vec<u8>,
    acc: u32,
    bits: u32,
}

impl Bits {
    fn put(&mut self, code: u32, size: u32) {
        self.acc |= code << self.bits;
        self.bits += size;
        while self.bits >= 8 {
            self.block.push(self.acc as u8);
            self.acc >>= 8;
            self.bits -= 8;
            if self.block.len() == 255 {
                self.out.push(255);
                self.out.append(&mut self.block);
            }
        }
    }
}

/// `GifLzw`: clear first, grow the code at `next == 1 << size`, clear on a full table.
fn lzw(data: &[u8], min: u8) -> Vec<u8> {
    let clear = 1u32 << min;
    let eoi = clear + 1;
    let mut size = min as u32 + 1;
    let mut next = clear + 2;
    let mut dict = std::collections::HashMap::new();
    let mut w = Bits {
        out: vec![min],
        block: Vec::new(),
        acc: 0,
        bits: 0,
    };
    w.put(clear, size);
    if data.is_empty() {
        w.put(eoi, size);
    } else {
        let mut prefix = data[0] as u32;
        for &c in &data[1..] {
            if let Some(&f) = dict.get(&(prefix, c)) {
                prefix = f;
                continue;
            }
            w.put(prefix, size);
            if next < 4096 {
                dict.insert((prefix, c), next);
                if next == (1 << size) {
                    size += 1;
                }
                next += 1;
            } else {
                w.put(clear, size);
                dict.clear();
                size = min as u32 + 1;
                next = clear + 2;
            }
            prefix = c as u32;
        }
        w.put(prefix, size);
        w.put(eoi, size);
    }
    if w.bits > 0 {
        w.block.push(w.acc as u8);
    }
    if !w.block.is_empty() {
        w.out.push(w.block.len() as u8);
        w.out.append(&mut w.block);
    }
    w.out.push(0);
    w.out
}

const PAL: [[u8; 3]; 4] = [[0, 0, 0], [255, 255, 255], [128, 128, 128], [0, 0, 0]];
const TRANS: u8 = 3;

fn header(w: u16, h: u16, bits: u8, loop_forever: bool) -> Vec<u8> {
    let mut b = b"GIF89a".to_vec();
    b.extend(w.to_le_bytes());
    b.extend(h.to_le_bytes());
    b.push(0x80 | ((bits - 1) << 4) | (bits - 1));
    b.extend([0, 0]);
    for i in 0..(1usize << bits) {
        b.extend(PAL.get(i).copied().unwrap_or([0, 0, 0]));
    }
    if loop_forever {
        b.extend([0x21, 0xFF, 0x0B]);
        b.extend(b"NETSCAPE2.0");
        b.extend([3, 1, 0, 0, 0]);
    }
    b
}

#[allow(clippy::too_many_arguments)]
fn frame(
    b: &mut Vec<u8>,
    idx: &[u8],
    x: u16,
    y: u16,
    w: u16,
    h: u16,
    delay: u16,
    trans: bool,
    disposal: u8,
) {
    b.extend([0x21, 0xF9, 4, (disposal << 2) | trans as u8]);
    b.extend(delay.to_le_bytes());
    b.extend([if trans { TRANS } else { 0 }, 0, 0x2C]);
    for v in [x, y, w, h] {
        b.extend(v.to_le_bytes());
    }
    b.push(0);
    b.extend(lzw(idx, 2));
}

/// A frame with slot number `num` in 16 bars of 64 px (white = 1), gray elsewhere.
fn bars_frame(w: usize, h: usize, num: u16) -> Vec<u8> {
    let mut v = vec![2u8; w * h];
    for y in 0..h.min(64) {
        for x in 0..w.min(1024) {
            v[y * w + x] = if num >> (x / 64) & 1 == 1 { 1 } else { 0 };
        }
    }
    v
}

struct Stream {
    out: Vec<u8>,
    w: usize,
    h: usize,
    prev: Option<Vec<u8>>,
    pend: Option<(Vec<u8>, [usize; 4], u16, bool)>,
    sum: i64,
    disposal: u8,
}

impl Stream {
    fn new(w: usize, h: usize, loop_forever: bool) -> Self {
        Self {
            out: header(w as u16, h as u16, 2, loop_forever),
            w,
            h,
            prev: None,
            pend: None,
            sum: 0,
            disposal: 1,
        }
    }

    fn flush(&mut self) {
        if let Some((idx, r, delay, trans)) = self.pend.take() {
            let [x0, y0, x1, y1] = r;
            let fw = x1 - x0;
            let sub: Vec<u8> = (y0..y1)
                .flat_map(|y| idx[y * self.w + x0..y * self.w + x1].to_vec())
                .collect();
            let (x, y, w, h) = (x0 as u16, y0 as u16, fw as u16, (y1 - y0) as u16);
            frame(&mut self.out, &sub, x, y, w, h, delay, trans, self.disposal);
            self.sum += delay as i64;
        }
    }

    /// `GifStream::Push`.
    fn push(&mut self, idx: Vec<u8>, delay: u16) {
        let Some(prev) = self.prev.take() else {
            self.pend = Some((idx.clone(), [0, 0, self.w, self.h], delay, false));
            self.prev = Some(idx);
            return;
        };
        let (mut x0, mut y0, mut x1, mut y1) = (self.w, self.h, 0, 0);
        for y in 0..self.h {
            for x in 0..self.w {
                if idx[y * self.w + x] != prev[y * self.w + x] {
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x + 1);
                    y1 = y1.max(y + 1);
                }
            }
        }
        if x1 == 0 {
            self.pend.as_mut().unwrap().2 += delay;
            self.prev = Some(prev);
            return;
        }
        self.flush();
        let mut p = idx.clone();
        for y in y0..y1 {
            for x in x0..x1 {
                if idx[y * self.w + x] == prev[y * self.w + x] {
                    p[y * self.w + x] = TRANS;
                }
            }
        }
        self.pend = Some((p, [x0, y0, x1, y1], delay, true));
        self.prev = Some(idx);
    }

    /// `GifStream::Finish(totalCs)`.
    fn finish(mut self, total_cs: i64) -> Vec<u8> {
        if let Some(p) = self.pend.as_mut() {
            let rest = total_cs - self.sum;
            if rest >= 1 {
                p.2 = rest as u16;
            }
        }
        self.flush();
        self.out.push(0x3B);
        self.out
    }
}

/// `n` frames of the bar synthetic at `fps` as the LH exporter writes them.
fn synthetic_gif(n: u16, fps: f64) -> Vec<u8> {
    let (w, h) = (1024, 48);
    let mut s = Stream::new(w, h, true);
    for m in 0..n {
        s.push(bars_frame(w, h, m), delay_for(m as u32, fps));
    }
    s.finish(total_cs_for(n as u64, fps))
}

fn bars() -> GifOptions {
    GifOptions { bars: true }
}

fn names_failed(c: &[znimok_video::check::Check]) -> Vec<&'static str> {
    failed(c).map(|c| c.name).collect()
}

fn geom_expect(n: usize, fps: f64) -> GifExpect {
    GifExpect {
        size: Some((1024, 48)),
        frames: Some((n, 0)),
        total_cs: Some(((n as f64 * 100.0 / fps) as i64, TOTAL_CS_TOL)),
        loop_count: Some(Some(0)),
        max_colors: None,
        bars_sequence: true,
        diff_frames: true,
    }
}

// ---------------------------------------------------------------------------------------------

/// `GifDelayFor`: accumulated rounding — 30 fps gives 3, 4, 3; 15 fps 7, 6, 7; the sum of `n`
/// delays is `round(n·100/fps)`.
#[test]
fn delay_schedule() {
    let d: Vec<u16> = (0..6).map(|m| delay_for(m, 30.0)).collect();
    assert_eq!(d, [3, 4, 3, 3, 4, 3]);
    let d: Vec<u16> = (0..3).map(|m| delay_for(m, 15.0)).collect();
    assert_eq!(d, [7, 6, 7]);
    for fps in [10.0, 15.0, 24.0, 25.0, 30.0, 50.0, 60.0] {
        let s: i64 = (0..97).map(|m| delay_for(m, fps) as i64).sum();
        assert_eq!(s, total_cs_for(97, fps), "{fps}");
    }
}

/// The good export: 30 frames at 30 fps — every frame decoded, its bar number in place, loop
/// forever, first frame full then differences, the sum of delays = 100 hundredths.
#[test]
fn good_gif_passes() {
    let b = synthetic_gif(30, 30.0);
    let g = parse_gif(&b, bars()).unwrap();
    assert_eq!(g.frames.len(), 30);
    assert_eq!(g.total_cs(), 100);
    assert_eq!(g.loop_count, Some(0));
    assert_eq!(g.colors, 4);
    assert_eq!(g.full_frames(), 1);
    assert_eq!(g.trans_frames(), 29);
    assert_eq!(g.bars.as_deref().unwrap(), (0..30).collect::<Vec<u16>>());
    assert_eq!(g.bytes, b.len());
    let c = check_gif(&g, &geom_expect(30, 30.0));
    assert!(all_ok(&c), "{c:#?}");
    // without bars the frames are still fully decoded, nothing composed
    let g2 = parse_gif(&b, GifOptions::default()).unwrap();
    assert!(g2.bars.is_none());
    assert_eq!(g2.delays(), g.delays());
}

/// Identical frames only lengthen the previous delay (`GifStream::Push`): fewer frames, same
/// total — the geometry test's "≈ N/3 ± 2" shape.
#[test]
fn identical_frames_merge() {
    let (w, h) = (1024, 48);
    let mut s = Stream::new(w, h, false);
    for m in 0..30u16 {
        s.push(bars_frame(w, h, m / 3), delay_for(m as u32, 30.0));
    }
    let g = parse_gif(&s.finish(100), bars()).unwrap();
    assert_eq!(g.frames.len(), 10);
    assert_eq!(g.total_cs(), 100);
    assert_eq!(g.loop_count, None);
    let e = GifExpect {
        frames: Some((10, 2)),
        total_cs: Some((100, 3)),
        loop_count: Some(None),
        max_colors: Some(64),
        ..GifExpect::default()
    };
    let c = check_gif(&g, &e);
    assert!(all_ok(&c), "{c:#?}");
}

/// Enough distinct pixels to fill the 4096-entry table: the writer clears, the reader follows.
#[test]
fn lzw_table_overflow_round_trips() {
    let (w, h) = (256, 256);
    let mut x: u32 = 12345;
    let idx: Vec<u8> = (0..w * h)
        .map(|_| {
            x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
            ((x >> 16) % 3) as u8
        })
        .collect();
    let mut b = header(w as u16, h as u16, 2, false);
    frame(&mut b, &idx, 0, 0, w as u16, h as u16, 5, false, 1);
    b.push(0x3B);
    let g = parse_gif(&b, bars()).unwrap();
    assert_eq!(g.frames.len(), 1);
}

// ---------------------------------------------------------------------------------------------
// Failures

/// 60 fps: `GifDelayFor` gives 2, 2, 1 — browsers play the 1-cs frames as 10 cs.
#[test]
fn one_cs_delays_fail() {
    let g = parse_gif(&synthetic_gif(12, 60.0), bars()).unwrap();
    assert!(g.delays().contains(&1));
    let c = check_gif(&g, &geom_expect(12, 60.0));
    assert_eq!(names_failed(&c), ["minDelay"]);
}

/// Not finished: no trailer.
#[test]
fn missing_trailer_fails() {
    let mut b = synthetic_gif(5, 30.0);
    b.pop();
    let g = parse_gif(&b, bars()).unwrap();
    assert!(!g.trailer);
    assert_eq!(
        names_failed(&check_gif(&g, &geom_expect(5, 30.0))),
        ["trailer"]
    );
}

/// Wrong expectations: size, frames, total, loop, palette cap.
#[test]
fn expectations_fail() {
    let g = parse_gif(&synthetic_gif(10, 30.0), bars()).unwrap();
    let e = GifExpect {
        size: Some((1280, 720)),
        frames: Some((12, 1)),
        total_cs: Some((40, 2)),
        loop_count: Some(None),
        max_colors: Some(2),
        bars_sequence: false,
        diff_frames: false,
    };
    let f = names_failed(&check_gif(&g, &e));
    assert_eq!(f, ["palette", "size", "frames", "totalCs", "loop"]);
}

/// Frames in the wrong order: the composed bar numbers disagree.
#[test]
fn wrong_frame_order_fails_bars() {
    let (w, h) = (1024, 48);
    let mut s = Stream::new(w, h, true);
    for m in [0u16, 1, 3, 2, 4] {
        s.push(bars_frame(w, h, m), 3);
    }
    let g = parse_gif(&s.finish(0), bars()).unwrap();
    assert_eq!(g.bars.as_deref().unwrap(), [0, 1, 3, 2, 4]);
    let mut e = geom_expect(5, 30.0);
    e.total_cs = None;
    let c = check_gif(&g, &e);
    assert_eq!(names_failed(&c), ["barsBad"]);
    // asking for bars without composing is a failure too, not a pass
    let g = parse_gif(&synthetic_gif(3, 30.0), GifOptions::default()).unwrap();
    assert_eq!(
        names_failed(&check_gif(&g, &geom_expect(3, 30.0))),
        ["barsBad"]
    );
}

/// Every frame full (no differences), a 2-colour palette, "restore background" disposal, a frame
/// past the screen, a first frame that is not full.
#[test]
fn structure_failures() {
    let (w, h) = (64usize, 40usize);
    let full = vec![1u8; w * h];
    // all full frames, disposal 2
    let mut b = header(w as u16, h as u16, 2, true);
    for _ in 0..3 {
        frame(&mut b, &full, 0, 0, w as u16, h as u16, 3, false, 2);
    }
    b.push(0x3B);
    let g = parse_gif(&b, bars()).unwrap();
    let e = GifExpect {
        diff_frames: true,
        ..GifExpect::default()
    };
    assert_eq!(names_failed(&check_gif(&g, &e)), ["disposal", "diffFrames"]);

    // 2 colours
    let mut b = header(w as u16, h as u16, 1, true);
    frame(&mut b, &full, 0, 0, w as u16, h as u16, 3, false, 1);
    b.push(0x3B);
    let g = parse_gif(&b, bars()).unwrap();
    assert_eq!(
        names_failed(&check_gif(&g, &GifExpect::default())),
        ["palette"]
    );

    // a small first frame, then one sticking out of the screen
    let mut b = header(w as u16, h as u16, 2, true);
    frame(&mut b, &[1; 16], 0, 0, 4, 4, 3, false, 1);
    frame(&mut b, &[1; 100], 60, 30, 10, 10, 3, true, 1);
    b.push(0x3B);
    let g = parse_gif(&b, bars()).unwrap();
    assert_eq!(g.frames_outside, 1);
    assert_eq!(
        names_failed(&check_gif(&g, &GifExpect::default())),
        ["firstFrameFull", "framesInside"]
    );
}

/// An interlaced frame is composed row by row in the right place.
#[test]
fn interlaced_frame_composes() {
    let (w, h) = (1024usize, 48usize);
    let img = bars_frame(w, h, 0xA5A5);
    let order: Vec<usize> = [(0, 8), (4, 8), (2, 4), (1, 2)]
        .into_iter()
        .flat_map(|(s, st)| (s..h).step_by(st))
        .collect();
    let file_rows: Vec<u8> = order
        .iter()
        .flat_map(|&y| img[y * w..(y + 1) * w].to_vec())
        .collect();
    let mut b = header(w as u16, h as u16, 2, false);
    b.extend([0x2C, 0, 0, 0, 0]);
    b.extend((w as u16).to_le_bytes());
    b.extend((h as u16).to_le_bytes());
    b.push(0x40);
    b.extend(lzw(&file_rows, 2));
    b.push(0x3B);
    let g = parse_gif(&b, bars()).unwrap();
    assert!(g.frames[0].interlace);
    assert_eq!(g.bars.unwrap(), [0xA5A5]);
}

// ---------------------------------------------------------------------------------------------
// Hostile input

#[test]
fn not_a_gif() {
    for b in [&b""[..], b"GIF8", b"PNG89a\0\0\0\0\0\0\0"] {
        assert!(matches!(
            parse_gif(b, GifOptions::default()),
            Err(CheckError::Malformed(_))
        ));
    }
}

/// Every prefix: an error or a parse, never a panic.
#[test]
fn truncated_never_panics() {
    let b = synthetic_gif(6, 30.0);
    for n in 0..b.len() {
        if let Ok(g) = parse_gif(&b[..n], bars()) {
            let _ = check_gif(&g, &geom_expect(6, 30.0));
        }
    }
}

/// Deterministic byte flips: never a panic.
#[test]
fn corrupted_never_panics() {
    let b = synthetic_gif(4, 30.0);
    let mut x: u64 = 0x2545_F491_4F6C_DD1D;
    for _ in 0..1500 {
        let mut c = b.clone();
        for _ in 0..3 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let i = (x % c.len() as u64) as usize;
            c[i] = (x >> 24) as u8;
        }
        if let Ok(g) = parse_gif(&c, bars()) {
            let _ = check_gif(&g, &GifExpect::default());
        }
    }
}

/// A 65535 × 65535 screen or frame: refused before allocating.
#[test]
fn huge_sizes_are_refused() {
    let mut b = header(65535, 65535, 2, false);
    b.push(0x3B);
    assert!(matches!(
        parse_gif(&b, bars()),
        Err(CheckError::TooLarge("екран GIF", _))
    ));
    let mut b = header(16, 16, 2, false);
    b.extend([0x2C, 0, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0xFF, 0]);
    b.extend(lzw(&[0; 4], 2));
    assert!(matches!(
        parse_gif(&b, bars()),
        Err(CheckError::TooLarge("кадр GIF", _))
    ));
}

/// LZW that decodes to fewer pixels than the frame, a code past the table, a bad minimum code
/// size, a graphic control of the wrong size, an unknown block.
#[test]
fn broken_structures_are_errors() {
    let (w, h) = (8u16, 8u16);
    let mut base = header(w, h, 2, false);
    base.extend([0x2C, 0, 0, 0, 0]);
    base.extend(w.to_le_bytes());
    base.extend(h.to_le_bytes());
    base.push(0);

    // short: 4 pixels for a 64-pixel frame
    let mut b = base.clone();
    b.extend(lzw(&[1; 4], 2));
    b.push(0x3B);
    assert!(
        matches!(parse_gif(&b, bars()), Err(CheckError::Malformed(m)) if m.contains("4 із 64"))
    );

    // first code after clear past the table: clear(4) then 7, 3-bit codes: 100 111 → 0b111100
    let mut b = base.clone();
    b.extend([2, 1, 0b0011_1100, 0, 0x3B]);
    assert!(matches!(parse_gif(&b, bars()), Err(CheckError::Malformed(m)) if m.contains("LZW")));

    // minimum code size 12
    let mut b = base.clone();
    b.extend([12, 1, 0, 0, 0x3B]);
    assert!(matches!(
        parse_gif(&b, bars()),
        Err(CheckError::Malformed(_))
    ));

    // graphic control with 2 bytes
    let mut b = header(w, h, 2, false);
    b.extend([0x21, 0xF9, 2, 0, 0, 0]);
    assert!(matches!(
        parse_gif(&b, bars()),
        Err(CheckError::Malformed(_))
    ));
    // graphic control without its terminator
    let mut b = header(w, h, 2, false);
    b.extend([0x21, 0xF9, 4, 0, 3, 0, 0, 7]);
    assert!(matches!(
        parse_gif(&b, bars()),
        Err(CheckError::Malformed(_))
    ));

    // unknown block
    let mut b = header(w, h, 2, false);
    b.push(0x99);
    assert!(matches!(parse_gif(&b, bars()), Err(CheckError::Malformed(m)) if m.contains("0x99")));

    // a sub-block that says it is longer than the file
    let mut b = header(w, h, 2, false);
    b.extend([0x21, 0xFE, 200, 1, 2]);
    assert_eq!(parse_gif(&b, bars()), Err(CheckError::Truncated("GIF")));
}
