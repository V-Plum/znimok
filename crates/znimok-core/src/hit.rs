//! Picking and handles (LH `EdPick`, `EdHitObject`, `EdHandles`; inventory §2.7).
//!
//! Everything here is in screenshot coordinates. `px_per_doc` is how many screen pixels one
//! screenshot pixel takes at the current zoom: tolerances are screen-constant, so they are
//! divided by it.

use crate::model::{Data, Document, IRect, Kind, Object};

/// Distance from `p` to the segment `a–b`.
pub fn dist_to_segment(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (abx, aby) = (b.0 - a.0, b.1 - a.1);
    let len2 = abx * abx + aby * aby;
    if len2 == 0.0 {
        return ((p.0 - a.0).powi(2) + (p.1 - a.1).powi(2)).sqrt();
    }
    let t = (((p.0 - a.0) * abx + (p.1 - a.1) * aby) / len2).clamp(0.0, 1.0);
    let (qx, qy) = (a.0 + abx * t, a.1 + aby * t);
    ((p.0 - qx).powi(2) + (p.1 - qy).powi(2)).sqrt()
}

/// The point turned back by the object's rotation around its centre, so a rotated box can be
/// tested as an axis-aligned one.
pub fn unrotate(p: (f64, f64), b: IRect, rot: u16) -> (f64, f64) {
    turn(p, b.center(), -(rot as f64))
}

/// `p` turned clockwise by `deg` degrees about `c` (y grows downwards, as on screen).
pub fn turn(p: (f64, f64), c: (f64, f64), deg: f64) -> (f64, f64) {
    let a = deg.to_radians();
    let (dx, dy) = (p.0 - c.0, p.1 - c.1);
    (
        c.0 + dx * a.cos() - dy * a.sin(),
        c.1 + dx * a.sin() + dy * a.cos(),
    )
}

/// Whether the object is drawn turned (its kind can turn and it has an angle).
pub fn turned(o: &Object) -> bool {
    o.kind().can_rotate() && o.rot != 0
}

/// Where the rotation handle stands (ZK-164, LH `EdRotHandle`): `stem` screen pixels above the
/// middle of the top edge, turned with the mark. None for kinds that do not turn.
pub fn rotation_handle(o: &Object, px_per_doc: f64) -> Option<(f64, f64)> {
    if !o.kind().can_rotate() {
        return None;
    }
    let b = o.bounds();
    let (cx, _) = b.center();
    let p = (cx, b.y as f64 - ROTATION_STEM / px_per_doc.max(1e-6));
    Some(if turned(o) {
        turn(p, b.center(), o.rot as f64)
    } else {
        p
    })
}

/// Length of the rotation handle's stem, screen pixels.
pub const ROTATION_STEM: f64 = 24.0;

/// Whether `p` is on the rotation handle (a screen-constant 7 px radius).
pub fn hit_rotation_handle(o: &Object, p: (f64, f64), px_per_doc: f64) -> bool {
    let k = px_per_doc.max(1e-6);
    rotation_handle(o, k)
        .is_some_and(|(x, y)| ((x - p.0).powi(2) + (y - p.1).powi(2)).sqrt() <= 7.0 / k)
}

/// The angle of `p` seen from `c`, in degrees clockwise from straight up (the handle's
/// direction when the mark is not turned).
pub fn angle_from_up(c: (f64, f64), p: (f64, f64)) -> f64 {
    (p.0 - c.0).atan2(c.1 - p.1).to_degrees()
}

/// Whether `p` hits the object: segments by distance ≤ 4 screen px + half the thickness;
/// everything else by its whole box grown by 3 screen px.
pub fn hits(o: &Object, p: (f64, f64), px_per_doc: f64) -> bool {
    let k = px_per_doc.max(1e-6);
    // A turned mark is tested in its own frame: the point is turned back (lines and pens too —
    // they are drawn turned about their box centre, ZK-164).
    let p = if turned(o) {
        unrotate(p, o.bounds(), o.rot)
    } else {
        p
    };
    match &o.data {
        Data::Line { .. } => {
            let r = o.rect;
            let a = (r.x as f64, r.y as f64);
            let b = ((r.x + r.w) as f64, (r.y + r.h) as f64);
            dist_to_segment(p, a, b) <= 4.0 / k + o.style.thick as f64 / 2.0
        }
        Data::Pen { points, .. } => {
            if points.len() == 1 {
                let q = (points[0].0 as f64, points[0].1 as f64);
                return dist_to_segment(p, q, q) <= 4.0 / k + o.style.thick as f64 / 2.0;
            }
            points.windows(2).any(|w| {
                let a = (w[0].0 as f64, w[0].1 as f64);
                let b = (w[1].0 as f64, w[1].1 as f64);
                dist_to_segment(p, a, b) <= 4.0 / k + o.style.thick as f64 / 2.0
            })
        }
        _ => {
            let b = o.bounds();
            let q = p;
            let slack = 3.0 / k;
            q.0 >= b.x as f64 - slack
                && q.1 >= b.y as f64 - slack
                && q.0 <= b.right() as f64 + slack
                && q.1 <= b.bottom() as f64 + slack
        }
    }
}

