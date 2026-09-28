//! Hide effects computed on the CPU in screenshot resolution (§7 п.28): the tile is built at
//! document size and only then scaled to the screen, so the file gets the same strength.
//! Formulas are Little Helpers' `EdHideRadius` / `EdHideBlock` / `EdBoxBlur`.

use znimok_core::Raster;

/// Blur radius in pixels for a tile of this size and strength (10..100 %).
pub fn blur_radius(w: u32, h: u32, strength: u8) -> u32 {
    (w.min(h) * strength.clamp(10, 100) as u32 / 600).clamp(3, 60)
}

/// Pixel block size for a tile of this size and strength.
pub fn block_size(w: u32, h: u32, strength: u8) -> u32 {
    (w.min(h) * strength.clamp(10, 100) as u32 / 500).clamp(4, 64)
}

/// Pixelate: every block becomes its average colour.
pub fn pixelate(src: &Raster, strength: u8) -> Raster {
    let block = block_size(src.width, src.height, strength);
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
                    for (a, &v) in acc.iter_mut().zip(&src.rgba[i..i + 4]) {
                        *a += v as u32;
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

/// Three box passes look Gaussian and cost linear time in pixels, not radius.
pub fn blur(src: &Raster, strength: u8) -> Raster {
    let radius = blur_radius(src.width, src.height, strength) as usize;
    let mut px = src.rgba.clone();
    box_blur_rgba(&mut px, src.width as usize, src.height as usize, radius);
    Raster::new(src.width, src.height, px)
}

/// LH `EdBoxBlur`: running sums with the window clipped at the edges (no edge replication).
pub fn box_blur_rgba(px: &mut [u8], w: usize, h: usize, radius: usize) {
    if radius < 1 || w < 2 || h < 2 {
        return;
    }
    let stride = w * 4;
    let mut tmp = vec![0u8; stride * h];
    for _ in 0..3 {
        for y in 0..h {
            let src = &px[y * stride..(y + 1) * stride];
            let dst = &mut tmp[y * stride..(y + 1) * stride];
            let mut sum = [0u32; 4];
            let mut cnt = 0u32;
            for x in 0..=radius.min(w - 1) {
                for (s, &v) in sum.iter_mut().zip(&src[x * 4..x * 4 + 4]) {
                    *s += v as u32;
                }
                cnt += 1;
            }
            for x in 0..w {
                for (d, s) in dst[x * 4..x * 4 + 4].iter_mut().zip(&sum) {
                    *d = (s / cnt) as u8;
                }
                let add = x + radius + 1;
                if add < w {
                    for (s, &v) in sum.iter_mut().zip(&src[add * 4..add * 4 + 4]) {
                        *s += v as u32;
                    }
                    cnt += 1;
                }
                if x >= radius {
                    let sub = x - radius;
                    for (s, &v) in sum.iter_mut().zip(&src[sub * 4..sub * 4 + 4]) {
                        *s -= v as u32;
                    }
                    cnt -= 1;
                }
            }
        }
        for x in 0..w {
            let mut sum = [0u32; 4];
            let mut cnt = 0u32;
            for y in 0..=radius.min(h - 1) {
                let o = y * stride + x * 4;
                for (s, &v) in sum.iter_mut().zip(&tmp[o..o + 4]) {
                    *s += v as u32;
                }
                cnt += 1;
            }
            for y in 0..h {
                let o = y * stride + x * 4;
                for (d, s) in px[o..o + 4].iter_mut().zip(&sum) {
                    *d = (s / cnt) as u8;
                }
                let add = y + radius + 1;
                if add < h {
                    let oa = add * stride + x * 4;
                    for (s, &v) in sum.iter_mut().zip(&tmp[oa..oa + 4]) {
                        *s += v as u32;
                    }
                    cnt += 1;
                }
                if y >= radius {
                    let os = (y - radius) * stride + x * 4;
                    for (s, &v) in sum.iter_mut().zip(&tmp[os..os + 4]) {
                        *s -= v as u32;
                    }
                    cnt -= 1;
                }
            }
        }
    }
}

/// Same blur for a single-channel mask (effects layers).
pub fn box_blur_alpha(m: &mut [u8], w: usize, h: usize, radius: usize) {
    if radius < 1 || w < 2 || h < 2 {
        return;
    }
    let mut tmp = vec![0u8; w * h];
    for _ in 0..3 {
        for y in 0..h {
            let src = &m[y * w..(y + 1) * w];
            let dst = &mut tmp[y * w..(y + 1) * w];
            let mut sum = 0u32;
            let mut cnt = 0u32;
            for &v in &src[..=radius.min(w - 1)] {
                sum += v as u32;
                cnt += 1;
            }
            for (x, d) in dst.iter_mut().enumerate() {
                *d = (sum / cnt) as u8;
                if x + radius + 1 < w {
                    sum += src[x + radius + 1] as u32;
                    cnt += 1;
                }
                if x >= radius {
                    sum -= src[x - radius] as u32;
                    cnt -= 1;
                }
            }
        }
        for x in 0..w {
            let mut sum = 0u32;
            let mut cnt = 0u32;
            for y in 0..=radius.min(h - 1) {
                sum += tmp[y * w + x] as u32;
                cnt += 1;
            }
            for y in 0..h {
                m[y * w + x] = (sum / cnt) as u8;
                if y + radius + 1 < h {
                    sum += tmp[(y + radius + 1) * w + x] as u32;
                    cnt += 1;
                }
                if y >= radius {
                    sum -= tmp[(y - radius) * w + x] as u32;
                    cnt -= 1;
                }
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
        assert!(out.rgba[0] > 200);
    }

    #[test]
    fn blur_of_solid_stays_solid() {
        let r = Raster::solid(32, 16, Rgb::BLUE);
        let out = blur(&r, 50);
        assert_eq!(out.rgba, r.rgba);
    }

    #[test]
    fn formulas_match_little_helpers() {
        assert_eq!(blur_radius(330, 44, 60), 4);
        assert_eq!(block_size(170, 44, 50), 4);
        assert_eq!(block_size(400, 400, 100), 64);
        assert_eq!(blur_radius(10, 10, 10), 3);
    }

    #[test]
    fn alpha_blur_spreads() {
        let mut m = vec![0u8; 9 * 9];
        m[4 * 9 + 4] = 255;
        box_blur_alpha(&mut m, 9, 9, 1);
        assert!(m[4 * 9 + 4] > 0 && m[3 * 9 + 4] > 0 && m[0] == 0);
    }
}
