//! Pixel operations the document itself needs: baking the geometry of the recipe into a new
//! original, resizing the canvas and the image (LH §7 п.23, п.36).
//!
//! Tone (exposure, gamma, contrast) is not baked here: it is a per-pixel function that keeps
//! applying over the new original, which looks the same and keeps «Compare» free. Only the
//! quarter turns and the mirror are baked, because they change the pixel grid.

use crate::model::{Data, Document, IRect, Kind, Raster, Rgb};

/// The source after the recipe's mirror and quarter turns, i.e. exactly the displayed grid.
pub fn baked_geometry(doc: &Document) -> Raster {
    let src = doc.source();
    let (sw, sh) = (src.width as usize, src.height as usize);
    let q = doc.recipe.rot_quarters % 4;
    let (dw, dh) = if q % 2 == 1 { (sh, sw) } else { (sw, sh) };
    let mut out = vec![0u8; dw * dh * 4];
    for dy in 0..dh {
        for dx in 0..dw {
            // Displayed (dx, dy) → mirrored source (mx, my) by undoing the turn …
            let (mx, my) = match q {
                0 => (dx, dy),
                1 => (dy, sh - 1 - dx),
                2 => (sw - 1 - dx, sh - 1 - dy),
                _ => (sw - 1 - dy, dx),
            };
            // … then undoing the mirror (applied first in the recipe).
            let sx = if doc.recipe.mirror { sw - 1 - mx } else { mx };
            let s = (my * sw + sx) * 4;
            let d = (dy * dw + dx) * 4;
            out[d..d + 4].copy_from_slice(&src.rgba[s..s + 4]);
        }
    }
    Raster::new(dw as u32, dh as u32, out)
}

/// New canvas: `rect` in current image coordinates (may extend beyond the picture), the new
/// area filled with `fill` (transparent when `None`). Objects shift by the offset.
pub fn resize_canvas(doc: &mut Document, rect: IRect, fill: Option<Rgb>) -> Result<(), String> {
    let r = rect.normalized();
    if r.w < 1 || r.h < 1 || r.w > 32767 || r.h > 32767 {
        return Err(format!("canvas size {}×{} is out of range", r.w, r.h));
    }
    let img = baked_geometry(doc);
    let (w, h) = (r.w as usize, r.h as usize);
    let bg = fill.map_or([0, 0, 0, 0], |c| [c.r, c.g, c.b, 255]);
    let mut out = Vec::with_capacity(w * h * 4);
    for _ in 0..w * h {
        out.extend_from_slice(&bg);
    }
    for y in 0..h {
        let sy = y as i64 + r.y as i64;
        if sy < 0 || sy >= img.height as i64 {
            continue;
        }
        for x in 0..w {
            let sx = x as i64 + r.x as i64;
            if sx < 0 || sx >= img.width as i64 {
                continue;
            }
            let s = ((sy as usize) * img.width as usize + sx as usize) * 4;
            let d = (y * w + x) * 4;
            out[d..d + 4].copy_from_slice(&img.rgba[s..s + 4]);
        }
    }
    let bank = doc.add_bank(Raster::new(w as u32, h as u32, out));
    doc.source = bank;
    doc.recipe.rot_quarters = 0;
    doc.recipe.mirror = false;
    for o in &mut doc.objects {
        o.translate(-r.x, -r.y);
    }
    doc.crop = None;
    Ok(())
}

