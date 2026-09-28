//! A «screenshot» with real text, drawn by znimok-render, read by the OS OCR and masked:
//! the password, the e-mail and the card are found where they are, the login is left alone.
#![cfg(any(windows, target_os = "macos"))]

use znimok_core::{Align, Data, Document, IRect, Object, Raster, Rgb, Style};
use znimok_mask::{Kind, suggest};
use znimok_models::{Rgba, ocr};
use znimok_render::{Renderer, View, pixmap_to_rgba, vello_cpu::Pixmap};

const LINES: &[(&str, i32)] = &[
    ("login: vadym", 30),
    ("password: hunter2secret", 100),
    ("mail: test.user@example.com", 170),
    // Not the usual 4111 1111 1111 1111: Windows OCR drops a line of repeated «1111» entirely.
    ("card 4539 1488 0343 6467", 240),
];

fn screenshot() -> Rgba {
    let mut doc = Document::from_raster("t", Raster::solid(1000, 400, Rgb::WHITE));
    for (text, y) in LINES {
        let mut o = Object::new(
            IRect::new(40, *y, 900, 50),
            Data::Text {
                text: (*text).into(),
                size: 36,
                bold: false,
                italic: false,
                align: Align::Left,
                box_w: 0,
            },
        );
        o.style = Style {
            color: Rgb::BLACK,
            ..Style::default()
        };
        doc.push(o);
    }
    let mut pix = Pixmap::new(1, 1);
    Renderer::new().render(&doc, View::one_to_one(&doc), &mut pix);
    Rgba::new(
        pix.width() as u32,
        pix.height() as u32,
        pixmap_to_rgba(&pix),
    )
    .unwrap()
}

#[test]
fn secrets_on_a_rendered_screenshot_are_found_in_place() {
    let img = screenshot();
    let engine = ocr::system().unwrap();
    let r = engine.recognize(&img, &["en"]).unwrap();
    let s = suggest(Some(&r), &[], img.width, img.height);
    let found = |k: Kind| s.iter().find(|x| x.kind == k);
    let line_of = |y: i32| {
        LINES
            .iter()
            .position(|(_, ly)| (ly - 10..ly + 60).contains(&y))
    };

    let pw = found(Kind::Secret).unwrap_or_else(|| panic!("{:?}\n{}", s, r.text()));
    assert_eq!(line_of(pw.rect.y + pw.rect.h / 2), Some(1), "{pw:?}");
    // Only the value: it starts to the right of «password:».
    assert!(pw.rect.x > 150, "{pw:?}");
    let mail = found(Kind::Email).unwrap_or_else(|| panic!("{:?}\n{}", s, r.text()));
    assert_eq!(line_of(mail.rect.y + mail.rect.h / 2), Some(2), "{mail:?}");
    let card = found(Kind::Card).unwrap_or_else(|| panic!("{:?}\n{}", s, r.text()));
    assert_eq!(line_of(card.rect.y + card.rect.h / 2), Some(3), "{card:?}");
    // The login line has nothing to hide.
    assert!(
        s.iter()
            .all(|x| line_of(x.rect.y + x.rect.h / 2) != Some(0)),
        "{s:?}"
    );
}

#[test]
#[ignore]
fn dump_screenshot() {
    let img = screenshot();
    let p = std::env::temp_dir().join("znimok-mask-e2e.png");
    image::save_buffer(
        &p,
        &img.pixels,
        img.width,
        img.height,
        image::ExtendedColorType::Rgba8,
    )
    .unwrap();
    eprintln!("{}", p.display());
}
