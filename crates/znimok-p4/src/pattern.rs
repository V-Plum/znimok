//! The synthetic test clip: every frame carries its own index as a barcode and a row of known
//! colour patches, so a decoded frame can be checked without any reference file. Layout is in
//! blocks of `blk` pixels (width / 40, even), all aligned to 2×2 so NV12 chroma stays uniform
//! inside a block.
//!
//!   row 1: 32 barcode blocks — bits 0..16 of the index, then the same 16 bits inverted
//!   row 3: 8 colour patches of 2×2 blocks (BT.709, limited range)
//!   the rest: a scrolling gradient and a bouncing box, so the encoder has real motion to code

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
    (width / 40) & !1
}

/// RGB (0..255, full range) → Y, Cb, Cr (BT.709, limited range) — the encoder's view of a colour.
pub fn rgb_to_ycbcr(rgb: [u8; 3]) -> [u8; 3] {
    let [r, g, b] = rgb.map(|c| f64::from(c) / 255.0);
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let cb = (b - y) / 1.8556;
    let cr = (r - y) / 1.5748;
    [16.0 + 219.0 * y, 128.0 + 224.0 * cb, 128.0 + 224.0 * cr]
        .map(|v| v.round().clamp(0.0, 255.0) as u8)
}

/// An NV12 frame in CPU memory, tightly packed (stride = width).
pub struct Nv12 {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl Nv12 {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            data: vec![0; (width * height * 3 / 2) as usize],
        }
    }

    /// Fill a rectangle (even coordinates) with one colour given as Y, Cb, Cr.
    fn fill(&mut self, x0: u32, y0: u32, w: u32, h: u32, ycc: [u8; 3]) {
        let (fw, fh) = (self.width, self.height);
        let (x1, y1) = ((x0 + w).min(fw), (y0 + h).min(fh));
        for y in y0..y1 {
            let row = (y * fw) as usize;
            self.data[row + x0 as usize..row + x1 as usize].fill(ycc[0]);
        }
        let uv0 = (fw * fh) as usize;
        for y in y0 / 2..y1 / 2 {
            let row = uv0 + (y * fw) as usize;
            for x in x0 / 2..x1 / 2 {
                self.data[row + 2 * x as usize] = ycc[1];
                self.data[row + 2 * x as usize + 1] = ycc[2];
            }
        }
    }

    /// Draw frame number `index` of a clip into this buffer (overwrites everything).
    pub fn draw(&mut self, index: u32) {
        let (w, h) = (self.width, self.height);
        let uv0 = (w * h) as usize;
        let shift = index * 8;
        // Scrolling gradient: luma ramps along x, chroma drifts slowly along y.
        for y in 0..h {
            let row = (y * w) as usize;
            for x in 0..w {
                let t = (x + shift) % 512;
                let t = if t < 256 { t } else { 511 - t };
                self.data[row + x as usize] = (40 + t * 150 / 255) as u8;
            }
        }
        for y in 0..h / 2 {
            let row = uv0 + (y * w) as usize;
            let cb = 110 + ((y * 2 + shift) % 64) as u8 / 2;
            let cr = 150 - ((y * 2) % 64) as u8 / 2;
            for x in 0..w / 2 {
                self.data[row + 2 * x as usize] = cb;
                self.data[row + 2 * x as usize + 1] = cr;
            }
        }
        let blk = block(w);
        // Bouncing box (4×4 blocks), 7 px per frame both ways.
        let (sx, sy) = (w - 4 * blk, h - 4 * blk - 5 * blk);
        let bounce = |p: u32, span: u32| {
            let p = p % (2 * span);
            if p < span { p } else { 2 * span - p }
        };
        let bx = bounce(index * 14, sx) & !1;
        let by = 5 * blk + (bounce(index * 10, sy) & !1);
        self.fill(bx, by, 4 * blk, 4 * blk, rgb_to_ycbcr([250, 250, 250]));
        // Barcode and patches on a dark band so they never sit on the moving content.
        self.fill(0, 0, w, 5 * blk, [16, 128, 128]);
        for (i, on) in bits(index).into_iter().enumerate() {
            let v = if on { 235 } else { 16 };
            self.fill(blk + i as u32 * blk, blk, blk, blk, [v, 128, 128]);
        }
        for (i, c) in PATCHES.iter().enumerate() {
            self.fill(
                blk + i as u32 * 2 * blk,
                3 * blk,
                2 * blk,
                2 * blk,
                rgb_to_ycbcr(*c),
            );
        }
    }
}

