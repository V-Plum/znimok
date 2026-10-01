//! GIF export (ZK-95), after Little Helpers' own encoder (§6.4 of the inventory) — WIC's GIF gave
//! a weak palette and no frame differences:
//!
//! 1. one global palette from up to 12 probe frames: a 6-bit-per-channel histogram, median cut
//!    (the heaviest box times its spread on the longest axis is split at its median), each
//!    box's colour the weighted mean; `colors − 1` colours, the last slot is transparency;
//! 2. colour matching with a cache on a 15-bit key, distance `2·dR² + 3·dG² + dB²`, optional
//!    Floyd–Steinberg dithering (7/3/5/1 of 16);
//! 3. the first frame whole; then the bounding box of what changed, unchanged pixels inside it
//!    transparent, disposal «keep»; an identical frame only lengthens the previous one's delay;
//! 4. delays in hundredths with the rounding carried (`delay_for`), so 15 fps is not 7·N.
//!
//! The container and LZW are the `gif` crate's.

use std::collections::HashMap;
use std::io::Write;

/// Delay of output frame `m` at `fps`, in hundredths of a second, with the rounding carried
/// (LH `GifDelayFor`).
pub fn delay_for(m: i64, fps: f64) -> u16 {
    let at = |k: i64| (k as f64 * 100.0 / fps.max(0.01)).round() as i64;
    (at(m + 1) - at(m)).clamp(1, u16::MAX as i64) as u16
}

/// A palette of up to `colors - 1` colours from RGBA pixels (median cut over a 6-bit histogram).
pub fn build_palette(probes: &[&[u8]], colors: usize) -> Vec<[u8; 3]> {
    let want = colors.clamp(2, 256) - 1;
    let mut hist: HashMap<u32, (u64, [u64; 3])> = HashMap::new();
    for px in probes {
        for p in px.as_chunks::<4>().0 {
            let key =
                (u32::from(p[0] >> 2) << 12) | (u32::from(p[1] >> 2) << 6) | u32::from(p[2] >> 2);
            let e = hist.entry(key).or_insert((0, [0; 3]));
            e.0 += 1;
            e.1[0] += u64::from(p[0]);
            e.1[1] += u64::from(p[1]);
            e.1[2] += u64::from(p[2]);
        }
    }
    // Each bin: count and the mean colour of its pixels.
    let bins: Vec<(u64, [f64; 3])> = hist
        .into_values()
        .map(|(n, s)| (n, s.map(|v| v as f64 / n as f64)))
        .collect();
    if bins.is_empty() {
        return vec![[0, 0, 0]];
    }
    let mut boxes: Vec<Vec<usize>> = vec![(0..bins.len()).collect()];
    while boxes.len() < want {
        // The heaviest box times its spread on the longest axis.
        let mut best: Option<(usize, usize, f64)> = None;
        for (bi, b) in boxes.iter().enumerate() {
            if b.len() < 2 {
                continue;
            }
            let weight: u64 = b.iter().map(|i| bins[*i].0).sum();
            for axis in 0..3 {
                let (mut lo, mut hi) = (f64::MAX, f64::MIN);
                for i in b {
                    lo = lo.min(bins[*i].1[axis]);
                    hi = hi.max(bins[*i].1[axis]);
                }
                let score = weight as f64 * (hi - lo);
                if best.is_none_or(|(_, _, s)| score > s) {
                    best = Some((bi, axis, score));
                }
            }
        }
        let Some((bi, axis, score)) = best else { break };
        if score <= 0.0 {
            break;
        }
        let mut b = boxes.swap_remove(bi);
        b.sort_by(|x, y| bins[*x].1[axis].total_cmp(&bins[*y].1[axis]));
        // Split at the weighted median.
        let total: u64 = b.iter().map(|i| bins[*i].0).sum();
        let mut acc = 0;
        let mut cut = 1;
        for (k, i) in b.iter().enumerate() {
            acc += bins[*i].0;
            if acc * 2 >= total {
                cut = (k + 1).clamp(1, b.len() - 1);
                break;
            }
        }
        let rest = b.split_off(cut);
        boxes.push(b);
        boxes.push(rest);
    }
    boxes
        .iter()
        .map(|b| {
            let n: u64 = b.iter().map(|i| bins[*i].0).sum::<u64>().max(1);
            let mut c = [0f64; 3];
            for i in b {
                for (k, v) in c.iter_mut().enumerate() {
                    *v += bins[*i].1[k] * bins[*i].0 as f64;
                }
            }
            c.map(|v| (v / n as f64).round().clamp(0.0, 255.0) as u8)
        })
        .collect()
}

