//! Scrolling capture (ZK-141): frames of the same area, taken while its content scrolls down,
//! become one tall picture.
//!
//! Each row of a frame gets a signature (luma at up to 128 evenly spaced columns, hashed); rows
//! that are nearly one colour get none — blank lines match everywhere and would only mislead.
//! The shift between two frames is the one under which the most signed rows agree; a blinking
//! caret or an animation changes a few rows and does not outvote the rest. Rows that do not move
//! from one frame to the next at the top and the bottom are a sticky header and footer: they go
//! into the result once, not once per frame.

/// What one more frame did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// New rows appended at the bottom.
    Added(u32),
    /// Nothing moved: the end of the content (or the scrolling did not take).
    Same,
    /// No shift fits: the content jumped further than a frame, or it is not the same area.
    Lost,
    /// The height limit is reached; the rest is not taken.
    Full,
}

struct Frame {
    rgba: Vec<u8>,
    sigs: Vec<Option<u64>>,
}

pub struct Stitcher {
    w: u32,
    h: u32,
    /// Sticky rows at the top and the bottom (known after the first movement).
    top: Option<(u32, u32)>,
    first: Option<Frame>,
    last: Option<Frame>,
    /// Scrolled content below the header, rows of `w` RGBA pixels.
    body: Vec<u8>,
    max_height: u32,
}

impl Stitcher {
    /// `max_height`: the tallest result (the rest is not taken; the format has its limits).
    pub fn new(max_height: u32) -> Self {
        Stitcher {
            w: 0,
            h: 0,
            top: None,
            first: None,
            last: None,
            body: Vec::new(),
            max_height: max_height.max(1),
        }
    }

    /// Height of the picture so far.
    pub fn height(&self) -> u32 {
        match (self.top, &self.first) {
            (Some((t, b)), _) => t + self.body_rows() + b,
            (None, Some(_)) => self.h,
            _ => 0,
        }
    }

    fn body_rows(&self) -> u32 {
        (self.body.len() / (self.w as usize * 4).max(1)) as u32
    }

    /// One more frame, straight RGBA, the same size as the first.
    pub fn push(&mut self, w: u32, h: u32, rgba: Vec<u8>) -> Step {
        if w == 0 || h == 0 || rgba.len() != w as usize * h as usize * 4 {
            return Step::Lost;
        }
        let frame = Frame {
            sigs: signatures(w, h, &rgba),
            rgba,
        };
        let Some(prev) = self.last.as_ref() else {
            self.w = w;
            self.h = h;
            self.first = Some(Frame {
                rgba: frame.rgba.clone(),
                sigs: frame.sigs.clone(),
            });
            self.last = Some(frame);
            return Step::Added(h);
        };
        if (w, h) != (self.w, self.h) {
            return Step::Lost;
        }
        let row = w as usize * 4;
        let same_row = |y: u32| {
            let a = y as usize * row;
            prev.rgba[a..a + row] == frame.rgba[a..a + row]
        };
        if (0..h).all(same_row) {
            return Step::Same;
        }
        // Sticky rows: fixed at the first movement, at most 40 % of the frame each.
        let (top, bottom) = match self.top {
            Some(tb) => tb,
            None => {
                let cap = h * 2 / 5;
                let t = (0..cap).take_while(|y| same_row(*y)).count() as u32;
                let b = (0..cap).take_while(|i| same_row(h - 1 - i)).count() as u32;
                let first = self.first.as_ref().expect("first frame");
                // The content of the first frame between them starts the body.
                self.body = first.rgba[t as usize * row..(h - b) as usize * row].to_vec();
                self.top = Some((t, b));
                (t, b)
            }
        };
        let len = h - top - bottom;
        let Some(d) = best_shift(&prev.sigs, &frame.sigs, top, len) else {
            return Step::Lost;
        };
        if d == 0 {
            return Step::Same;
        }
        // The new rows are the last `d` of the moving part.
        let room = self
            .max_height
            .saturating_sub(top + bottom + self.body_rows());
        let take = d.min(room);
        let from = (top + len - d) as usize * row;
        self.body
            .extend_from_slice(&frame.rgba[from..from + take as usize * row]);
        self.last = Some(frame);
        if take < d || room == take {
            return Step::Full;
        }
        Step::Added(take)
    }

