//! Golden images of the renderer (ZK-114): scenes built in code, rendered 1:1 by the
//! deterministic renderer and compared with PNGs kept in `tests/golden/`.
//!
//! - A pixel counts as different when any channel differs by more than [`CHANNEL`]; the scene
//!   passes while at most [`SHARE`] of its pixels differ (anti-aliasing may vary a little between
//!   CPU architectures — the CI runs Windows x64 and macOS arm64).
//! - Emoji come from the system's colour font (Segoe UI Emoji / Apple Color Emoji), everything
//!   else from the bundled Onest (ZK-34), so the areas of emoji stamps are not compared.
//! - `ZNIMOK_BLESS=1 cargo test -p znimok-render --test golden` writes the goldens anew (look at
//!   them before committing). On a mismatch the actual image and a difference map land in
//!   `target/golden-diff/`.

use std::path::PathBuf;

use znimok_core::{Command, Data, Document, Editor, IRect};
use znimok_render::vello_cpu::Pixmap;
use znimok_render::{Renderer, View, reference};

const CHANNEL: u8 = 4;
const SHARE: f64 = 0.002;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

fn render(doc: &Document) -> Pixmap {
    render_view(doc, View::one_to_one(doc))
}

fn render_view(doc: &Document, view: View) -> Pixmap {
    let mut r = Renderer::deterministic();
    let mut pix = Pixmap::new(1, 1);
    r.render(doc, view, &mut pix);
    pix
}

/// Rectangles (output pixels) not compared: emoji stamps, a little larger than their marks.
fn masked(doc: &Document) -> Vec<IRect> {
    masked_in(doc, View::one_to_one(doc))
}

fn masked_in(doc: &Document, view: View) -> Vec<IRect> {
    doc.objects
        .iter()
        .filter(|o| matches!(o.data, Data::Stamp { id } if id >= 100))
        .map(|o| {
            let b = o.bounds();
            let m = b.w.max(b.h) / 3 + 4;
            let x = ((b.x - m) as f64 - view.origin.x) * view.scale;
            let y = ((b.y - m) as f64 - view.origin.y) * view.scale;
            let side = |v: i32| ((v + 2 * m) as f64 * view.scale).ceil() as i32 + 2;
            IRect::new(
                x.floor() as i32 - 1,
                y.floor() as i32 - 1,
                side(b.w),
                side(b.h),
            )
        })
        .collect()
}

/// Pixels differing beyond the tolerance, and pixels compared (outside the mask).
fn compare(actual: &Pixmap, golden: &Pixmap, mask: &[IRect]) -> (usize, usize) {
    let m = diff_map(actual, golden, mask);
    let bad = m.chunks(4).filter(|p| p[0] == 255).count();
    let masked_px = m.chunks(4).filter(|p| p[3] == 0).count();
    (bad, m.len() / 4 - masked_px)
}

/// Red where a pixel differs beyond the tolerance, black where it matches, clear where masked.
fn diff_map(actual: &Pixmap, golden: &Pixmap, mask: &[IRect]) -> Vec<u8> {
    let w = actual.width() as i32;
    let h = actual.height() as i32;
    let a = actual.data_as_u8_slice();
    let g = golden.data_as_u8_slice();
    let mut out = vec![0u8; a.len()];
    for y in 0..h {
        for x in 0..w {
            if mask
                .iter()
                .any(|m| x >= m.x && x < m.right() && y >= m.y && y < m.bottom())
            {
                continue;
            }
            let i = ((y * w + x) * 4) as usize;
            let d = (0..4)
                .map(|c| a[i + c].abs_diff(g[i + c]))
                .max()
                .unwrap_or(0);
            out[i..i + 4].copy_from_slice(if d > CHANNEL {
                &[255, 0, 0, 255]
            } else {
                &[0, 0, 0, 255]
            });
        }
    }
    out
}

fn check(name: &str, doc: &Document) {
    check_view(name, doc, View::one_to_one(doc));
}

