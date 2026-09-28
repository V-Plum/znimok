//! Reference scene for P1: a synthetic "settings dialog" screenshot with every kind of mark,
//! built in code so the golden image does not depend on any file.

use znimok_core::*;

/// Emoji stamps by index (100+). A handful is enough for the prototype.
pub fn emoji_for(id: u32) -> &'static str {
    const E: [&str; 8] = ["😀", "👍", "🔥", "⭐", "❤️", "✅", "⚠️", "🚀"];
    E[(id.saturating_sub(100) as usize) % E.len()]
}

/// A flat "application window" drawing, `w`×`h`, straight alpha.
pub fn synthetic_screenshot(w: u32, h: u32) -> Raster {
    let mut r = Raster::solid(w, h, Rgb::new(0xF4, 0xF5, 0xF8));
    let fill = |r: &mut Raster, x0: u32, y0: u32, x1: u32, y1: u32, c: Rgb| {
        for y in y0.min(h)..y1.min(h) {
            for x in x0.min(w)..x1.min(w) {
                let i = ((y * w + x) * 4) as usize;
                r.rgba[i..i + 3].copy_from_slice(&[c.r, c.g, c.b]);
            }
        }
    };
    // Title bar, sidebar, cards, a primary button and some "text" bars.
    fill(&mut r, 0, 0, w, h / 14, Rgb::new(0xFF, 0xFF, 0xFF));
    fill(
        &mut r,
        0,
        h / 14,
        h / 14 * 5 / 2,
        h,
        Rgb::new(0xEE, 0xF0, 0xF4),
    );
    let cw = w / 4;
    for i in 0..3 {
        let x0 = w / 3 + i * (cw + w / 24);
        fill(
            &mut r,
            x0,
            h / 5,
            x0 + cw,
            h / 5 + h / 4,
            Rgb::new(0xFF, 0xFF, 0xFF),
        );
        fill(
            &mut r,
            x0 + 12,
            h / 5 + 12,
            x0 + cw - 12,
            h / 5 + 24,
            Rgb::new(0xD0, 0xD4, 0xDB),
        );
    }
    fill(
        &mut r,
        w / 3,
        h / 5 + h / 4 + h / 20,
        w / 3 + w / 8,
        h / 5 + h / 4 + h / 20 + h / 22,
        Rgb::BLUE,
    );
    for i in 0..6 {
        let y = h / 2 + i * (h / 26);
        fill(
            &mut r,
            w / 3,
            y,
            w / 3 + w / 3 - i * 20,
            y + h / 60,
            Rgb::new(0xC5, 0xC9, 0xD1),
        );
    }
    // A fake "secret": a bar that the Hide marks cover.
    fill(
        &mut r,
        w / 3,
        h * 3 / 4,
        w / 3 + w / 5,
        h * 3 / 4 + h / 30,
        Rgb::new(0x22, 0x26, 0x2E),
    );
    // Fine checker in the corner so blur/pixelate are visibly different.
    for y in h * 3 / 4..h * 3 / 4 + h / 30 {
        for x in w / 3 + w / 5 + 20..w / 3 + w / 5 + 20 + w / 10 {
            if (x / 3 + y / 3) % 2 == 0 {
                let i = ((y * w + x) * 4) as usize;
                r.rgba[i..i + 3].copy_from_slice(&[0x20, 0x20, 0x20]);
            }
        }
    }
    r
}

