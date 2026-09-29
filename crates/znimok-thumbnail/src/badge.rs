//! The video mark on a thumbnail (ZK-150): a ▶ in a dark disc at the centre and the duration in a
//! dark pill at the bottom right — how Explorer and Finder show videos, so a `.znimok` video reads
//! as a video at a glance. Drawn here, no font or vector library: a 3×5 bitmap font for the
//! digits, and coverage sampled 4×4 per pixel for smooth edges.

use image::{Rgba, RgbaImage};

/// Blends `c` with coverage `a` (0–1) over the pixel.
fn blend(img: &mut RgbaImage, x: i32, y: i32, c: [u8; 3], a: f32) {
    if a <= 0.0 || x < 0 || y < 0 || x >= img.width() as i32 || y >= img.height() as i32 {
        return;
    }
    let p = img.get_pixel_mut(x as u32, y as u32);
    let Rgba([r, g, b, pa]) = *p;
    let mix = |d: u8, s: u8| (d as f32 * (1.0 - a) + s as f32 * a).round() as u8;
    let out_a = (pa as f32 + (255.0 - pa as f32) * a).round() as u8;
    *p = Rgba([mix(r, c[0]), mix(g, c[1]), mix(b, c[2]), out_a]);
}

/// Fills where `inside(x, y)` holds (pixel-space floats), `alpha` times the covered share.
fn fill(
    img: &mut RgbaImage,
    bbox: (f32, f32, f32, f32),
    c: [u8; 3],
    alpha: f32,
    inside: impl Fn(f32, f32) -> bool,
) {
    let (x0, y0, x1, y1) = bbox;
    for y in (y0.floor() as i32)..=(y1.ceil() as i32) {
        for x in (x0.floor() as i32)..=(x1.ceil() as i32) {
            let mut hit = 0;
            for sy in 0..4 {
                for sx in 0..4 {
                    let (px, py) = (
                        x as f32 + (sx as f32 + 0.5) / 4.0,
                        y as f32 + (sy as f32 + 0.5) / 4.0,
                    );
                    hit += inside(px, py) as u32;
                }
            }
            blend(img, x, y, c, alpha * hit as f32 / 16.0);
        }
    }
}

/// 3×5 digits and the colon, rows top to bottom, bit 2 = left column.
const GLYPHS: [(char, [u8; 5]); 11] = [
    ('0', [0b111, 0b101, 0b101, 0b101, 0b111]),
    ('1', [0b010, 0b110, 0b010, 0b010, 0b111]),
    ('2', [0b111, 0b001, 0b111, 0b100, 0b111]),
    ('3', [0b111, 0b001, 0b111, 0b001, 0b111]),
    ('4', [0b101, 0b101, 0b111, 0b001, 0b001]),
    ('5', [0b111, 0b100, 0b111, 0b001, 0b111]),
    ('6', [0b111, 0b100, 0b111, 0b101, 0b111]),
    ('7', [0b111, 0b001, 0b010, 0b010, 0b010]),
    ('8', [0b111, 0b101, 0b111, 0b101, 0b111]),
    ('9', [0b111, 0b101, 0b111, 0b001, 0b111]),
    (':', [0b000, 0b010, 0b000, 0b010, 0b000]),
];