/// Resamples the image to `w`×`h` (Catmull-Rom bicubic). Object geometry scales with it;
/// thicknesses, font sizes and stamp diameters do not (18/2 = 9 is not in the set, §7 п.36) —
/// text sizes scale only with `scale_text`.
pub fn resize_image(doc: &mut Document, w: u32, h: u32, scale_text: bool) -> Result<(), String> {
    if w < 1 || h < 1 || w > 32767 || h > 32767 {
        return Err(format!("image size {w}×{h} is out of range"));
    }
    let img = baked_geometry(doc);
    let (sx, sy) = (w as f64 / img.width as f64, h as f64 / img.height as f64);
    let out = bicubic(&img, w, h);
    let bank = doc.add_bank(out);
    doc.source = bank;
    doc.recipe.rot_quarters = 0;
    doc.recipe.mirror = false;
    let scale = |x: i32, k: f64| (x as f64 * k).round() as i32;
    for o in &mut doc.objects {
        let kind = o.kind();
        match &mut o.data {
            Data::Pen { points } => {
                for p in points.iter_mut() {
                    *p = (scale(p.0, sx), scale(p.1, sy));
                }
                o.rect = o.bounds();
            }
            Data::Text { size, box_w, .. } => {
                let (cx, cy) = o.rect.center();
                if scale_text {
                    let k = sx.min(sy);
                    *size = ((*size as f64) * k).round().max(1.0) as i32;
                    *box_w = scale(*box_w, sx);
                    o.rect = IRect::new(
                        scale(o.rect.x, sx),
                        scale(o.rect.y, sy),
                        scale(o.rect.w, sx),
                        scale(o.rect.h, sy),
                    );
                } else {
                    // Keeps its size, moves with its centre.
                    let (w, h) = (o.rect.w, o.rect.h);
                    o.rect = IRect::new((cx * sx) as i32 - w / 2, (cy * sy) as i32 - h / 2, w, h);
                }
            }
            _ if kind.is_stamped() => {
                let (cx, cy) = o.rect.center();
                let (w, h) = (o.rect.w, o.rect.h);
                o.rect = IRect::new((cx * sx) as i32 - w / 2, (cy * sy) as i32 - h / 2, w, h);
            }
            _ => {
                let r = o.rect;
                let x0 = scale(r.x, sx);
                let y0 = scale(r.y, sy);
                o.rect = IRect::new(x0, y0, scale(r.x + r.w, sx) - x0, scale(r.y + r.h, sy) - y0);
                if kind != Kind::Line {
                    o.rect = o.rect.normalized();
                }
            }
        }
    }
    if let Some(c) = doc.crop {
        let c = c.normalized();
        let x0 = scale(c.x, sx);
        let y0 = scale(c.y, sy);
        doc.crop = Some(IRect::new(
            x0,
            y0,
            scale(c.right(), sx) - x0,
            scale(c.bottom(), sy) - y0,
        ));
    }
    Ok(())
}