/// Colour matching against a palette, cached on 15-bit keys.
struct Matcher {
    palette: Vec<[u8; 3]>,
    cache: HashMap<u16, u8>,
}

impl Matcher {
    fn new(palette: Vec<[u8; 3]>) -> Self {
        Self {
            palette,
            cache: HashMap::new(),
        }
    }

    fn index(&mut self, r: u8, g: u8, b: u8) -> u8 {
        let key = (u16::from(r >> 3) << 10) | (u16::from(g >> 3) << 5) | u16::from(b >> 3);
        if let Some(i) = self.cache.get(&key) {
            return *i;
        }
        let (r, g, b) = (i32::from(r), i32::from(g), i32::from(b));
        let mut best = (0usize, i32::MAX);
        for (i, c) in self.palette.iter().enumerate() {
            let (dr, dg, db) = (
                r - i32::from(c[0]),
                g - i32::from(c[1]),
                b - i32::from(c[2]),
            );
            let d = 2 * dr * dr + 3 * dg * dg + db * db;
            if d < best.1 {
                best = (i, d);
            }
        }
        self.cache.insert(key, best.0 as u8);
        best.0 as u8
    }
}

/// Palette indices of an RGBA frame, with or without Floyd–Steinberg dithering.
fn quantize(m: &mut Matcher, rgba: &[u8], w: usize, h: usize, dither: bool) -> Vec<u8> {
    let mut out = vec![0u8; w * h];
    if !dither {
        for (i, p) in rgba.as_chunks::<4>().0.iter().enumerate() {
            out[i] = m.index(p[0], p[1], p[2]);
        }
        return out;
    }
    let mut err_row = vec![[0i32; 3]; w + 2];
    let mut next_row = vec![[0i32; 3]; w + 2];
    for y in 0..h {
        for x in 0..w {
            let o = (y * w + x) * 4;
            let e = err_row[x + 1];
            let c = [
                (i32::from(rgba[o]) + e[0] / 16).clamp(0, 255),
                (i32::from(rgba[o + 1]) + e[1] / 16).clamp(0, 255),
                (i32::from(rgba[o + 2]) + e[2] / 16).clamp(0, 255),
            ];
            let i = m.index(c[0] as u8, c[1] as u8, c[2] as u8);
            out[y * w + x] = i;
            let p = m.palette[i as usize];
            for k in 0..3 {
                let d = c[k] - i32::from(p[k]);
                err_row[x + 2][k] += d * 7;
                next_row[x][k] += d * 3;
                next_row[x + 1][k] += d * 5;
                next_row[x + 2][k] += d;
            }
        }
        std::mem::swap(&mut err_row, &mut next_row);
        next_row.iter_mut().for_each(|e| *e = [0; 3]);
    }
    out
}

/// A frame waiting for its successor (an identical next frame only lengthens it).
struct Pending {
    left: u16,
    top: u16,
    width: u16,
    height: u16,
    indices: Vec<u8>,
    delay: u16,
    transparent: bool,
}

pub struct GifWriter<W: Write> {
    enc: gif::Encoder<W>,
    matcher: Matcher,
    w: usize,
    h: usize,
    dither: bool,
    transparent: u8,
    /// What the viewer shows after the frames written so far (palette indices).
    shown: Option<Vec<u8>>,
    pending: Option<Pending>,
    pub frames: usize,
}

