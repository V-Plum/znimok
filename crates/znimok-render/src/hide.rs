//! Hide effects computed on the CPU in screenshot resolution (§7 п.28): the tile is built at
//! document size and only then scaled to the screen, so the file gets the same strength.
//! Formulas are Little Helpers' `EdHideRadius` / `EdHideBlock` / `EdBoxBlur`.

use znimok_core::Raster;

/// How hard a hide works (ZK-343): a share of the tile's smaller side, with a floor in pixels so a
/// line of text is never merely softened, and no ceiling but the side itself — the old formulas
/// (side × strength / 500, 4…64 px) left a 20 px line of text readable and big areas guessable.
/// At 100 % a tile is two or three blocks across its smaller side: nothing to read or guess.
fn strength_px(w: u32, h: u32, strength: u8, floor: f32, share: (f32, f32)) -> u32 {
    let t = strength.clamp(10, 100) as f32 / 100.0;
    let side = w.min(h).max(1) as f32;
    let px = (side * (share.0 + share.1 * t)).max(floor + 8.0 * t);
    (px.round() as u32).clamp(2, (side as u32).max(2))
}

/// Blur radius in pixels for a tile of this size and strength (10..100 %).
pub fn blur_radius(w: u32, h: u32, strength: u8) -> u32 {
    strength_px(w, h, strength, 3.0, (0.06, 0.40)).max(3)
}

/// Pixel block size for a tile of this size and strength.
pub fn block_size(w: u32, h: u32, strength: u8) -> u32 {
    strength_px(w, h, strength, 4.0, (0.06, 0.40)).max(4)
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

    /// ZK-343: a line of text is hidden hard (Little Helpers' formulas gave a 44 px field 4 px
    /// blocks); big areas go past the old 64 px ceiling; stronger is never weaker; never wider
    /// than the tile.
    #[test]
    fn strength_hides_text_and_big_areas() {
        assert_eq!(blur_radius(330, 44, 60), 13);
        assert_eq!(block_size(170, 44, 50), 11);
        assert_eq!(block_size(400, 400, 100), 184);
        assert_eq!(blur_radius(10, 10, 10), 4);
        // A 16 px line of text at the lowest strength still gets blocks of 5 px.
        assert!(block_size(300, 16, 10) >= 5);
        for side in [8u32, 16, 44, 200, 900] {
            let mut last = 0;
            for s in (10..=100).step_by(10) {
                let b = block_size(side * 3, side, s as u8);
                assert!(
                    b >= last && b <= side.max(4),
                    "side {side}, strength {s}: {b}"
                );
                last = b;
            }
        }
    }

    #[test]
    fn alpha_blur_spreads() {
        let mut m = vec![0u8; 9 * 9];
        m[4 * 9 + 4] = 255;
        box_blur_alpha(&mut m, 9, 9, 1);
        assert!(m[4 * 9 + 4] > 0 && m[3 * 9 + 4] > 0 && m[0] == 0);
    }
}