/// Catmull-Rom (a = −0.5) bicubic resampling on premultiplied values, so transparent edges
/// do not bleed dark fringes.
pub fn bicubic(src: &Raster, w: u32, h: u32) -> Raster {
    let (sw, sh) = (src.width as i64, src.height as i64);
    let kernel = |t: f64| {
        let t = t.abs();
        if t < 1.0 {
            1.5 * t * t * t - 2.5 * t * t + 1.0
        } else if t < 2.0 {
            -0.5 * t * t * t + 2.5 * t * t - 4.0 * t + 2.0
        } else {
            0.0
        }
    };
    let px = |x: i64, y: i64| -> [f64; 4] {
        let x = x.clamp(0, sw - 1);
        let y = y.clamp(0, sh - 1);
        let i = ((y * sw + x) * 4) as usize;
        let a = src.rgba[i + 3] as f64 / 255.0;
        [
            src.rgba[i] as f64 * a,
            src.rgba[i + 1] as f64 * a,
            src.rgba[i + 2] as f64 * a,
            src.rgba[i + 3] as f64,
        ]
    };
    // When shrinking, widen the kernel so every source pixel contributes (box-like support).
    let (kx, ky) = (
        (sw as f64 / w as f64).max(1.0),
        (sh as f64 / h as f64).max(1.0),
    );
    let mut out = vec![0u8; (w * h * 4) as usize];
    for y in 0..h as i64 {
        let fy = (y as f64 + 0.5) * sh as f64 / h as f64 - 0.5;
        let y0 = fy.floor() as i64;
        let ry = (2.0 * ky).ceil() as i64;
        for x in 0..w as i64 {
            let fx = (x as f64 + 0.5) * sw as f64 / w as f64 - 0.5;
            let x0 = fx.floor() as i64;
            let rx = (2.0 * kx).ceil() as i64;
            let mut acc = [0.0f64; 4];
            let mut wsum = 0.0;
            for yy in y0 - ry + 1..=y0 + ry {
                let wy = kernel((yy as f64 - fy) / ky);
                if wy == 0.0 {
                    continue;
                }
                for xx in x0 - rx + 1..=x0 + rx {
                    let wx = kernel((xx as f64 - fx) / kx);
                    if wx == 0.0 {
                        continue;
                    }
                    let wgt = wx * wy;
                    let p = px(xx, yy);
                    for c in 0..4 {
                        acc[c] += p[c] * wgt;
                    }
                    wsum += wgt;
                }
            }
            let i = ((y * w as i64 + x) * 4) as usize;
            let a = (acc[3] / wsum).clamp(0.0, 255.0);
            let un = if a > 0.0 { 255.0 / a } else { 0.0 };
            for c in 0..3 {
                out[i + c] = (acc[c] / wsum * un).round().clamp(0.0, 255.0) as u8;
            }
            out[i + 3] = a.round() as u8;
        }
    }
    Raster::new(w, h, out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Head, Object};

    fn gradient(w: u32, h: u32) -> Raster {
        let mut v = Vec::new();
        for y in 0..h {
            for x in 0..w {
                v.extend_from_slice(&[x as u8, y as u8, (x + y) as u8, 255]);
            }
        }
        Raster::new(w, h, v)
    }

    #[test]
    fn baked_geometry_matches_turns_and_mirror() {
        let mut doc = Document::from_raster("t", gradient(4, 3));
        doc.recipe.rot_quarters = 1;
        let b = baked_geometry(&doc);
        assert_eq!((b.width, b.height), (3, 4));
        // Clockwise turn: displayed top-left = source bottom-left (x 0, y 2).
        assert_eq!(&b.rgba[0..2], &[0, 2]);
        doc.recipe.rot_quarters = 0;
        doc.recipe.mirror = true;
        let m = baked_geometry(&doc);
        assert_eq!(&m.rgba[0..2], &[3, 0]);
    }

    #[test]
    fn canvas_resize_moves_objects_and_keeps_old_original() {
        let mut doc = Document::from_raster("t", gradient(10, 10));
        doc.push(Object::new(
            IRect::new(2, 2, 3, 3),
            crate::model::Data::Rect,
        ));
        resize_canvas(&mut doc, IRect::new(-5, -5, 20, 20), Some(Rgb::WHITE)).unwrap();
        assert_eq!(doc.image_size(), (20, 20));
        assert_eq!(doc.objects[0].rect, IRect::new(7, 7, 3, 3));
        assert_eq!(doc.banks.len(), 2);
        assert_eq!(&doc.source().rgba[0..4], &[255, 255, 255, 255]);
        assert_eq!(
            &doc.source().rgba[(5 * 20 + 5) * 4..(5 * 20 + 5) * 4 + 2],
            &[0, 0]
        );
    }

    #[test]
    fn image_resize_scales_geometry_not_thickness() {
        let mut doc = Document::from_raster("t", gradient(100, 50));
        let mut line = Object::new(
            IRect::new(10, 10, 40, -8),
            crate::model::Data::Line {
                head_front: Head::Triangle,
                head_back: Head::None,
                head_size: 1,
            },
        );
        line.style.thick = 7;
        doc.push(line);
        resize_image(&mut doc, 50, 25, false).unwrap();
        assert_eq!(doc.image_size(), (50, 25));
        assert_eq!(doc.objects[0].rect, IRect::new(5, 5, 20, -4));
        assert_eq!(doc.objects[0].style.thick, 7);
    }

    #[test]
    fn bicubic_keeps_flat_colour_and_alpha() {
        let flat = Raster::solid(7, 5, Rgb::new(10, 200, 30));
        let up = bicubic(&flat, 20, 13);
        assert!(up.rgba.chunks(4).all(|p| p == [10, 200, 30, 255]));
        let down = bicubic(&flat, 3, 2);
        assert!(down.rgba.chunks(4).all(|p| p == [10, 200, 30, 255]));
    }
}
