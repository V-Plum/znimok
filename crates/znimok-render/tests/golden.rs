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
    let mut r = Renderer::deterministic();
    let mut pix = Pixmap::new(1, 1);
    r.render(doc, View::one_to_one(doc), &mut pix);
    pix
}

/// Rectangles (output pixels) not compared: emoji stamps, a little larger than their marks.
fn masked(doc: &Document) -> Vec<IRect> {
    let f = doc.frame();
    doc.objects
        .iter()
        .filter(|o| matches!(o.data, Data::Stamp { id } if id >= 100))
        .map(|o| {
            let b = o.bounds();
            let m = b.w.max(b.h) / 3 + 4;
            IRect::new(b.x - m - f.x, b.y - m - f.y, b.w + 2 * m, b.h + 2 * m)
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
    let actual = render(doc);
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
    let (bad, compared) = compare(&actual, &golden, &masked(doc));
    let share = bad as f64 / compared.max(1) as f64;
    if share > SHARE {
        let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/golden-diff");
        std::fs::create_dir_all(&out).unwrap();
        let mut d = Pixmap::new(w as u16, h as u16);
        d.data_as_u8_slice_mut()
            .copy_from_slice(&diff_map(&actual, &golden, &masked(doc)));
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