fn check_view(name: &str, doc: &Document, view: View) {
    let actual = render_view(doc, view);
    let path = dir().join(format!("{name}.png"));
    if std::env::var_os("ZNIMOK_BLESS").is_some() || !path.exists() {
        std::fs::create_dir_all(dir()).unwrap();
        std::fs::write(&path, actual.clone().into_png().unwrap()).unwrap();
        eprintln!("golden written: {}", path.display());
        return;
    }
    let golden = Pixmap::from_png(std::io::Cursor::new(std::fs::read(&path).unwrap())).unwrap();
    assert_eq!(
        (actual.width(), actual.height()),
        (golden.width(), golden.height()),
        "{name}: size differs from the golden"
    );
    let (w, h) = (actual.width() as i32, actual.height() as i32);
    let mask = masked_in(doc, view);
    let (bad, compared) = compare(&actual, &golden, &mask);
    let share = bad as f64 / compared.max(1) as f64;
    if share > SHARE {
        let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/golden-diff");
        std::fs::create_dir_all(&out).unwrap();
        let mut d = Pixmap::new(w as u16, h as u16);
        d.data_as_u8_slice_mut()
            .copy_from_slice(&diff_map(&actual, &golden, &mask));
        std::fs::write(
            out.join(format!("{name}-actual.png")),
            actual.into_png().unwrap(),
        )
        .unwrap();
        std::fs::write(out.join(format!("{name}-diff.png")), d.into_png().unwrap()).unwrap();
        panic!(
            "{name}: {bad} of {compared} pixels differ ({:.3} %, allowed {:.1} %); see {}",
            share * 100.0,
            SHARE * 100.0,
            out.display()
        );
    }
}

/// Every kind of mark (the reference scene of P1).
#[test]
fn every_kind_of_mark() {
    check("every-kind", &reference::reference_document(1600, 1000));
}

/// The recipe over the source (ZK-53) through the editor's commands, as a user makes it: a
/// quarter turn and the mirror move the marks with the picture, tone, then a crop.
#[test]
fn recipe_and_crop() {
    let mut ed = Editor::new(reference::reference_document(1600, 1000));
    for cmd in [
        Command::Rotate { quarters: 1 },
        Command::Mirror,
        Command::SetTone {
            exposure: Some(0.5),
            gamma: Some(1.1),
            contrast: Some(15),
            merge: None,
        },
        Command::SetCrop {
            rect: Some(IRect::new(100, 150, 800, 1200)),
        },
    ] {
        ed.apply(cmd).unwrap();
    }
    check("recipe-crop", &ed.doc);
}

/// The comparison itself catches a change: one moved mark must fail against the golden.
#[test]
fn a_moved_mark_fails() {
    let mut doc = reference::reference_document(1600, 1000);
    doc.objects[0].translate(40, 0);
    let actual = render(&doc);
    let golden = Pixmap::from_png(std::io::Cursor::new(
        std::fs::read(dir().join("every-kind.png")).unwrap(),
    ))
    .unwrap();
    let (bad, compared) = compare(&actual, &golden, &masked(&doc));
    assert!(bad as f64 / compared as f64 > SHARE, "{bad} of {compared}");
}