/// `m:ss`, or `h:mm:ss` from an hour.
pub fn duration_text(hns: i64) -> String {
    let s = (hns.max(0) + 5_000_000) / 10_000_000;
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// Draws the ▶ and the duration onto a thumbnail.
pub fn draw(img: &mut RgbaImage, duration_hns: i64) {
    let (w, h) = (img.width() as f32, img.height() as f32);
    let short = w.min(h);
    if short < 12.0 {
        return;
    }
    // ▶ in a dark disc, centred.
    let r = (short * 0.2).max(6.0);
    let (cx, cy) = (w / 2.0, h / 2.0);
    fill(
        img,
        (cx - r, cy - r, cx + r, cy + r),
        [0, 0, 0],
        0.55,
        |x, y| (x - cx).powi(2) + (y - cy).powi(2) <= r * r,
    );
    // A triangle pointing right, its centre of mass on the disc's centre.
    let t = r * 0.55;
    let (ax, ay) = (cx - t * 0.6, cy - t);
    let (bx, by) = (cx - t * 0.6, cy + t);
    let (qx, qy) = (cx + t * 1.0, cy);
    let edge = |x0: f32, y0: f32, x1: f32, y1: f32, x: f32, y: f32| {
        (x1 - x0) * (y - y0) - (y1 - y0) * (x - x0)
    };
    fill(img, (ax, ay, qx, by), [255, 255, 255], 0.95, |x, y| {
        let (e1, e2, e3) = (
            edge(ax, ay, qx, qy, x, y),
            edge(qx, qy, bx, by, x, y),
            edge(bx, by, ax, ay, x, y),
        );
        (e1 >= 0.0 && e2 >= 0.0 && e3 >= 0.0) || (e1 <= 0.0 && e2 <= 0.0 && e3 <= 0.0)
    });
    // The duration, bottom right, when there is room for it.
    // Below ~90 px the digits would be a smudge: the ▶ alone says «video».
    if short < 90.0 || duration_hns <= 0 {
        return;
    }
    let text = duration_text(duration_hns);
    let px = (short / 48.0).floor().max(2.0); // size of one font pixel
    let glyph_w = |c: char| if c == ':' { 1.0 } else { 3.0 };
    let tw: f32 = text.chars().map(|c| (glyph_w(c) + 1.0) * px).sum::<f32>() - px;
    let th = 5.0 * px;
    let pad = 2.0 * px;
    let margin = (short * 0.04).max(2.0);
    let (x1, y1) = (w - margin, h - margin);
    let (x0, y0) = (x1 - tw - 2.0 * pad, y1 - th - 2.0 * pad);
    let rad = (th + 2.0 * pad) * 0.35;
    fill(img, (x0, y0, x1, y1), [0, 0, 0], 0.65, |x, y| {
        // Rounded rectangle.
        let dx = (x0 + rad - x).max(0.0).max(x - (x1 - rad));
        let dy = (y0 + rad - y).max(0.0).max(y - (y1 - rad));
        dx * dx + dy * dy <= rad * rad
    });
    let mut gx = x0 + pad;
    for c in text.chars() {
        let rows = GLYPHS
            .iter()
            .find(|g| g.0 == c)
            .map(|g| g.1)
            .unwrap_or([0; 5]);
        let cols = if c == ':' { 1 } else { 3 };
        for (ry, bits) in rows.iter().enumerate() {
            for cx_ in 0..cols {
                let bit = if c == ':' {
                    (bits >> 1) & 1
                } else {
                    (bits >> (2 - cx_)) & 1
                };
                if bit == 1 {
                    let (sx, sy) = (gx + cx_ as f32 * px, y0 + pad + ry as f32 * px);
                    fill(
                        img,
                        (sx, sy, sx + px - 0.01, sy + px - 0.01),
                        [255, 255, 255],
                        1.0,
                        |x, y| x >= sx && x < sx + px && y >= sy && y < sy + px,
                    );
                }
            }
        }
        gx += (glyph_w(c) + 1.0) * px;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations() {
        assert_eq!(duration_text(0), "0:00");
        assert_eq!(duration_text(30_000_000), "0:03");
        assert_eq!(duration_text(754 * 10_000_000), "12:34");
        assert_eq!(duration_text((3600 + 62) * 10_000_000), "1:01:02");
    }

    #[test]
    fn marks_the_centre_and_the_corner() {
        let mut img = RgbaImage::from_pixel(160, 100, Rgba([255, 255, 255, 255]));
        draw(&mut img, 83 * 10_000_000);
        // The disc darkens around the ▶; the ▶ itself stays light.
        let disc = img.get_pixel(80 - 16, 50).0;
        assert!(disc[0] < 180, "{disc:?}");
        let tri = img.get_pixel(81, 50).0;
        assert!(tri[0] > 200, "{tri:?}");
        // The duration pill in the bottom right corner is dark; the top left is untouched.
        let pill = img.get_pixel(160 - 8, 100 - 6).0;
        assert!(pill[0] < 200, "{pill:?}");
        assert_eq!(img.get_pixel(2, 2).0, [255, 255, 255, 255]);
        // Tiny thumbnails get no mark rather than a smudge.
        let mut tiny = RgbaImage::from_pixel(10, 10, Rgba([255, 255, 255, 255]));
        draw(&mut tiny, 10);
        assert!(tiny.pixels().all(|p| p.0 == [255, 255, 255, 255]));
    }
}

/// `ZNIMOK_BADGE_PREVIEW=<dir>`: writes the mark on thumbnail-sized pictures, to look at.
#[cfg(test)]
#[test]
fn preview_when_asked() {
    let Some(dir) = std::env::var_os("ZNIMOK_BADGE_PREVIEW") else {
        return;
    };
    for (w, h, dur) in [
        (96, 60, 83),
        (160, 100, 83),
        (256, 160, 754),
        (320, 200, 3662),
    ] {
        let mut img = RgbaImage::from_fn(w, h, |x, y| {
            Rgba([(40 + x * 180 / w) as u8, (90 + y * 120 / h) as u8, 200, 255])
        });
        draw(&mut img, dur * 10_000_000);
        img.save(std::path::Path::new(&dir).join(format!("badge-{w}x{h}.png")))
            .unwrap();
    }
}