/// Top-most visible object under `p`.
pub fn pick(doc: &Document, p: (f64, f64), px_per_doc: f64) -> Option<usize> {
    doc.objects
        .iter()
        .enumerate()
        .rev()
        .find(|(_, o)| !o.hidden && hits(o, p, px_per_doc))
        .map(|(i, _)| i)
}

/// Objects whose bounds intersect a rubber-band rectangle.
pub fn pick_in_rect(doc: &Document, r: IRect) -> Vec<usize> {
    let r = r.normalized();
    doc.objects
        .iter()
        .enumerate()
        .filter(|(_, o)| {
            let b = o.bounds();
            !o.hidden && b.x < r.right() && b.right() > r.x && b.y < r.bottom() && b.bottom() > r.y
        })
        .map(|(i, _)| i)
        .collect()
}

/// Handle positions (screenshot coordinates): 8 around a box, the two ends of a line, the
/// left and right side of text and marker (width, not size); stamped marks have none. A turned
/// mark has them turned with it about its centre (LH `EdHandles`).
pub fn handles(o: &Object) -> Vec<(f64, f64)> {
    let h = local_handles(o);
    if !turned(o) {
        return h;
    }
    let c = o.bounds().center();
    h.into_iter().map(|p| turn(p, c, o.rot as f64)).collect()
}

/// The handle across from `handle`: the one that stays put while `handle` is dragged.
fn opposite(o: &Object, handle: usize) -> usize {
    match local_handles(o).len() {
        8 => (handle + 4) % 8,
        2 => 1 - handle.min(1),
        _ => handle,
    }
}

/// Handles of the mark as if it were not turned.
fn local_handles(o: &Object) -> Vec<(f64, f64)> {
    match o.kind() {
        Kind::Line => {
            let r = o.rect;
            vec![
                (r.x as f64, r.y as f64),
                ((r.x + r.w) as f64, (r.y + r.h) as f64),
            ]
        }
        Kind::Text | Kind::Mark => {
            let b = o.bounds();
            let (_, cy) = b.center();
            vec![(b.x as f64, cy), (b.right() as f64, cy)]
        }
        Kind::Counter | Kind::Stamp | Kind::Pen => vec![],
        _ => {
            let b = o.bounds();
            let (x0, y0, x1, y1) = (b.x as f64, b.y as f64, b.right() as f64, b.bottom() as f64);
            let (cx, cy) = b.center();
            vec![
                (x0, y0),
                (cx, y0),
                (x1, y0),
                (x1, cy),
                (x1, y1),
                (cx, y1),
                (x0, y1),
                (x0, cy),
            ]
        }
    }
}

/// Handle under `p` (screen-constant 7 px square), if any.
pub fn hit_handle(o: &Object, p: (f64, f64), px_per_doc: f64) -> Option<usize> {
    let tol = 7.0 / px_per_doc.max(1e-6);
    handles(o)
        .into_iter()
        .position(|(x, y)| (x - p.0).abs() <= tol && (y - p.1).abs() <= tol)
}

/// Resizes by dragging `handle` by (dx, dy) from the original rectangle `orig`. Always computed
/// from the original, never incrementally, so rounding does not accumulate (LH `EdManyBegin`).
/// A turned mark is resized in its own frame (the drag is turned back), and then shifted so the
/// handle across from the dragged one stays where it was on the screenshot (ZK-164).
pub fn resize(o: &mut Object, handle: usize, orig: IRect, dx: i32, dy: i32) {
    if !turned(o) {
        resize_local(o, handle, orig, dx, dy);
        return;
    }
    let rot = o.rot as f64;
    let (ldx, ldy) = turn((dx as f64, dy as f64), (0.0, 0.0), -rot);
    let before = {
        let mut a = o.clone();
        a.rect = orig;
        let fixed = local_handles(&a)[opposite(&a, handle)];
        turn(fixed, a.bounds().center(), rot)
    };
    resize_local(o, handle, orig, ldx.round() as i32, ldy.round() as i32);
    let after = turn(
        local_handles(o)[opposite(o, handle)],
        o.bounds().center(),
        rot,
    );
    o.rect.x += (before.0 - after.0).round() as i32;
    o.rect.y += (before.1 - after.1).round() as i32;
}