/// ZK-50: a Hide works on everything below it — a caption under a blur is blurred (not dropped,
/// not left sharp), and editing the caption changes the blurred tile (the cache key sees it).
#[test]
fn hide_covers_the_marks_below() {
    use znimok_core::{Align, HideMode, Object, Raster, Rgb, Style};
    let base = Document::from_raster("t", Raster::solid(200, 80, Rgb::WHITE));
    let caption = |text: &str| {
        Object::new(
            IRect::new(20, 20, 160, 40),
            Data::Text {
                text: text.into(),
                size: 28,
                bold: true,
                italic: false,
                align: Align::Left,
                box_w: 0,
            },
        )
        .with_style(Style {
            color: Rgb::BLACK,
            ..Style::default()
        })
    };
    let hide = Object::new(
        IRect::new(10, 10, 180, 60),
        Data::Hide {
            mode: HideMode::Pixelate,
            strength: 40,
        },
    );
    let with = |text: Option<&str>| {
        let mut d = base.clone();
        if let Some(t) = text {
            d.push(caption(t));
        }
        d.push(hide.clone());
        d
    };
    let mut r = Renderer::deterministic();
    let mut shot = |d: &Document| {
        let mut p = Pixmap::new(1, 1);
        r.render(d, View::one_to_one(d), &mut p);
        p.data_as_u8_slice().to_vec()
    };
    let empty = shot(&with(None));
    let one = shot(&with(Some("СЕКРЕТ")));
    let two = shot(&with(Some("ІНШЕ")));
    assert_ne!(one, empty, "the caption under the Hide shows, pixelated");
    assert_ne!(one, two, "a changed caption makes a new tile");
    // Not sharp: the text's own pixels are not all there.
    let sharp = shot(&{
        let mut d = base.clone();
        d.push(caption("СЕКРЕТ"));
        d
    });
    assert_ne!(one, sharp);
}