/// Every kind at least once, with the styles that matter (dash, fill, heads, outline, corners,
/// effects, rotation, opacity). Sized for a 1600×1000 screenshot; scaled for other sizes.
pub fn reference_document(w: u32, h: u32) -> Document {
    let mut doc = Document::from_raster("reference", synthetic_screenshot(w, h));
    let sx = w as f64 / 1600.0;
    let sy = h as f64 / 1000.0;
    let r = |x: f64, y: f64, rw: f64, rh: f64| {
        IRect::new(
            (x * sx) as i32,
            (y * sy) as i32,
            (rw * sx) as i32,
            (rh * sy) as i32,
        )
    };
    let s = |color: Rgb, thick: i32| Style {
        color,
        thick,
        ..Style::default()
    };

    // Rect: outline + translucent fill, soft corners, light shadow.
    doc.push(
        Object::new(r(560.0, 190.0, 380.0, 260.0), Data::Rect).with_style(Style {
            color2: Some(Rgb::RED),
            alpha2: 15,
            corners: Corners::Soft,
            shadow: Effect::Light,
            ..s(Rgb::RED, 4)
        }),
    );
    // Rect: dashed, thin, no fill.
    doc.push(
        Object::new(r(1000.0, 190.0, 380.0, 260.0), Data::Rect).with_style(Style {
            dash: Dash::Dashed,
            ..s(Rgb::BLUE, 2)
        }),
    );
    // Rect: solid plate (no outline) with 60 % opacity, rotated.
    let mut plate = Object::new(r(1180.0, 520.0, 220.0, 90.0), Data::Rect).with_style(Style {
        no_main: true,
        alpha: 60,
        ..s(Rgb::YELLOW, 4)
    });
    plate.rot = 350;
    doc.push(plate);
    // Ellipse: thick, dash-dot, glow.
    doc.push(
        Object::new(r(90.0, 200.0, 200.0, 140.0), Data::Ellipse).with_style(Style {
            dash: Dash::DashDot,
            glow: Effect::Strong,
            ..s(Rgb::GREEN, 7)
        }),
    );
    // Line with a triangle head, and a plain line with chevron and dot heads.
    doc.push(
        Object::new(
            r(1040.0, 120.0, -260.0, 110.0),
            Data::Line {
                head_front: Head::Triangle,
                head_back: Head::None,
                head_size: 2,
            },
        )
        .with_style(s(Rgb::RED, 4)),
    );
    doc.push(
        Object::new(
            r(120.0, 420.0, 300.0, -40.0),
            Data::Line {
                head_front: Head::Chevron,
                head_back: Head::Dot,
                head_size: 1,
            },
        )
        .with_style(Style {
            dash: Dash::Dashed,
            ..s(Rgb::BLUE, 2)
        }),
    );
    // Pen trail.
    let pts: Vec<(i32, i32)> = (0..40)
        .map(|i| {
            let t = i as f64 / 39.0;
            (
                ((600.0 + t * 260.0) * sx) as i32,
                ((520.0 + (t * 12.0).sin() * 28.0 + t * 40.0) * sy) as i32,
            )
        })
        .collect();
    doc.push(
        Object::new(r(600.0, 500.0, 260.0, 80.0), Data::Pen { points: pts })
            .with_style(s(Rgb::new(0xA0, 0x5C, 0xF5), 4)),
    );
    // Text: with outline, bold; and a wrapped block, right aligned, italic, 70 %.
    doc.push(
        Object::new(
            r(560.0, 60.0, 0.0, 0.0),
            Data::Text {
                text: "Ось ця кнопка".into(),
                size: 32,
                bold: true,
                italic: false,
                align: Align::Left,
                box_w: 0,
            },
        )
        .with_style(Style {
            color2: Some(Rgb::WHITE),
            ..s(Rgb::RED, 4)
        }),
    );
    doc.push(
        Object::new(
            r(1120.0, 640.0, 300.0, 0.0),
            Data::Text {
                text: "Довгий напис, що переноситься на кілька рядків у блоці 300 px".into(),
                size: 20,
                bold: false,
                italic: true,
                align: Align::Right,
                box_w: (300.0 * sx) as i32,
            },
        )
        .with_style(Style {
            alpha: 70,
            ..s(Rgb::new(0x14, 0x16, 0x1A), 4)
        }),
    );
    // Hide: blur, pixelate, plate.
    doc.push(
        Object::new(
            r(526.0, 745.0, 330.0, 44.0),
            Data::Hide {
                mode: HideMode::Blur,
                strength: 60,
            },
        )
        .with_style(s(Rgb::BLACK, 4)),
    );
    doc.push(
        Object::new(
            r(870.0, 745.0, 170.0, 44.0),
            Data::Hide {
                mode: HideMode::Pixelate,
                strength: 50,
            },
        )
        .with_style(s(Rgb::BLACK, 4)),
    );
    doc.push(
        Object::new(
            r(1060.0, 745.0, 120.0, 44.0),
            Data::Hide {
                mode: HideMode::Plate,
                strength: 100,
            },
        )
        .with_style(s(Rgb::new(0x22, 0x26, 0x2E), 4)),
    );
    // Mark: yellow marker over "text" bars.
    doc.push(Object::new(r(526.0, 540.0, 320.0, 24.0), Data::Mark).with_style(s(Rgb::YELLOW, 24)));
    // Counters: circle ×2 in group 1 (start 1), rounded box, pin in group 2 (start 10).
    for (i, x) in [560.0, 760.0].into_iter().enumerate() {
        doc.push(
            Object::new(
                r(x, 170.0, 36.0, 36.0),
                Data::Counter {
                    seq: i as u32,
                    group: 1,
                    start: 1,
                    shape: CounterShape::Circle,
                },
            )
            .with_style(s(Rgb::RED, 36)),
        );
    }
    doc.push(
        Object::new(
            r(980.0, 170.0, 36.0, 36.0),
            Data::Counter {
                seq: 2,
                group: 1,
                start: 1,
                shape: CounterShape::RoundedBox,
            },
        )
        .with_style(Style {
            color2: Some(Rgb::YELLOW),
            ..s(Rgb::BLUE, 36)
        }),
    );
    doc.push(
        Object::new(
            r(1380.0, 420.0, 40.0, 48.0),
            Data::Counter {
                seq: 3,
                group: 2,
                start: 10,
                shape: CounterShape::Pin,
            },
        )
        .with_style(s(Rgb::GREEN, 40)),
    );
    // Stamps: check, cross, question, exclamation, star, warning, then two emoji.
    for (i, id) in (0..6).enumerate() {
        doc.push(
            Object::new(
                r(80.0 + i as f64 * 60.0, 620.0, 44.0, 44.0),
                Data::Stamp { id },
            )
            .with_style(s(if id == 1 { Rgb::RED } else { Rgb::GREEN }, 44)),
        );
    }
    doc.push(
        Object::new(r(80.0, 700.0, 48.0, 48.0), Data::Stamp { id: 100 })
            .with_style(s(Rgb::BLACK, 48)),
    );
    doc.push(
        Object::new(r(140.0, 700.0, 48.0, 48.0), Data::Stamp { id: 102 })
            .with_style(s(Rgb::BLACK, 48)),
    );
    // Image: a pasted thumbnail with round corners and strong shadow.
    let thumb = {
        let mut t = Raster::solid(120, 80, Rgb::new(0x3D, 0x7B, 0xF5));
        for y in 0..80u32 {
            for x in 0..120u32 {
                if (x / 10 + y / 10) % 2 == 0 {
                    let i = ((y * 120 + x) * 4) as usize;
                    t.rgba[i..i + 3].copy_from_slice(&[0xFF, 0xD2, 0x3F]);
                }
            }
        }
        t
    };
    doc.banks.push(thumb);
    doc.push(
        Object::new(r(80.0, 800.0, 240.0, 160.0), Data::Image { bank: 1 }).with_style(Style {
            corners: Corners::Round,
            shadow: Effect::Strong,
            ..Style::default()
        }),
    );
    doc
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn reference_covers_every_kind() {
        let doc = reference_document(1600, 1000);
        let kinds: HashSet<Kind> = doc.objects.iter().map(|o| o.kind()).collect();
        for k in [
            Kind::Rect,
            Kind::Ellipse,
            Kind::Line,
            Kind::Pen,
            Kind::Text,
            Kind::Hide,
            Kind::Mark,
            Kind::Counter,
            Kind::Stamp,
            Kind::Image,
        ] {
            assert!(kinds.contains(&k), "missing {k:?}");
        }
        let first_counter = doc
            .objects
            .iter()
            .position(|o| o.kind() == Kind::Counter)
            .unwrap();
        assert_eq!(doc.counter_number(first_counter), Some(1));
        assert_eq!(doc.counter_number(first_counter + 2), Some(3));
        assert_eq!(doc.counter_number(first_counter + 3), Some(10));
    }
}
