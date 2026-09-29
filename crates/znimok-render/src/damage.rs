//! Partial repaint (ZK-130): which parts of the canvas changed since the last frame, and
//! rendering only those.
//!
//! A [`Tracker`] remembers what the last frame showed: a key of everything that changes the
//! whole picture (the view, the developed source, the frame, the app's own extras such as the
//! theme) and, per visible mark, a signature (its data, style, place, counter number) with the
//! box it covers on the canvas — padded for strokes, heads, shadows and glows, texts measured.
//! [`Renderer::changes`] compares the document with that: a mark that appeared, went, changed
//! or moved in z-order repaints its old and new boxes; a Hide over a changed area repaints whole
//! (its blur reaches beyond the change). Then [`Renderer::render_rects`] renders just those
//! rectangles into the canvas pixmap — the same pixels a full render gives (sub-views are
//! shifted by whole output pixels; the test below holds them equal).

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use vello_cpu::Pixmap;
use vello_cpu::kurbo::Point;
use znimok_core::{Data, Document, IRect, Kind};

use crate::{GLOW, Renderer, SHADOW, View, develop, head_len, irect, preset, rotation};

/// What to repaint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Repaint {
    /// Nothing changed.
    Nothing,
    /// These rectangles of the canvas (output pixels, inside the view).
    Rects(Vec<IRect>),
    /// Everything: the first frame, a new view, source, frame or extra key.
    All,
}

/// What the last frame showed. One per canvas.
#[derive(Default)]
pub struct Tracker {
    key: Option<u64>,
    /// Signature → (z-position, box on the canvas).
    marks: HashMap<u64, (usize, IRect)>,
}

impl Tracker {
    /// Forget the last frame: the next one repaints everything.
    pub fn reset(&mut self) {
        self.key = None;
        self.marks.clear();
    }
}

/// Tiles of this size: damage is rounded out to them, so small changes merge and every
/// rectangle starts on the same grid a full render uses.
const GRID: i32 = 16;
/// More rectangles than this merge into their bounding box.
const MAX_RECTS: usize = 12;
/// Past this share of the canvas a full repaint is cheaper.
const FULL_SHARE: f64 = 0.6;

impl Renderer {
    /// What changed on the canvas since the frame `t` remembers; updates `t` to this frame.
    /// `extra` is the caller's key for things drawn with the picture (theme, device scale).
    pub fn changes(&mut self, doc: &Document, view: View, extra: u64, t: &mut Tracker) -> Repaint {
        let key = {
            let mut h = std::hash::DefaultHasher::new();
            view.scale.to_bits().hash(&mut h);
            view.origin.x.to_bits().hash(&mut h);
            view.origin.y.to_bits().hash(&mut h);
            (view.width, view.height).hash(&mut h);
            develop::key(doc.source(), &doc.recipe).hash(&mut h);
            doc.frame().hash(&mut h);
            doc.image_size().hash(&mut h);
            extra.hash(&mut h);
            h.finish()
        };
        let mut marks = HashMap::with_capacity(doc.objects.len());
        for (i, o) in doc.objects.iter().enumerate() {
            if o.hidden {
                continue;
            }
            let mut h = std::hash::DefaultHasher::new();
            o.hash(&mut h);
            doc.counter_number(i).hash(&mut h);
            let sig = h.finish();
            let b = self.canvas_box(o, &view);
            marks.insert(sig, (i, b));
        }
        let first = t.key != Some(key);
        let prev = std::mem::replace(&mut t.marks, marks);
        t.key = Some(key);
        if first {
            return Repaint::All;
        }
        let cur = &t.marks;
        let mut damage: Vec<IRect> = Vec::new();
        for (sig, (_, b)) in prev.iter() {
            if !cur.contains_key(sig) {
                damage.push(*b);
            }
        }
        for (sig, (_, b)) in cur.iter() {
            if !prev.contains_key(sig) {
                damage.push(*b);
            }
        }
        // Marks that stayed but changed their order among the others (brought to the front,
        // moved in the layers list): the boxes of every mark in a flipped pair.
        let mut kept: Vec<(usize, usize, IRect)> = cur
            .iter()
            .filter_map(|(sig, (i, b))| prev.get(sig).map(|(j, _)| (*i, *j, *b)))
            .collect();
        kept.sort_by_key(|k| k.0);
        for (n, (_, j, b)) in kept.iter().enumerate() {
            if kept[..n].iter().any(|(_, pj, _)| pj > j)
                || kept[n + 1..].iter().any(|(_, pj, _)| pj < j)
            {
                damage.push(*b);
            }
        }
        if damage.is_empty() {
            return Repaint::Nothing;
        }
        // A Hide shows what lies below it, blurred beyond the change: repaint it whole.
        let hides: Vec<IRect> = doc
            .objects
            .iter()
            .filter(|o| !o.hidden && matches!(o.data, Data::Hide { .. }))
            .map(|o| self.canvas_box(o, &view))
            .collect();
        loop {
            let before = damage.len();
            for h in &hides {
                if !damage.contains(h) && damage.iter().any(|d| meets(*d, *h)) {
                    damage.push(*h);
                }
            }
            if damage.len() == before {
                break;
            }
        }
        let canvas = IRect::new(0, 0, view.width as i32, view.height as i32);
        let rects = merge(
            damage
                .into_iter()
                .filter_map(|r| clip(snap(r), canvas))
                .collect(),
        );
        if rects.is_empty() {
            return Repaint::Nothing;
        }
        let area: i64 = rects.iter().map(|r| r.w as i64 * r.h as i64).sum();
        if area as f64 > FULL_SHARE * canvas.w as f64 * canvas.h as f64 {
            return Repaint::All;
        }
        Repaint::Rects(rects)
    }