fn bits(index: u32) -> [bool; 2 * BITS] {
    let mut out = [false; 2 * BITS];
    for i in 0..BITS {
        out[i] = (index >> i) & 1 == 1;
        out[BITS + i] = !out[i];
    }
    out
}

/// What a decoded frame says about itself.
pub struct Reading {
    /// Frame index from the barcode, `None` when the inverted half does not match (damaged frame).
    pub index: Option<u32>,
    /// Largest channel difference over the patch centres against `PATCHES`.
    pub patch_max_diff: u8,
}

/// Read the barcode and patches from RGBA8 rows `0..5*blk` of a frame `width` wide.
pub fn read(rgba: &[u8], width: u32) -> Reading {
    let blk = block(width);
    let px = |x: u32, y: u32| {
        let o = ((y * width + x) * 4) as usize;
        [rgba[o], rgba[o + 1], rgba[o + 2]]
    };
    let mut got = [false; 2 * BITS];
    for (i, g) in got.iter_mut().enumerate() {
        let p = px(blk + i as u32 * blk + blk / 2, blk + blk / 2);
        *g = u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2]) > 3 * 128;
    }
    let valid = (0..BITS).all(|i| got[i] != got[BITS + i]);
    let index = valid.then(|| (0..BITS).fold(0u32, |a, i| a | (u32::from(got[i]) << i)));
    let mut worst = 0u8;
    for (i, c) in PATCHES.iter().enumerate() {
        let p = px(blk + i as u32 * 2 * blk + blk, 4 * blk);
        for k in 0..3 {
            worst = worst.max(p[k].abs_diff(c[k]));
        }
    }
    Reading {
        index,
        patch_max_diff: worst,
    }
}

/// Rows of the frame that `read` needs.
pub fn band_rows(width: u32) -> u32 {
    5 * block(width)
}

/// The shader's conversion, on the CPU (BT.709 limited range → RGB8), for tests.
pub fn ycbcr_to_rgb(ycc: [u8; 3]) -> [u8; 3] {
    let y = (f64::from(ycc[0]) - 16.0) / 219.0;
    let cb = (f64::from(ycc[1]) - 128.0) / 224.0;
    let cr = (f64::from(ycc[2]) - 128.0) / 224.0;
    let r = y + 1.5748 * cr;
    let g = y - 0.187_324 * cb - 0.468_124 * cr;
    let b = y + 1.8556 * cb;
    [r, g, b].map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// Convert a CPU NV12 frame to RGBA8 with `ycbcr_to_rgb` (nearest chroma, as the shader does).
pub fn nv12_to_rgba(f: &Nv12) -> Vec<u8> {
    let (w, h) = (f.width as usize, f.height as usize);
    let mut out = vec![255u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let uv = w * h + (y / 2) * w + (x / 2) * 2;
            let c = ycbcr_to_rgb([f.data[y * w + x], f.data[uv], f.data[uv + 1]]);
            out[(y * w + x) * 4..(y * w + x) * 4 + 3].copy_from_slice(&c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_survive_the_round_trip() {
        for c in PATCHES {
            let back = ycbcr_to_rgb(rgb_to_ycbcr(c));
            for k in 0..3 {
                assert!(back[k].abs_diff(c[k]) <= 2, "{c:?} -> {back:?}");
            }
        }
    }

    #[test]
    fn barcode_reads_back_on_4k_and_1080p() {
        for (w, h) in [(3840, 2160), (1920, 1080)] {
            let mut f = Nv12::new(w, h);
            for idx in [0, 1, 59, 60, 1199, 40_000] {
                f.draw(idx);
                let r = read(&nv12_to_rgba(&f), w);
                assert_eq!(r.index, Some(idx));
                assert!(r.patch_max_diff <= 2, "patches {}", r.patch_max_diff);
            }
        }
    }

    #[test]
    fn damaged_barcode_is_rejected() {
        let (w, h) = (1920, 1080);
        let mut f = Nv12::new(w, h);
        f.draw(5);
        let mut rgba = nv12_to_rgba(&f);
        let blk = block(w);
        // Paint bit 16 (inverse of bit 0) the same as bit 0.
        let y = blk + blk / 2;
        let x = blk + 16 * blk + blk / 2;
        let o = ((y * w + x) * 4) as usize;
        rgba[o..o + 3].copy_from_slice(&[255, 255, 255]);
        assert_eq!(read(&rgba, w).index, None);
    }
}
