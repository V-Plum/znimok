//! The picture as the document shows it: the source bank "developed" by the recipe —
//! mirror, quarter turns, tone. The source itself never changes (the recipe is a layer on top,
//! PLAN §5.1); every consumer (canvas, Hide regions, export, thumbnails) draws the developed
//! picture, so what you see is what the file gets.
//!
//! Formulas are Little Helpers' (`EdBuildWorking`, `EdBuildLut`), 1:1:
//! - geometry: mirror first, then `rot_quarters` clockwise (`F·R(q)·M` in `znimok-core`);
//!   pixel for pixel, no interpolation;
//! - tone: a 256-entry table per channel — exposure `× 2^EV` (clamped to white), gamma
//!   `v^(1/γ)`, contrast as a slope around mid-grey with the classic factor
//!   `259·(C+255) / (255·(259−C))`, `C = contrast · 255 / 100`. Unlike LH (which applied the
//!   table to premultiplied values and clamped to alpha) it works on straight RGB; alpha stays.

use znimok_core::{Raster, Recipe};

/// True when the recipe changes nothing (the source can be drawn as it is).
pub fn is_identity(r: &Recipe) -> bool {
    r.rot_quarters.is_multiple_of(4) && !r.mirror && tone_is_default(r)
}

pub fn tone_is_default(r: &Recipe) -> bool {
    r.exposure == 0.0 && (r.gamma - 1.0).abs() < 1e-6 && r.contrast == 0
}

/// Cache key of a developed picture: the source buffer and every recipe field.
pub fn key(src: &Raster, r: &Recipe) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |v: u64| {
        h ^= v;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    };
    mix(src.rgba.as_ptr() as u64);
    mix(src.rgba.len() as u64);
    mix(r.exposure.to_bits() as u64);
    mix(r.gamma.to_bits() as u64);
    mix(r.contrast as u64);
    mix(r.rot_quarters as u64);
    mix(r.mirror as u64);
    h
}

/// LH `EdBuildLut`.
pub fn tone_lut(r: &Recipe) -> [u8; 256] {
    let mul = 2f64.powf(r.exposure as f64);
    let gam = r.gamma as f64;
    let cc = r.contrast as f64 * 255.0 / 100.0;
    let k = (259.0 * (cc + 255.0)) / (255.0 * (259.0 - cc));
    let mut lut = [0u8; 256];
    for (i, out) in lut.iter_mut().enumerate() {
        let mut v = i as f64 / 255.0 * mul;
        if v > 1.0 {
            v = 1.0;
        }
        if (gam - 1.0).abs() > 1e-9 {
            v = v.powf(1.0 / gam);
        }
        v = (k * (v - 0.5) + 0.5).clamp(0.0, 1.0);
        *out = (v * 255.0 + 0.5) as u8;
    }
    lut
}