    /// Renders only `rects` (canvas pixels) of `doc` through `view` into `out`, which already
    /// holds the last frame at the view's size.
    pub fn render_rects(&mut self, doc: &Document, view: View, rects: &[IRect], out: &mut Pixmap) {
        if out.width() != view.width || out.height() != view.height {
            self.render(doc, view, out);
            return;
        }
        let mut tile = std::mem::replace(&mut self.scratch, Pixmap::new(1, 1));
        for r in rects {
            let sub = View {
                scale: view.scale,
                origin: Point::new(
                    view.origin.x + r.x as f64 / view.scale,
                    view.origin.y + r.y as f64 / view.scale,
                ),
                width: r.w as u16,
                height: r.h as u16,
            };
            self.render(doc, sub, &mut tile);
            let (ow, tw) = (out.width() as usize, tile.width() as usize);
            let src = tile.data();
            let dst = out.data_mut();
            for y in 0..r.h as usize {
                let d = (r.y as usize + y) * ow + r.x as usize;
                dst[d..d + tw].copy_from_slice(&src[y * tw..(y + 1) * tw]);
            }
        }
        self.scratch = tile;
    }

    /// The box a mark covers on the canvas: its bounds padded for the stroke, heads, shadow
    /// and glow (as the effects tile pads them), rotated, a text without a box measured.
    fn canvas_box(&mut self, o: &znimok_core::Object, view: &View) -> IRect {
        let b = self.covered_box(o);
        let sh = preset(&SHADOW, o.style.shadow);
        let gl = preset(&GLOW, o.style.glow);
        let extent = (sh.off + sh.blur * 3.0).max(gl.blur * 3.0) + 2.0;
        let mut pad = o.style.thick.max(0) as f64 + extent;
        if o.kind().is_segment() {
            pad += head_len(o) * 1.2;
        }
        match o.kind() {
            Kind::Text => pad += 10.0,
            Kind::Counter | Kind::Stamp => pad += b.w.max(b.h) as f64 * 0.25,
            _ => {}
        }
        let r = irect(b).inflate(pad, pad);
        let t = view.transform() * rotation(o);
        let pts = [
            t * Point::new(r.x0, r.y0),
            t * Point::new(r.x1, r.y0),
            t * Point::new(r.x0, r.y1),
            t * Point::new(r.x1, r.y1),
        ];
        let x0 = pts.iter().map(|p| p.x).fold(f64::MAX, f64::min).floor() - 2.0;
        let y0 = pts.iter().map(|p| p.y).fold(f64::MAX, f64::min).floor() - 2.0;
        let x1 = pts.iter().map(|p| p.x).fold(f64::MIN, f64::max).ceil() + 2.0;
        let y1 = pts.iter().map(|p| p.y).fold(f64::MIN, f64::max).ceil() + 2.0;
        let lim = |v: f64| v.clamp(-1e7, 1e7) as i32;
        IRect::new(lim(x0), lim(y0), lim(x1) - lim(x0), lim(y1) - lim(y0))
    }
}