    /// The tall picture: the header of the first frame, the content, the footer of the last.
    pub fn finish(self) -> Option<(u32, u32, Vec<u8>)> {
        let first = self.first?;
        let Some((top, bottom)) = self.top else {
            return Some((self.w, self.h, first.rgba));
        };
        let row = self.w as usize * 4;
        let last = self.last.as_ref().unwrap_or(&first);
        let mut out = Vec::with_capacity((top + bottom) as usize * row + self.body.len());
        out.extend_from_slice(&first.rgba[..top as usize * row]);
        out.extend_from_slice(&self.body);
        out.extend_from_slice(&last.rgba[(self.h - bottom) as usize * row..]);
        let h = (out.len() / row) as u32;
        Some((self.w, h, out))
    }
}

/// Row signatures; `None` for a nearly uniform row.
fn signatures(w: u32, h: u32, rgba: &[u8]) -> Vec<Option<u64>> {
    let n = w.min(128) as usize;
    let cols: Vec<usize> = (0..n).map(|i| (i * w as usize) / n).collect();
    (0..h as usize)
        .map(|y| {
            let base = y * w as usize * 4;
            let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
            let (mut lo, mut hi) = (255u8, 0u8);
            for &x in &cols {
                let p = &rgba[base + x * 4..base + x * 4 + 3];
                let l = ((p[0] as u32 * 299 + p[1] as u32 * 587 + p[2] as u32 * 114) / 1000) as u8;
                lo = lo.min(l);
                hi = hi.max(l);
                for b in p {
                    hash ^= *b as u64;
                    hash = hash.wrapping_mul(0x0100_0000_01b3);
                }
            }
            (hi - lo >= 8).then_some(hash)
        })
        .collect()
}

