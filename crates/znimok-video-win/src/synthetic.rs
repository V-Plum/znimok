//! A source that draws its slot number (the LH test build's `VidSynthFill`, the P4 clip's
//! barcode), fed through the real GPU path — the pool, the shader, the encoder — so a decoded
//! file tells which slot each frame shows and whether the colours survived.
//!
//! Layout in blocks of `width / 40` pixels (even, so a block has uniform chroma):
//! row 1 — 32 barcode blocks: bits 0..16 of the number, then the same bits inverted;
//! row 3 — 8 colour patches of 2 × 2 blocks; the rest — a gradient and a bouncing box.

use std::time::Duration;

use znimok_video::traits::{FrameSource, Pulled};
use znimok_video::{Result, VideoError};

use crate::decoder::Nv12Frame;
use crate::shader::FrameGeometry;
use crate::source::{GpuFrame, PoolFormat, SharedPool};

pub const PATCHES: [[u8; 3]; 8] = [
    [220, 30, 30],
    [30, 200, 60],
    [40, 60, 220],
    [30, 200, 210],
    [210, 40, 200],
    [230, 210, 40],
    [200, 200, 200],
    [40, 40, 40],
];
const BITS: usize = 16;

pub fn block(width: u32) -> u32 {
    ((width / 40) & !1).max(2)
}

/// RGB (full range) → Y, Cb, Cr (BT.709, limited range) — the encoder's view of a colour.
pub fn rgb_to_ycbcr(rgb: [u8; 3]) -> [u8; 3] {
    let [r, g, b] = rgb.map(|c| f64::from(c) / 255.0);
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let cb = (b - y) / 1.8556;
    let cr = (r - y) / 1.5748;
    [16.0 + 219.0 * y, 128.0 + 224.0 * cb, 128.0 + 224.0 * cr]
        .map(|v| v.round().clamp(0.0, 255.0) as u8)
}

fn bits(n: u32) -> [bool; 2 * BITS] {
    let mut v = [false; 2 * BITS];
    for (i, b) in v.iter_mut().enumerate() {
        let bit = (n >> (i % BITS)) & 1 == 1;
        *b = if i < BITS { bit } else { !bit };
    }
    v
}

/// The pattern as RGB8, tightly packed.
pub struct Pattern {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
}

impl Pattern {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            rgb: vec![0; (width * height * 3) as usize],
        }
    }

    fn fill(&mut self, x0: u32, y0: u32, w: u32, h: u32, c: [u8; 3]) {
        let (fw, fh) = (self.width, self.height);
        for y in y0..(y0 + h).min(fh) {
            for x in x0..(x0 + w).min(fw) {
                let i = ((y * fw + x) * 3) as usize;
                self.rgb[i..i + 3].copy_from_slice(&c);
            }
        }
    }

    pub fn draw(&mut self, index: u32) {
        let (w, h) = (self.width, self.height);
        let shift = index * 8;
        for y in 0..h {
            for x in 0..w {
                let t = (x + shift) % 512;
                let t = if t < 256 { t } else { 511 - t };
                let v = (40 + t * 150 / 255) as u8;
                let i = ((y * w + x) * 3) as usize;
                self.rgb[i] = v;
                self.rgb[i + 1] = v.saturating_sub(((y * 2) % 64) as u8);
                self.rgb[i + 2] = v.saturating_add(((y + shift) % 64) as u8 / 2);
            }
        }
        let blk = block(w);
        let (sx, sy) = (w.saturating_sub(4 * blk), h.saturating_sub(9 * blk));
        let bounce = |p: u32, span: u32| {
            if span == 0 {
                return 0;
            }
            let p = p % (2 * span);
            if p < span { p } else { 2 * span - p }
        };
        let bx = bounce(index * 14, sx) & !1;
        let by = 5 * blk + (bounce(index * 10, sy) & !1);
        self.fill(bx, by, 4 * blk, 4 * blk, [250, 250, 250]);
        self.fill(0, 0, w, 5 * blk, [0, 0, 0]);
        for (i, on) in bits(index).into_iter().enumerate() {
            let v = if on { 255 } else { 0 };
            self.fill(blk + i as u32 * blk, blk, blk, blk, [v, v, v]);
        }
        for (i, c) in PATCHES.iter().enumerate() {
            self.fill(blk + i as u32 * 2 * blk, 3 * blk, 2 * blk, 2 * blk, *c);
        }
    }

    /// BGRA8, rows `width × 4` bytes.
    pub fn to_bgra8(&self) -> Vec<u8> {
        self.rgb
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| [p[2], p[1], p[0], 255])
            .collect()
    }

    /// scRGB FP16 (linear, 1.0 = 80 nits) of the pattern shown at SDR white `white_nits`, so
    /// tone mode 1 maps it back to the same sRGB values; rows `width × 8` bytes.
    pub fn to_scrgb_f16(&self, white_nits: f32) -> Vec<u8> {
        let k = white_nits / 80.0;
        let mut out = Vec::with_capacity(self.rgb.len() / 3 * 8);
        for p in self.rgb.as_chunks::<3>().0 {
            for c in p {
                out.extend_from_slice(&f16(srgb_decode(*c as f32 / 255.0) * k).to_le_bytes());
            }
            out.extend_from_slice(&f16(1.0).to_le_bytes());
        }
        out
    }
}