fn meets(a: IRect, b: IRect) -> bool {
    a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom()
}

/// Out to the grid.
fn snap(r: IRect) -> IRect {
    let x0 = r.x.div_euclid(GRID) * GRID;
    let y0 = r.y.div_euclid(GRID) * GRID;
    let x1 = (r.right() + GRID - 1).div_euclid(GRID) * GRID;
    let y1 = (r.bottom() + GRID - 1).div_euclid(GRID) * GRID;
    IRect::new(x0, y0, x1 - x0, y1 - y0)
}

fn clip(r: IRect, to: IRect) -> Option<IRect> {
    let x0 = r.x.max(to.x);
    let y0 = r.y.max(to.y);
    let x1 = r.right().min(to.right());
    let y1 = r.bottom().min(to.bottom());
    (x1 > x0 && y1 > y0).then(|| IRect::new(x0, y0, x1 - x0, y1 - y0))
}

fn union(a: IRect, b: IRect) -> IRect {
    let x0 = a.x.min(b.x);
    let y0 = a.y.min(b.y);
    IRect::new(
        x0,
        y0,
        a.right().max(b.right()) - x0,
        a.bottom().max(b.bottom()) - y0,
    )
}

/// Overlapping or touching rectangles merge (no pixel is rendered twice); too many become one.
pub fn merge(mut rects: Vec<IRect>) -> Vec<IRect> {
    loop {
        let mut merged = false;
        'outer: for i in 0..rects.len() {
            for j in i + 1..rects.len() {
                let (a, b) = (rects[i], rects[j]);
                if a.x <= b.right() && b.x <= a.right() && a.y <= b.bottom() && b.y <= a.bottom() {
                    rects[i] = union(a, b);
                    rects.swap_remove(j);
                    merged = true;
                    break 'outer;
                }
            }
        }
        if !merged {
            break;
        }
    }
    if rects.len() > MAX_RECTS {
        let all = rects.iter().copied().reduce(union).unwrap();
        return vec![all];
    }
    rects
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reference::reference_document;

    fn full(doc: &Document, view: View) -> Vec<u8> {
        let mut r = Renderer::deterministic();
        let mut p = Pixmap::new(1, 1);
        r.render(doc, view, &mut p);
        p.data_as_u8_slice().to_vec()
    }

    fn view(doc: &Document, scale: f64) -> View {
        View {
            scale,
            origin: Point::new(37.25, 21.5),
            width: (doc.image_size().0 as f64 * scale * 0.8) as u16,
            height: (doc.image_size().1 as f64 * scale * 0.8) as u16,
        }
    }

    /// Frame 1 in full, then a change and only the damage: the canvas equals a full render
    /// of the changed document.
    fn after(change: impl Fn(&mut Document), scale: f64) -> (Repaint, usize) {
        let mut doc = reference_document(1600, 1000);
        let v = view(&doc, scale);
        let mut r = Renderer::deterministic();
        let mut t = Tracker::default();
        let mut canvas = Pixmap::new(1, 1);
        assert_eq!(r.changes(&doc, v, 0, &mut t), Repaint::All);
        r.render(&doc, v, &mut canvas);
        change(&mut doc);
        let rep = r.changes(&doc, v, 0, &mut t);
        match &rep {
            Repaint::Rects(rs) => r.render_rects(&doc, v, rs, &mut canvas),
            Repaint::All => r.render(&doc, v, &mut canvas),
            Repaint::Nothing => {}
        }
        let want = full(&doc, v);
        let got = canvas.data_as_u8_slice();
        let bad = got
            .chunks(4)
            .zip(want.chunks(4))
            .filter(|(a, b)| a.iter().zip(b.iter()).any(|(x, y)| x.abs_diff(*y) > 1))
            .count();
        (rep, bad)
    }

    fn area(r: &Repaint) -> i64 {
        match r {
            Repaint::Rects(rs) => rs.iter().map(|r| r.w as i64 * r.h as i64).sum(),
            _ => i64::MAX,
        }
    }

    #[test]
    fn nothing_changed_nothing_to_paint() {
        let (rep, bad) = after(|_| {}, 1.0);
        assert_eq!(rep, Repaint::Nothing);
        assert_eq!(bad, 0);
    }

    #[test]
    fn moving_a_mark_repaints_its_old_and_new_place_only() {
        for scale in [1.0, 0.5, 2.0, 0.37] {
            let (rep, bad) = after(|d| d.objects[0].translate(30, 12), scale);
            assert!(matches!(rep, Repaint::Rects(_)), "{scale}: {rep:?}");
            assert_eq!(
                bad, 0,
                "scale {scale}: {bad} pixels differ from a full render"
            );
        }
    }

    #[test]
    fn every_kind_moved_matches_a_full_render() {
        let n = reference_document(1600, 1000).objects.len();
        for i in 0..n {
            let (rep, bad) = after(|d| d.objects[i].translate(-17, 9), 0.75);
            assert_eq!(bad, 0, "mark {i} ({:?}): {bad} pixels differ", rep);
        }
    }

    #[test]
    fn editing_under_a_hide_repaints_the_whole_hide() {
        // A caption under the blur changes: the blur is recomputed over its whole area.
        let doc = reference_document(1600, 1000);
        let hide = doc
            .objects
            .iter()
            .position(|o| matches!(o.data, Data::Hide { .. }))
            .unwrap();
        let hb = doc.objects[hide].bounds();
        let (rep, bad) = after(
            |d| {
                let mut m =
                    znimok_core::Object::new(IRect::new(hb.x + 10, hb.y + 5, 60, 20), Data::Rect);
                m.id = 9999;
                d.objects.insert(hide, m);
            },
            1.0,
        );
        assert_eq!(bad, 0, "{rep:?}");
    }

    #[test]
    fn order_counters_and_removal_match_a_full_render() {
        // Bring the first mark to the front.
        let (_, bad) = after(
            |d| {
                let o = d.objects.remove(0);
                d.objects.push(o);
            },
            1.0,
        );
        assert_eq!(bad, 0);
        // Delete a counter: the ones after it renumber.
        let (_, bad) = after(
            |d| {
                let i = d
                    .objects
                    .iter()
                    .position(|o| matches!(o.data, Data::Counter { .. }))
                    .unwrap();
                d.objects.remove(i);
            },
            1.0,
        );
        assert_eq!(bad, 0);
        // Hide a mark with the eye.
        let (_, bad) = after(|d| d.objects[2].hidden = true, 1.0);
        assert_eq!(bad, 0);
    }

    #[test]
    fn a_small_change_is_a_small_area() {
        // The pen: a few percent of the canvas, not all of it.
        let (rep, _) = after(|d| d.objects[0].translate(4, 0), 1.0);
        let v = view(&reference_document(1600, 1000), 1.0);
        let share = area(&rep) as f64 / (v.width as f64 * v.height as f64);
        assert!(share < 0.25, "{share:.2}");
    }

    #[test]
    fn a_new_view_repaints_all() {
        let doc = reference_document(1600, 1000);
        let mut r = Renderer::deterministic();
        let mut t = Tracker::default();
        let v = view(&doc, 1.0);
        r.changes(&doc, v, 0, &mut t);
        let mut v2 = v;
        v2.origin.x += 1.0;
        assert_eq!(r.changes(&doc, v2, 0, &mut t), Repaint::All);
        assert_eq!(r.changes(&doc, v2, 1, &mut t), Repaint::All, "extra key");
        assert_eq!(r.changes(&doc, v2, 1, &mut t), Repaint::Nothing);
    }

    #[test]
    fn merge_joins_touching_and_caps_the_count() {
        let m = merge(vec![IRect::new(0, 0, 16, 16), IRect::new(16, 0, 16, 16)]);
        assert_eq!(m, vec![IRect::new(0, 0, 32, 16)]);
        let many = (0..30).map(|i| IRect::new(i * 40, 0, 16, 16)).collect();
        assert_eq!(merge(many).len(), 1);
    }
}