/// The shift `d` (rows the content moved up) under which the most signed rows of the moving
/// part agree: new[top + j] == old[top + j + d]. `Some(0)` = not moved; `None` = nothing fits.
fn best_shift(old: &[Option<u64>], new: &[Option<u64>], top: u32, len: u32) -> Option<u32> {
    let (top, len) = (top as usize, len as usize);
    let mut best: Option<(usize, usize, usize)> = None; // (matches, compared, d)
    for d in 0..len {
        let (mut matches, mut compared) = (0usize, 0usize);
        for j in 0..len - d {
            if let (Some(a), Some(b)) = (new[top + j], old[top + j + d]) {
                compared += 1;
                if a == b {
                    matches += 1;
                }
            }
        }
        // Enough evidence, and most of it agreeing.
        if compared < 4 || matches * 10 < compared * 8 {
            continue;
        }
        if best.is_none_or(|(m, _, _)| matches > m) {
            best = Some((matches, compared, d));
        }
    }
    best.map(|(_, _, d)| d as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tall page of "text lines" (a deterministic pattern), `w` wide.
    fn page(w: u32, h: u32) -> Vec<u8> {
        let mut v = vec![250u8; (w * h * 4) as usize];
        let mut seed = 7u32;
        let mut rnd = || {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            seed >> 16
        };
        let mut y = 8;
        while y + 14 < h {
            let len = 60 + rnd() % (w - 80);
            let shade = (rnd() % 120) as u8;
            for yy in y..y + 12 {
                for x in 10..10 + len {
                    // "Letters": gaps every few pixels, varying per line.
                    if (x + yy * 3 + rnd() % 2) % 7 != 0 {
                        let i = ((yy * w + x) * 4) as usize;
                        v[i..i + 3].copy_from_slice(&[shade, shade, shade.saturating_add(40)]);
                    }
                }
            }
            y += 20 + rnd() % 14;
        }
        for p in v.chunks_mut(4) {
            p[3] = 255;
        }
        v
    }

    fn band(w: u32, h: u32, colour: [u8; 3], label: u8) -> Vec<u8> {
        let mut v = vec![0u8; (w * h * 4) as usize];
        for (i, p) in v.chunks_mut(4).enumerate() {
            let x = i as u32 % w;
            let c = if x % 50 < 20 {
                [label, label, label]
            } else {
                colour
            };
            p.copy_from_slice(&[c[0], c[1], c[2], 255]);
        }
        v
    }

    /// Frames of a viewport over `page` with a sticky header and footer, scrolled by `step`,
    /// and a blinking caret at a fixed place.
    fn frames(w: u32, page_h: u32, view: u32, step: u32) -> (Vec<Vec<u8>>, Vec<u8>) {
        let p = page(w, page_h);
        let (hh, fh) = (40u32, 30u32);
        let head = band(w, hh, [40, 60, 90], 200);
        let foot = band(w, fh, [90, 40, 40], 220);
        let body = view - hh - fh;
        let row = w as usize * 4;
        let mut out = Vec::new();
        let mut off = 0u32;
        let mut k = 0;
        loop {
            let mut f = head.clone();
            f.extend_from_slice(&p[off as usize * row..(off + body) as usize * row]);
            f.extend_from_slice(&foot);
            // Caret: a 2×14 bar at (300, 200), on every other frame.
            if k % 2 == 0 {
                for y in 200..214 {
                    for x in 300..302 {
                        let i = (y * w + x) as usize * 4;
                        f[i..i + 3].copy_from_slice(&[0, 0, 0]);
                    }
                }
            }
            out.push(f);
            k += 1;
            if off + body >= page_h {
                break;
            }
            off = (off + step).min(page_h - body);
        }
        // The page as it should come out, caret excluded (compared outside its columns).
        let mut want = head;
        want.extend_from_slice(&p);
        want.extend_from_slice(&foot);
        (out, want)
    }

    fn same_outside_caret(w: u32, got: &[u8], want: &[u8]) -> bool {
        got.len() == want.len()
            && got
                .chunks(4)
                .zip(want.chunks(4))
                .enumerate()
                .all(|(i, (a, b))| (i as u32 % w).abs_diff(301) < 4 || a == b)
    }

    #[test]
    fn stitches_a_scrolled_page_with_sticky_header_and_footer() {
        let (w, page_h) = (420u32, 2400u32);
        let (fr, want) = frames(w, page_h, 500, 170);
        let mut s = Stitcher::new(20_000);
        let mut steps = Vec::new();
        for f in fr {
            steps.push(s.push(w, 500, f));
        }
        assert!(
            steps.iter().all(|s| matches!(s, Step::Added(_))),
            "{steps:?}"
        );
        let (gw, gh, got) = s.finish().expect("picture");
        assert_eq!((gw, gh), (w, 40 + page_h + 30));
        assert!(same_outside_caret(w, &got, &want));
    }

    #[test]
    fn end_of_content_and_height_limit() {
        let (w, page_h) = (300u32, 900u32);
        let (fr, _) = frames(w, page_h, 400, 150);
        let last = fr.last().cloned().expect("frames");
        let mut s = Stitcher::new(20_000);
        for f in fr {
            s.push(w, 400, f);
        }
        // The same frame again: nothing moved.
        assert_eq!(s.push(w, 400, last), Step::Same);
        // A limit stops it.
        let (fr, _) = frames(w, page_h, 400, 150);
        let mut s = Stitcher::new(600);
        let steps: Vec<Step> = fr.into_iter().map(|f| s.push(w, 400, f)).collect();
        assert!(steps.contains(&Step::Full), "{steps:?}");
        assert!(s.height() <= 600);
    }

    #[test]
    fn a_jump_further_than_a_frame_is_lost() {
        let (w, page_h) = (300u32, 3000u32);
        let (fr, _) = frames(w, page_h, 400, 600);
        let mut s = Stitcher::new(20_000);
        s.push(w, 400, fr[0].clone());
        assert_eq!(s.push(w, 400, fr[1].clone()), Step::Lost);
    }
}