/// The developed picture. Returns a copy even for the identity recipe (callers that care
/// check [`is_identity`] first and draw the source directly).
pub fn develop(src: &Raster, r: &Recipe) -> Raster {
    let (sw, sh) = (src.width as usize, src.height as usize);
    let q = (r.rot_quarters % 4) as usize;
    let (dw, dh) = if q % 2 == 1 { (sh, sw) } else { (sw, sh) };
    let mut out = vec![0u8; dw * dh * 4];
    // For every destination pixel, the source pixel it comes from: undo the turn
    // (counter-clockwise), then the mirror.
    for ny in 0..dh {
        for nx in 0..dw {
            let (mut x, mut y) = (nx, ny);
            // One clockwise turn maps old (x, y) to new (h−1−y, x) with h the old height;
            // inverting: old = (new.y, w'−1−new.x) where w' is the new width.
            let mut w = dw;
            let mut hgt = dh;
            for _ in 0..q {
                let (ox, oy) = (y, w - 1 - x);
                x = ox;
                y = oy;
                std::mem::swap(&mut w, &mut hgt);
            }
            if r.mirror {
                x = sw - 1 - x;
            }
            let s = (y * sw + x) * 4;
            let d = (ny * dw + nx) * 4;
            out[d..d + 4].copy_from_slice(&src.rgba[s..s + 4]);
        }
    }
    if !tone_is_default(r) {
        let lut = tone_lut(r);
        for p in out.as_chunks_mut::<4>().0 {
            p[0] = lut[p[0] as usize];
            p[1] = lut[p[1] as usize];
            p[2] = lut[p[2] as usize];
        }
    }
    Raster::new(dw as u32, dh as u32, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 3 × 2 picture with distinct pixels: value = index.
    fn pic() -> Raster {
        let mut v = Vec::new();
        for i in 0..6u8 {
            v.extend_from_slice(&[i, i, i, 255]);
        }
        Raster::new(3, 2, v)
    }

    fn at(r: &Raster, x: usize, y: usize) -> u8 {
        r.rgba[(y * r.width as usize + x) * 4]
    }

    #[test]
    fn quarter_turn_clockwise_matches_core() {
        // Source:     0 1 2
        //             3 4 5
        // Clockwise:  3 0
        //             4 1
        //             5 2
        let r = develop(
            &pic(),
            &Recipe {
                rot_quarters: 1,
                ..Recipe::default()
            },
        );
        assert_eq!((r.width, r.height), (2, 3));
        let got: Vec<u8> = (0..3)
            .flat_map(|y| (0..2).map(move |x| (x, y)))
            .map(|(x, y)| at(&r, x, y))
            .collect();
        assert_eq!(got, vec![3, 0, 4, 1, 5, 2]);
    }

    #[test]
    fn mirror_then_turn() {
        // Mirror:  2 1 0 / 5 4 3 ; then clockwise: 5 2 / 4 1 / 3 0
        let r = develop(
            &pic(),
            &Recipe {
                rot_quarters: 1,
                mirror: true,
                ..Recipe::default()
            },
        );
        let got: Vec<u8> = (0..3)
            .flat_map(|y| (0..2).map(move |x| (x, y)))
            .map(|(x, y)| at(&r, x, y))
            .collect();
        assert_eq!(got, vec![5, 2, 4, 1, 3, 0]);
    }

    #[test]
    fn half_turn_and_identity() {
        let r = develop(
            &pic(),
            &Recipe {
                rot_quarters: 2,
                ..Recipe::default()
            },
        );
        assert_eq!(at(&r, 0, 0), 5);
        assert_eq!(at(&r, 2, 1), 0);
        assert!(is_identity(&Recipe::default()));
    }

    #[test]
    fn mirror_vertical_flips_rows() {
        let mut ed = znimok_core::Editor::new(znimok_core::Document::from_raster("t", pic()));
        ed.apply(znimok_core::Command::MirrorVertical).unwrap();
        let r = develop(ed.doc.source(), &ed.doc.recipe);
        // 0 1 2 / 3 4 5  →  3 4 5 / 0 1 2
        let got: Vec<u8> = (0..2)
            .flat_map(|y| (0..3).map(move |x| (x, y)))
            .map(|(x, y)| at(&r, x, y))
            .collect();
        assert_eq!(got, vec![3, 4, 5, 0, 1, 2]);
    }

    #[test]
    fn tone_lut_is_lh() {
        let id = tone_lut(&Recipe::default());
        assert!(id.iter().enumerate().all(|(i, v)| *v as usize == i));
        // +1 EV doubles, clamped at white.
        let up = tone_lut(&Recipe {
            exposure: 1.0,
            ..Recipe::default()
        });
        assert_eq!(up[64], 128);
        assert_eq!(up[200], 255);
        // Contrast pivots around mid-grey: 128 stays within 1.
        let c = tone_lut(&Recipe {
            contrast: 50,
            ..Recipe::default()
        });
        assert!((c[128] as i32 - 128).abs() <= 1 && c[64] < 64 && c[192] > 192);
    }
}
