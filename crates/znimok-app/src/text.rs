//! Text on the picture (ZK-184, ZK-185): recognised on the device — Znimok's own Tesseract helper
//! on Windows (ZK-120), Apple Vision on macOS — never a cloud model, no tokens. The editor shows
//! the lines on the canvas and in a panel; the capture overlay and the tray put the text straight
//! on the clipboard.

use znimok_core::IRect;
use znimok_models::ocr;

/// A recognised line, in document coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub text: String,
    pub rect: IRect,
}

/// What a reading gave: the lines, and the languages wanted but not installed here.
pub type Reading = Result<(Vec<Found>, Vec<String>), String>;

/// Recognises a `w`×`h` RGBA picture whose top-left corner is `origin` in the document.
pub fn recognize(w: u32, h: u32, rgba: Vec<u8>, origin: (i32, i32)) -> Reading {
    let img = znimok_models::Rgba::new(w, h, rgba).ok_or("empty picture")?;
    let engine = ocr::system().ok_or("no text recognition on this system")?;
    let r = ocr::read_text(engine.as_ref(), &img).map_err(|e| e.to_string())?;
    // What the engine made of icons, lines and shapes is dropped: a line needs letters or digits
    // for most of it, and a box many times taller than a usual line is not a line of text.
    let kept: Vec<_> = r.lines.into_iter().filter(|l| plausible(&l.text)).collect();
    let mut heights: Vec<f32> = kept.iter().map(|l| l.rect.h).collect();
    heights.sort_by(|a, b| a.total_cmp(b));
    let median = heights.get(heights.len() / 2).copied().unwrap_or(0.0);
    let lines = kept
        .into_iter()
        .filter(|l| heights.len() < 3 || l.rect.h <= median * 3.0)
        .map(|l| Found {
            text: l.text.trim().to_string(),
            rect: IRect::new(
                origin.0 + l.rect.x.floor() as i32,
                origin.1 + l.rect.y.floor() as i32,
                l.rect.w.ceil() as i32,
                l.rect.h.ceil() as i32,
            ),
        })
        .collect();
    Ok((lines, r.missing))
}

/// Whether a recognised line looks like text: at least two letters or digits, and they make at
/// least 60 % of what is not a space.
pub fn plausible(line: &str) -> bool {
    let chars: Vec<char> = line.chars().filter(|c| !c.is_whitespace()).collect();
    let alnum = chars.iter().filter(|c| c.is_alphanumeric()).count();
    alnum >= 2 && alnum * 10 >= chars.len() * 6
}

/// The lines as text, one per line, in reading order.
pub fn joined(lines: &[Found]) -> String {
    lines
        .iter()
        .map(|l| l.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

/// X in the capture overlay, the hotkey, the tray (ZK-185): reads `raster` on a worker thread,
/// puts the text on the clipboard and says so in the app's window, with what was copied.
pub fn read_to_clipboard(raster: znimok_core::Raster, me: crate::wins::WeakCtx) {
    std::thread::spawn(move || {
        let reading = recognize(raster.width, raster.height, raster.rgba, (0, 0));
        let _ = slint::invoke_from_event_loop(move || {
            me.with(|a, ui| {
                let (title, body) = match &reading {
                    Ok((lines, _)) if !lines.is_empty() => {
                        let text = joined(lines);
                        let copied = copy(&text);
                        LAST.with(|l| *l.borrow_mut() = Some(text.clone()));
                        let title = if copied {
                            a.tr.tr_args(
                                "text-copied-title",
                                &crate::app::fargs(&[("n", lines.len().to_string())]),
                            )
                        } else {
                            a.tr.tr("clipboard-error")
                        };
                        // The first lines of what went, so it can be checked at a glance.
                        let mut shown: Vec<&str> = text.lines().take(12).collect();
                        if text.lines().count() > 12 {
                            shown.push("…");
                        }
                        (title, shown.join("\n"))
                    }
                    Ok(_) => {
                        LAST.with(|l| *l.borrow_mut() = Some(String::new()));
                        (a.tr.tr("text-none"), String::new())
                    }
                    Err(e) => {
                        LAST.with(|l| *l.borrow_mut() = None);
                        (
                            a.tr.tr_args("text-error", &crate::app::fargs(&[("error", e.clone())])),
                            String::new(),
                        )
                    }
                };
                crate::show_window(ui);
                crate::dialog::ask(
                    ui,
                    title,
                    body,
                    vec![a.tr.tr("common-close")],
                    0,
                    Some(0),
                    |_, _| {},
                );
            })
        });
    });
}

thread_local! {
    /// What the last quick reading copied (for the self-test): `None` = failed.
    static LAST: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// For the self-test: the text the last quick reading copied.
pub fn last() -> Option<String> {
    LAST.with(|l| l.borrow().clone())
}

/// Puts `text` on the clipboard.
pub fn copy(text: &str) -> bool {
    arboard::Clipboard::new()
        .and_then(|mut cb| cb.set_text(text.to_string()))
        .is_ok()
}

/// A picture to read: width, height, RGBA and its top-left in the document.
pub type Picture = (u32, u32, Vec<u8>, (i32, i32));

/// A part `r` (document coordinates) of a frame picture `w`×`h` whose top-left is `origin`:
/// the pixels and the part's own top-left, or `None` when it misses the picture.
pub fn crop(w: u32, h: u32, rgba: &[u8], origin: (i32, i32), r: IRect) -> Option<Picture> {
    let r = r.normalized();
    let x0 = (r.x - origin.0).clamp(0, w as i32);
    let y0 = (r.y - origin.1).clamp(0, h as i32);
    let x1 = (r.right() - origin.0).clamp(0, w as i32);
    let y1 = (r.bottom() - origin.1).clamp(0, h as i32);
    if x1 - x0 < 4 || y1 - y0 < 4 {
        return None;
    }
    let (cw, ch) = ((x1 - x0) as u32, (y1 - y0) as u32);
    let mut out = Vec::with_capacity(cw as usize * ch as usize * 4);
    for y in y0..y1 {
        let row = (y as usize * w as usize + x0 as usize) * 4;
        out.extend_from_slice(&rgba[row..row + cw as usize * 4]);
    }
    Some((cw, ch, out, (origin.0 + x0, origin.1 + y0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_from_icons_and_shapes_is_not_text() {
        assert!(plausible("Ось ця кнопка"));
        assert!(plausible("Version 2.0 (build 17)"));
        assert!(!plausible("| | : й E -"));
        assert!(!plausible("-"));
        assert!(!plausible("✓ ✗ ? ! ★"));
    }

    #[test]
    fn a_part_of_the_frame_keeps_its_place_in_the_document() {
        let (w, h) = (10u32, 8u32);
        let rgba: Vec<u8> = (0..w * h * 4).map(|i| (i / 4) as u8).collect();
        let (cw, ch, px, at) = crop(w, h, &rgba, (100, 50), IRect::new(102, 53, 5, 4)).unwrap();
        assert_eq!((cw, ch, at), (5, 4, (102, 53)));
        // The first pixel is (2, 3) of the frame.
        assert_eq!(px[0], (3 * w + 2) as u8);
        assert!(crop(w, h, &rgba, (100, 50), IRect::new(0, 0, 3, 3)).is_none());
    }
}