impl<W: Write> GifWriter<W> {
    /// A looping GIF `w` × `h` with `palette` (at most 255 colours; the next index is
    /// transparency).
    pub fn new(
        out: W,
        w: u16,
        h: u16,
        palette: Vec<[u8; 3]>,
        dither: bool,
    ) -> Result<Self, String> {
        let mut palette = palette;
        palette.truncate(255);
        let transparent = palette.len() as u8;
        let mut flat: Vec<u8> = palette.iter().flatten().copied().collect();
        flat.extend_from_slice(&[0, 0, 0]);
        // The table is a power of two in size.
        let mut n = 2;
        while n < palette.len() + 1 {
            n *= 2;
        }
        flat.resize(n * 3, 0);
        let mut enc = gif::Encoder::new(out, w, h, &flat).map_err(|e| e.to_string())?;
        enc.set_repeat(gif::Repeat::Infinite)
            .map_err(|e| e.to_string())?;
        Ok(Self {
            enc,
            matcher: Matcher::new(palette),
            w: usize::from(w),
            h: usize::from(h),
            dither,
            transparent,
            shown: None,
            pending: None,
            frames: 0,
        })
    }

    /// One frame (RGBA, `w` × `h`) shown for `delay` hundredths.
    pub fn push(&mut self, rgba: &[u8], delay: u16) -> Result<(), String> {
        if rgba.len() != self.w * self.h * 4 {
            return Err("a frame of the wrong size".into());
        }
        let idx = quantize(&mut self.matcher, rgba, self.w, self.h, self.dither);
        // What will be on screen once the pending frame is shown.
        let base = match (&self.shown, &self.pending) {
            (Some(s), Some(p)) => Some(apply(s, self.w, p, self.transparent)),
            (None, Some(p)) => Some(p.indices.clone()),
            (s, None) => s.clone(),
        };
        let next = match &base {
            None => Pending {
                left: 0,
                top: 0,
                width: self.w as u16,
                height: self.h as u16,
                indices: idx,
                delay,
                transparent: false,
            },
            Some(b) => {
                // The box of what changed.
                let (mut x0, mut y0, mut x1, mut y1) = (self.w, self.h, 0usize, 0usize);
                for y in 0..self.h {
                    for x in 0..self.w {
                        if b[y * self.w + x] != idx[y * self.w + x] {
                            x0 = x0.min(x);
                            y0 = y0.min(y);
                            x1 = x1.max(x + 1);
                            y1 = y1.max(y + 1);
                        }
                    }
                }
                if x1 == 0 {
                    // Identical: the pending frame lasts longer.
                    if let Some(p) = self.pending.as_mut() {
                        p.delay = p.delay.saturating_add(delay);
                    }
                    return Ok(());
                }
                let (bw, bh) = (x1 - x0, y1 - y0);
                let mut sub = Vec::with_capacity(bw * bh);
                for y in y0..y1 {
                    for x in x0..x1 {
                        let i = y * self.w + x;
                        sub.push(if b[i] == idx[i] {
                            self.transparent
                        } else {
                            idx[i]
                        });
                    }
                }
                Pending {
                    left: x0 as u16,
                    top: y0 as u16,
                    width: bw as u16,
                    height: bh as u16,
                    indices: sub,
                    delay,
                    transparent: true,
                }
            }
        };
        self.flush()?;
        self.shown = base;
        self.pending = Some(next);
        Ok(())
    }

    fn flush(&mut self) -> Result<(), String> {
        let Some(p) = self.pending.take() else {
            return Ok(());
        };
        let f = gif::Frame {
            delay: p.delay,
            dispose: gif::DisposalMethod::Keep,
            transparent: p.transparent.then_some(self.transparent),
            left: p.left,
            top: p.top,
            width: p.width,
            height: p.height,
            buffer: std::borrow::Cow::Owned(p.indices),
            ..Default::default()
        };
        self.enc.write_frame(&f).map_err(|e| e.to_string())?;
        self.frames += 1;
        Ok(())
    }

    /// Writes the last frame and the trailer.
    pub fn finish(mut self) -> Result<W, String> {
        self.flush()?;
        self.enc.into_inner().map_err(|e| e.to_string())
    }
}

/// The screen after a (possibly partial, transparent-holed) frame over `shown`.
fn apply(shown: &[u8], w: usize, p: &Pending, transparent: u8) -> Vec<u8> {
    let mut out = shown.to_vec();
    for y in 0..usize::from(p.height) {
        for x in 0..usize::from(p.width) {
            let v = p.indices[y * usize::from(p.width) + x];
            if !(p.transparent && v == transparent) {
                out[(y + usize::from(p.top)) * w + x + usize::from(p.left)] = v;
            }
        }
    }
    out
}