fn resize_local(o: &mut Object, handle: usize, orig: IRect, dx: i32, dy: i32) {
    match o.kind() {
        Kind::Line => {
            o.rect = if handle == 0 {
                IRect::new(orig.x + dx, orig.y + dy, orig.w - dx, orig.h - dy)
            } else {
                IRect::new(orig.x, orig.y, orig.w + dx, orig.h + dy)
            };
        }
        Kind::Text | Kind::Mark => {
            let n = orig.normalized();
            let w = (if handle == 0 { n.w - dx } else { n.w + dx }).max(8);
            let x = if handle == 0 { n.right() - w } else { n.x };
            o.rect = IRect::new(x, n.y, w, n.h);
            if let Data::Text { box_w, .. } = &mut o.data {
                *box_w = w;
            }
        }
        _ => {
            let n = orig.normalized();
            let (mut x0, mut y0, mut x1, mut y1) = (n.x, n.y, n.right(), n.bottom());
            match handle {
                0 => {
                    x0 += dx;
                    y0 += dy;
                }
                1 => y0 += dy,
                2 => {
                    x1 += dx;
                    y0 += dy;
                }
                3 => x1 += dx,
                4 => {
                    x1 += dx;
                    y1 += dy;
                }
                5 => y1 += dy,
                6 => {
                    x0 += dx;
                    y1 += dy;
                }
                _ => x0 += dx,
            }
            o.rect = IRect::new(x0, y0, x1 - x0, y1 - y0).normalized();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Head, Raster, Rgb};

    fn doc() -> Document {
        let mut d = Document::from_raster("t", Raster::solid(200, 200, Rgb::WHITE));
        d.push(Object::new(IRect::new(10, 10, 100, 50), Data::Rect));
        d.push(Object::new(
            IRect::new(0, 100, 100, 0),
            Data::Line {
                head_front: Head::None,
                head_back: Head::None,
                head_size: 1,
            },
        ));
        d
    }

    #[test]
    fn turned_marks_have_turned_handles_and_resize_in_their_own_frame() {
        let mut o = Object::new(IRect::new(100, 100, 100, 50), Data::Rect);
        o.rot = 90;
        // Turned a quarter clockwise about (150, 125): the top-left handle goes top-right.
        let h = handles(&o);
        assert!(
            (h[0].0 - 175.0).abs() < 1e-9 && (h[0].1 - 75.0).abs() < 1e-9,
            "{h:?}"
        );
        // The rotation handle stands to the right of the turned mark.
        let (rx, ry) = rotation_handle(&o, 1.0).unwrap();
        assert!((rx - (175.0 + ROTATION_STEM)).abs() < 1e-9 && (ry - 125.0).abs() < 1e-9);
        assert!(hit_rotation_handle(&o, (rx + 3.0, ry), 1.0));
        // Dragging the "right edge" handle (3) down by 20 on the screen makes the mark 20 wider
        // in its own frame; the left edge (handle 7) stays where it was.
        let fixed = handles(&o)[7];
        let orig = o.rect;
        resize(&mut o, 3, orig, 0, 20);
        assert_eq!((o.rect.w, o.rect.h), (120, 50));
        let now = handles(&o)[7];
        assert!(
            (now.0 - fixed.0).abs() <= 1.0 && (now.1 - fixed.1).abs() <= 1.0,
            "{fixed:?} → {now:?}"
        );
    }

    #[test]
    fn a_turned_line_is_hit_where_it_is_drawn() {
        let mut l = Object::new(
            IRect::new(0, 100, 100, 0),
            Data::Line {
                head_front: Head::None,
                head_back: Head::None,
                head_size: 1,
            },
        );
        l.rot = 90;
        // Drawn vertical through (50, 100).
        assert!(hits(&l, (50.0, 60.0), 1.0));
        assert!(!hits(&l, (10.0, 100.0), 1.0));
        assert_eq!(angle_from_up((0.0, 0.0), (10.0, 0.0)).round(), 90.0);
    }

    #[test]
    fn picks_topmost_and_respects_screen_tolerance() {
        let d = doc();
        assert_eq!(pick(&d, (50.0, 30.0), 1.0), Some(0));
        assert_eq!(pick(&d, (50.0, 104.0), 1.0), Some(1));
        // 4 px + half of 4 px thickness = 6 px at 1:1; at 4× zoom only 1 + 2 = 3 doc px.
        assert_eq!(pick(&d, (50.0, 105.5), 1.0), Some(1));
        assert_eq!(pick(&d, (50.0, 105.5), 4.0), None);
        assert_eq!(pick(&d, (150.0, 150.0), 1.0), None);
    }

    #[test]
    fn rotated_box_is_hit_inside_its_turned_shape() {
        let mut o = Object::new(IRect::new(0, 45, 100, 10), Data::Rect);
        o.rot = 90;
        // Turned by 90° around (50, 50): now a vertical bar x 45..55.
        assert!(hits(&o, (50.0, 5.0), 1.0));
        assert!(!hits(&o, (5.0, 50.0), 1.0));
    }

    #[test]
    fn resize_from_original_and_normalise() {
        let mut o = Object::new(IRect::new(10, 10, 100, 50), Data::Rect);
        let orig = o.rect;
        resize(&mut o, 4, orig, -150, -80);
        assert_eq!(o.rect, IRect::new(-40, -20, 50, 30));
        let mut l = Object::new(
            IRect::new(0, 0, 10, 10),
            Data::Line {
                head_front: Head::None,
                head_back: Head::None,
                head_size: 1,
            },
        );
        let orig = l.rect;
        resize(&mut l, 0, orig, 20, 0);
        assert_eq!(l.rect, IRect::new(20, 0, -10, 10));
        assert_eq!(hit_handle(&l, (20.0, 0.0), 2.0), Some(0));
    }
}
