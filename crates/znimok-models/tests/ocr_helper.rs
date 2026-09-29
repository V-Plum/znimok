//! Live test of `znimok-ocr.exe` (ZK-120) against pictures of the evaluation set. Runs only with
//! `ZNIMOK_OCR_HELPER` pointing at a built helper (tools/ocr-helper/build.py); CI sets it.

#![cfg(windows)]

use std::path::PathBuf;
use znimok_models::Rgba;
use znimok_models::ocr::helper::TessHelper;
use znimok_models::ocr::{Ocr, read_for_masking};

fn helper() -> Option<TessHelper> {
    let exe = PathBuf::from(std::env::var_os("ZNIMOK_OCR_HELPER")?);
    Some(TessHelper::new(exe))
}

fn picture(name: &str) -> Rgba {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/ocr-eval/set")
        .join(name);
    let img = image::open(&p).unwrap().into_rgba8();
    Rgba::new(img.width(), img.height(), img.into_raw()).unwrap()
}

#[test]
fn reads_ukrainian_and_the_masking_reading_sees_the_email() {
    let Some(h) = helper() else {
        eprintln!("ZNIMOK_OCR_HELPER not set: skipped");
        return;
    };
    let img = picture("segoe-15-light-06.png"); // «Надіслати на v.plum@example.com о 14:35»
    let r = h.recognize(&img, &["uk", "en"]).unwrap();
    let text = r.text();
    assert!(text.contains("Надіслати"), "{text}");
    assert!(r.missing.is_empty());
    assert!(!r.lines[0].words.is_empty());
    let both = read_for_masking(&h, &img).unwrap();
    assert!(
        both.text().contains("v.plum@example.com"),
        "{}",
        both.text()
    );
    // Russian is never asked for, never used.
    assert!(matches!(
        h.recognize(&img, &["ru"]),
        Err(znimok_models::ocr::OcrError::NoLanguage { .. })
    ));
}

/// The helper leaves after its idle time; the next request starts it again.
#[test]
fn restarts_after_the_helper_left() {
    let Some(h) = helper() else {
        return;
    };
    let img = picture("arial-13-dark-01.png");
    let first = h.recognize(&img, &[]).unwrap().text();
    std::thread::sleep(std::time::Duration::from_secs(7));
    let again = h.recognize(&img, &[]).unwrap().text();
    assert_eq!(first, again);
}