/// Bytes of a GIF from these frames (one whole, the next as a difference) — the size estimate's
/// unit (LH: `est = header + 1 + avg(full) + avg(diff) · (frames − 1)`).
pub fn sizes_of_pair(
    a: &[u8],
    b: &[u8],
    w: u16,
    h: u16,
    palette: &[[u8; 3]],
    dither: bool,
) -> Result<(usize, usize, usize), String> {
    let mut g = GifWriter::new(Vec::new(), w, h, palette.to_vec(), dither)?;
    let header = {
        let v = GifWriter::new(Vec::new(), w, h, palette.to_vec(), dither)?;
        v.finish()?.len()
    };
    g.push(a, 10)?;
    g.flush()?;
    let after_full = g.enc.get_ref().len();
    g.push(b, 10)?;
    let bytes = g.finish()?.len();
    Ok((
        header,
        after_full - header,
        bytes.saturating_sub(after_full),
    ))
}

/// The estimated size of `frames` frames from probe pairs' sizes.
pub fn estimate(header: usize, full: &[usize], diff: &[usize], frames: usize) -> u64 {
    let avg = |v: &[usize]| {
        if v.is_empty() {
            0.0
        } else {
            v.iter().sum::<usize>() as f64 / v.len() as f64
        }
    };
    (header as f64 + 1.0 + avg(full) + avg(diff) * frames.saturating_sub(1) as f64) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: usize, h: usize, c: [u8; 3]) -> Vec<u8> {
        (0..w * h).flat_map(|_| [c[0], c[1], c[2], 255]).collect()
    }

    #[test]
    fn delays_carry_their_rounding() {
        let sum: u32 = (0..15).map(|m| u32::from(delay_for(m, 15.0))).sum();
        assert_eq!(sum, 100, "15 frames at 15 fps last one second");
        assert_eq!(
            delay_for(0, 30.0) + delay_for(1, 30.0) + delay_for(2, 30.0),
            10
        );
    }

    #[test]
    fn the_palette_finds_the_colours() {
        let a = solid(8, 8, [200, 30, 30]);
        let b = solid(8, 8, [30, 200, 40]);
        let p = build_palette(&[&a, &b], 16);
        assert!(p.iter().any(|c| c[0] > 180 && c[1] < 60));
        assert!(p.iter().any(|c| c[1] > 180 && c[0] < 60));
    }

    #[test]
    fn frames_round_trip_through_a_decoder() {
        let (w, h) = (16usize, 8usize);
        let red = solid(w, h, [220, 20, 20]);
        let mut half = red.clone();
        for y in 0..h {
            for x in 8..w {
                let o = (y * w + x) * 4;
                half[o..o + 3].copy_from_slice(&[20, 20, 220]);
            }
        }
        let pal = build_palette(&[&red, &half], 8);
        let mut g = GifWriter::new(Vec::new(), w as u16, h as u16, pal, false).unwrap();
        g.push(&red, 10).unwrap();
        g.push(&red, 10).unwrap(); // identical: lengthens the first
        g.push(&half, 10).unwrap();
        let bytes = g.finish().unwrap();
        let mut opts = gif::DecodeOptions::new();
        opts.set_color_output(gif::ColorOutput::RGBA);
        let mut d = opts.read_info(&bytes[..]).unwrap();
        let mut frames = Vec::new();
        while let Some(f) = d.read_next_frame().unwrap() {
            frames.push((f.delay, f.left, f.width));
        }
        assert_eq!(frames.len(), 2, "{frames:?}");
        assert_eq!(frames[0], (20, 0, 16));
        assert_eq!((frames[1].1, frames[1].2), (8, 8), "only the changed half");
        let (header, full, diff) = sizes_of_pair(
            &red,
            &half,
            w as u16,
            h as u16,
            &build_palette(&[&red], 8),
            true,
        )
        .unwrap();
        assert!(header > 0 && full > 0 && diff > 0);
        assert!(estimate(header, &[full], &[diff], 10) > (header + full) as u64);
    }
}
