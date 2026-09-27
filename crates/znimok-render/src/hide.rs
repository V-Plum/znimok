//! Hide effects computed on the CPU in screenshot resolution (§7 п.28): the tile is built at
//! document size and only then scaled to the screen, so the file gets the same strength.

use znimok_core::Raster;

/// Pixelate: block size grows with strength (10..100 %) relative to the tile size.
pub fn pixelate(src: &Raster, strength: u8) -> Raster {
    let s = strength.clamp(10, 100) as u32;
    let min_side = src.width.min(src.height).max(1);
    let block = ((min_side * s) / 400).clamp(2, 64);
    let mut out = vec![0u8; src.rgba.len()];
    let (w, h) = (src.width, src.height);
    let mut by = 0;
    while by < h {
        let mut bx = 0;
        while bx < w {
            let (bw, bh) = (block.min(w - bx), block.min(h - by));
            let mut acc = [0u32; 4];
            for y in by..by + bh {
                for x in bx..bx + bw {
                    let i = ((y * w + x) * 4) as usize;
                    for c in 0..4 {
                        acc[c] += src.rgba[i + c] as u32;
                    }
                }
            }
            let n = bw * bh;
            let avg = [
                (acc[0] / n) as u8,
                (acc[1] / n) as u8,
                (acc[2] / n) as u8,
                (acc[3] / n) as u8,
            ];
            for y in by..by + bh {
                for x in bx..bx + bw {
                    let i = ((y * w + x) * 4) as usize;
                    out[i..i + 4].copy_from_slice(&avg);
                }
            }
            bx += block;
        }
        by += block;
    }
    Raster::new(w, h, out)
}

/// Gaussian-like blur (three box passes); radius grows with strength.
pub fn blur(src: &Raster, strength: u8) -> Raster {
    let s = strength.clamp(10, 100) as u32;
    let min_side = src.width.min(src.height).max(1);
    let radius = ((min_side * s) / 600).clamp(1, 48);
    let mut a = src.rgba.clone();
    let mut b = vec![0u8; a.len()];
    for _ in 0..3 {
        box_h(&a, &mut b, src.width, src.height, radius);
        box_v(&b, &mut a, src.width, src.height, radius);
    }
    Raster::new(src.width, src.height, a)
}

fn box_h(src: &[u8], dst: &mut [u8], w: u32, h: u32, r: u32) {
    let w = w as i64;
    let r = r as i64;
    for y in 0..h as i64 {
        let row = (y * w * 4) as usize;
        let mut acc = [0i64; 4];
        let mut count = 0i64;
        for x in -r..=r {
            let xx = x.clamp(0, w - 1) as usize;
            for c in 0..4 {
                acc[c] += src[row + xx * 4 + c] as i64;
            }
            count += 1;
        }
        for x in 0..w {
            let o = row + (x as usize) * 4;
            for c in 0..4 {
                dst[o + c] = (acc[c] / count) as u8;
            }
            let add = (x + r + 1).clamp(0, w - 1) as usize;
            let sub = (x - r).clamp(0, w - 1) as usize;
            for c in 0..4 {
                acc[c] += src[row + add * 4 + c] as i64 - src[row + sub * 4 + c] as i64;
            }
        }
    }
}

fn box_v(src: &[u8], dst: &mut [u8], w: u32, h: u32, r: u32) {
    let (w, h, r) = (w as i64, h as i64, r as i64);
    for x in 0..w {
        let mut acc = [0i64; 4];
        let mut count = 0i64;
        for y in -r..=r {
            let yy = y.clamp(0, h - 1);
            let o = ((yy * w + x) * 4) as usize;
            for c in 0..4 {
                acc[c] += src[o + c] as i64;
            }
            count += 1;
        }
        for y in 0..h {
            let o = ((y * w + x) * 4) as usize;
            for c in 0..4 {
                dst[o + c] = (acc[c] / count) as u8;
            }
            let add = ((y + r + 1).clamp(0, h - 1) * w + x) as usize * 4;
            let sub = ((y - r).clamp(0, h - 1) * w + x) as usize * 4;
            for c in 0..4 {
                acc[c] += src[add + c] as i64 - src[sub + c] as i64;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use znimok_core::Rgb;

    #[test]
    fn pixelate_keeps_size_and_flattens_blocks() {
        let mut r = Raster::solid(64, 64, Rgb::WHITE);
        r.rgba[0] = 0;
        let out = pixelate(&r, 100);
        assert_eq!((out.width, out.height), (64, 64));
        // The dark pixel got averaged away into a near-white block.
        assert!(out.rgba[0] > 200);
    }

    #[test]
    fn blur_of_solid_stays_solid() {
        let r = Raster::solid(32, 16, Rgb::BLUE);
        let out = blur(&r, 50);
        assert_eq!(out.rgba, r.rgba);
    }
}