pub fn srgb_decode(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// f32 → binary16, round to nearest.
pub fn f16(v: f32) -> u16 {
    let b = v.to_bits();
    let sign = ((b >> 16) & 0x8000) as u16;
    let exp = ((b >> 23) & 0xFF) as i32 - 127 + 15;
    let man = b & 0x7F_FFFF;
    if v.is_nan() {
        0x7E00
    } else if exp <= 0 {
        sign
    } else if exp >= 31 {
        sign | 0x7C00
    } else {
        sign | ((exp as u16) << 10) | ((man + 0x1000) >> 13) as u16
    }
}

/// The number in a decoded frame's barcode; `None` when the two halves disagree (not a pattern
/// frame, or too damaged).
pub fn read_index(f: &Nv12Frame) -> Option<u32> {
    let blk = block(f.width);
    let mut n = 0u32;
    for i in 0..2 * BITS {
        let x = blk + i as u32 * blk + blk / 2;
        let y = blk + blk / 2;
        if x >= f.width || y >= f.height {
            return None;
        }
        let on = f.luma(x, y) > 125;
        if i < BITS {
            n |= (on as u32) << i;
        } else if on == ((n >> (i - BITS)) & 1 == 1) {
            return None;
        }
    }
    Some(n)
}

/// Y, Cb, Cr at the centre of patch `i` of a decoded frame.
pub fn patch_ycc(f: &Nv12Frame, i: usize) -> [u8; 3] {
    let blk = block(f.width);
    f.ycc(blk + i as u32 * 2 * blk + blk, 3 * blk + blk)
}

/// The largest channel difference between the decoded patches and what the pattern drew.
pub fn patch_error(f: &Nv12Frame) -> u8 {
    (0..PATCHES.len())
        .map(|i| {
            let want = rgb_to_ycbcr(PATCHES[i]);
            let got = patch_ycc(f, i);
            (0..3).map(|c| want[c].abs_diff(got[c])).max().unwrap_or(0)
        })
        .max()
        .unwrap_or(255)
}

/// Feeds the pattern of each slot into the pool.
pub struct SyntheticSource {
    pool: SharedPool,
    pattern: Pattern,
    format: PoolFormat,
    white: f32,
    have: bool,
    current: Option<GpuFrame>,
    /// Pulls before this one give no frame (the wait before the first frame).
    pulls_until_frame: u32,
    pulls: u32,
}

impl SyntheticSource {
    /// `white`: the SDR white a scRGB pool pretends to have (the tone must undo it).
    pub fn new(pool: SharedPool, width: u32, height: u32, white: f32) -> Result<Self> {
        let format = pool.borrow().format;
        if format == PoolFormat::Rgb10A2 {
            return Err(VideoError::Invalid("синтетика: без 10-біт".into()));
        }
        pool.borrow_mut().ensure(width, height)?;
        Ok(Self {
            pool,
            pattern: Pattern::new(width, height),
            format,
            white,
            have: false,
            current: None,
            pulls_until_frame: 0,
            pulls: 0,
        })
    }

    pub fn first_frame_after_pulls(mut self, n: u32) -> Self {
        self.pulls_until_frame = n;
        self
    }
}

impl FrameSource for SyntheticSource {
    type Frame = GpuFrame;

    fn pull(&mut self, _wait: Duration) -> Result<Pulled> {
        self.pulls += 1;
        if self.pulls <= self.pulls_until_frame {
            return Ok(Pulled::Unchanged);
        }
        self.have = true;
        Ok(Pulled::Frame)
    }

    fn has_frame(&self) -> bool {
        self.have
    }

    fn frame_for_slot(&mut self, slot: i64) -> Result<&GpuFrame> {
        self.pattern.draw(slot.max(0) as u32);
        let (w, h) = (self.pattern.width, self.pattern.height);
        let (data, pitch, mode) = match self.format {
            PoolFormat::Rgba16F => (self.pattern.to_scrgb_f16(self.white), w * 8, 1),
            _ => (self.pattern.to_bgra8(), w * 4, 0),
        };
        let (slot_i, ready) = self.pool.borrow_mut().put_cpu(&data, pitch, w, h);
        let overlay = self.current.take().map(|c| c.overlay).unwrap_or_default();
        self.current = Some(GpuFrame {
            slot: slot_i,
            ready,
            geometry: FrameGeometry {
                mode,
                white: self.white,
                crop: (0, 0, w, h),
                out: (w, h),
            },
            overlay,
        });
        Ok(self.current.as_ref().expect("set above"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn barcode_round_trip_through_nv12() {
        let (w, h) = (640, 360);
        let mut p = Pattern::new(w, h);
        for n in [0u32, 1, 12345, 65535] {
            p.draw(n);
            // A "decoded" frame: the pattern's own luma, chroma flat.
            let y: Vec<u8> = p
                .rgb
                .as_chunks::<3>()
                .0
                .iter()
                .map(|c| rgb_to_ycbcr(*c)[0])
                .collect();
            let f = Nv12Frame {
                width: w,
                height: h,
                y,
                uv: vec![128; (w * h / 2) as usize],
            };
            assert_eq!(read_index(&f), Some(n));
        }
    }

    #[test]
    fn f16_known_values() {
        assert_eq!(f16(1.0), 0x3C00);
        assert_eq!(f16(0.5), 0x3800);
        assert_eq!(f16(-2.0), 0xC000);
        assert_eq!(f16(0.0), 0);
    }
}