/// ZK-33: the styles the reference scene shows only once, each next to its neighbours — dashes,
/// corners, shadow and glow levels, opacity, a plate without outline, line and pen heads with
/// their sizes, counter shapes, text weights, alignments and outline.
#[test]
fn styles() {
    use znimok_core::{
        Align, Corners, CounterShape, Dash, Effect, Head, Object, Raster, Rgb, Style,
    };
    let mut doc = Document::from_raster(
        "styles",
        Raster::solid(1200, 900, Rgb::new(0xF6, 0xF7, 0xF9)),
    );
    let s = |color: Rgb| Style {
        color,
        ..Style::default()
    };
    let blue = Rgb::new(0x3D, 0x7B, 0xF5);
    let green = Rgb::new(0x34, 0xC4, 0x8A);
    // Row 1: dash × corners × effects on rectangles.
    let rects = [
        (Dash::Solid, Corners::Sharp, Effect::None, Effect::None, 100),
        (
            Dash::Dashed,
            Corners::Soft,
            Effect::Light,
            Effect::None,
            100,
        ),
        (
            Dash::DashDot,
            Corners::Round,
            Effect::Strong,
            Effect::None,
            100,
        ),
        (
            Dash::Solid,
            Corners::Round,
            Effect::None,
            Effect::Light,
            100,
        ),
        (Dash::Solid, Corners::Soft, Effect::None, Effect::Strong, 40),
    ];
    for (i, (dash, corners, shadow, glow, alpha)) in rects.into_iter().enumerate() {
        doc.push(
            Object::new(IRect::new(40 + i as i32 * 230, 40, 180, 120), Data::Rect).with_style(
                Style {
                    dash,
                    corners,
                    corner_px: 24,
                    shadow,
                    glow,
                    alpha,
                    thick: 7,
                    ..s(blue)
                },
            ),
        );
    }
    // Row 2: a plate (fill, no outline), an ellipse with fill, thin and thick outlines.
    doc.push(
        Object::new(IRect::new(40, 210, 180, 120), Data::Rect).with_style(Style {
            no_main: true,
            color2: Some(Rgb::YELLOW),
            alpha2: 80,
            corners: Corners::Soft,
            corner_px: 16,
            ..s(Rgb::RED)
        }),
    );
    doc.push(
        Object::new(IRect::new(270, 210, 180, 120), Data::Ellipse).with_style(Style {
            color2: Some(green),
            alpha2: 30,
            thick: 2,
            ..s(green)
        }),
    );
    doc.push(
        Object::new(IRect::new(500, 210, 180, 120), Data::Ellipse).with_style(Style {
            thick: 7,
            dash: Dash::Dashed,
            ..s(Rgb::RED)
        }),
    );
    // Row 3: every head at every size, on lines; a pen with heads.
    let heads = [Head::Triangle, Head::Chevron, Head::Dot, Head::None];
    for (i, head) in heads.into_iter().enumerate() {
        for size in 0..3u8 {
            let y = 380 + size as i32 * 50;
            let x = 40 + i as i32 * 170;
            doc.push(
                Object::new(
                    IRect::new(x, y, 130, 20),
                    Data::Line {
                        head_front: head,
                        head_back: if i == 3 { Head::Triangle } else { Head::None },
                        head_size: size,
                    },
                )
                .with_style(Style {
                    thick: [2, 4, 7][size as usize],
                    ..s(Rgb::RED)
                }),
            );
        }
    }
    doc.push(
        Object::new(
            IRect::new(740, 380, 200, 120),
            Data::Pen {
                points: vec![(740, 480), (780, 400), (840, 460), (900, 390), (940, 470)],
                head_front: Head::Triangle,
                head_back: Head::Dot,
            },
        )
        .with_style(Style {
            thick: 4,
            ..s(blue)
        }),
    );
    // Row 4: counter shapes, two digits, a second colour for the digit.
    let shapes = [
        CounterShape::Circle,
        CounterShape::RoundedBox,
        CounterShape::Pin,
    ];
    for (i, shape) in shapes.into_iter().enumerate() {
        doc.push(
            Object::new(
                IRect::new(40 + i as i32 * 90, 560, 56, 56),
                Data::Counter {
                    seq: i as u32,
                    group: 1,
                    start: 9,
                    shape,
                },
            )
            .with_style(Style {
                thick: 56,
                color2: (i == 2).then_some(Rgb::YELLOW),
                ..s(Rgb::RED)
            }),
        );
    }
    // Row 5: text — weights, alignment in a block, outline.
    let texts = [
        ("Звичайний", false, false, Align::Left, 0, None),
        ("Жирний", true, false, Align::Left, 0, None),
        ("Курсив", false, true, Align::Left, 0, None),
        ("Обведений", true, false, Align::Left, 0, Some(Rgb::WHITE)),
    ];
    for (i, (text, bold, italic, align, box_w, outline)) in texts.into_iter().enumerate() {
        doc.push(
            Object::new(
                IRect::new(40 + i as i32 * 280, 660, 0, 0),
                Data::Text {
                    text: text.into(),
                    size: 36,
                    bold,
                    italic,
                    align,
                    box_w,
                },
            )
            .with_style(Style {
                color2: outline,
                ..s(if outline.is_some() {
                    Rgb::RED
                } else {
                    Rgb::BLACK
                })
            }),
        );
    }
    for (i, align) in [Align::Left, Align::Center, Align::Right]
        .into_iter()
        .enumerate()
    {
        doc.push(
            Object::new(
                IRect::new(40 + i as i32 * 380, 740, 320, 0),
                Data::Text {
                    text: "Блок 320 px: рядки переносяться й вирівнюються".into(),
                    size: 22,
                    bold: false,
                    italic: false,
                    align,
                    box_w: 320,
                },
            )
            .with_style(s(Rgb::BLACK)),
        );
    }
    check("styles", &doc);
}

/// ZK-33: the canvas shows the document zoomed — 200 % over a detail, 50 % over the whole —
/// the views a GPU path (ZK-130) has to match too.
#[test]
fn zoomed_views() {
    use znimok_render::vello_cpu::kurbo::Point;
    let doc = reference::reference_document(1600, 1000);
    check_view(
        "zoom-200",
        &doc,
        View {
            scale: 2.0,
            origin: Point::new(500.0, 150.0),
            width: 900,
            height: 700,
        },
    );
    check_view(
        "zoom-50",
        &doc,
        View {
            scale: 0.5,
            origin: Point::new(0.0, 0.0),
            width: 800,
            height: 500,
        },
    );
}
