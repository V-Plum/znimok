//! A 4K canvas: a full render against the partial repaint of one dragged mark (ZK-130).
//! `cargo run --release -p znimok-render --example repaint_bench`

use std::time::Instant;
use znimok_render::vello_cpu::Pixmap;
use znimok_render::vello_cpu::kurbo::Point;
use znimok_render::{Renderer, Repaint, Tracker, View, reference};

fn main() {
    let mut doc = reference::reference_document(3840, 2160);
    let view = View {
        scale: 1.0,
        origin: Point::new(0.0, 0.0),
        width: 3840,
        height: 2160,
    };
    let mut r = Renderer::new();
    let mut t = Tracker::default();
    let mut canvas = Pixmap::new(1, 1);
    r.changes(&doc, view, 0, &mut t);
    r.render(&doc, view, &mut canvas);
    let n = 30;
    let mut full = 0.0;
    for _ in 0..n {
        let s = Instant::now();
        r.render(&doc, view, &mut canvas);
        full += s.elapsed().as_secs_f64();
    }
    println!("full 4K render: {:.1} ms", full / n as f64 * 1000.0);
    for (name, i) in [
        ("rectangle", 0usize),
        ("arrow", 3),
        ("text", 6),
        ("counter", 12),
    ] {
        let i = i.min(doc.objects.len() - 1);
        let mut part = 0.0;
        let mut px = 0i64;
        for k in 0..n {
            doc.objects[i].translate(if k % 2 == 0 { 6 } else { -5 }, 2);
            let s = Instant::now();
            match r.changes(&doc, view, 0, &mut t) {
                Repaint::Rects(rs) => {
                    px += rs.iter().map(|r| r.w as i64 * r.h as i64).sum::<i64>();
                    r.render_rects(&doc, view, &rs, &mut canvas);
                }
                Repaint::All => r.render(&doc, view, &mut canvas),
                Repaint::Nothing => {}
            }
            part += s.elapsed().as_secs_f64();
        }
        let mut whole = 0.0;
        for k in 0..n {
            doc.objects[i].translate(if k % 2 == 0 { 6 } else { -5 }, 2);
            let s = Instant::now();
            r.render(&doc, view, &mut canvas);
            whole += s.elapsed().as_secs_f64();
        }
        r.changes(&doc, view, 0, &mut t);
        println!(
            "drag {name:9}: full {:.1} ms, partial {:.1} ms, {:.1} % of the canvas",
            whole / n as f64 * 1000.0,
            part / n as f64 * 1000.0,
            px as f64 / n as f64 / (3840.0 * 2160.0) * 100.0
        );
    }
}
