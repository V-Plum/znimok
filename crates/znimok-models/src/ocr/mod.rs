//! Text recognition on the device (nothing is sent anywhere).
//!
//! | | engine | Ukrainian |
//! |---|---|---|
//! | Windows | `znimok-ocr.exe` next to the app (Tesseract, [`helper`], ZK-120); without it Windows.Media.Ocr — languages come with the OS language packs | helper: **yes**; Windows.Media.Ocr: **no** (25 languages without it) |
//! | macOS | Apple Vision `VNRecognizeTextRequest` (accurate) | yes (`uk-UA`) |
//!
//! [`OcrResult::missing`] says which asked-for languages the engine could not use, so the app can
//! offer the cloud (with consent) instead of showing garbled text.

pub mod helper;
#[cfg(target_os = "macos")]
mod mac;
#[cfg(windows)]
mod win;

use crate::Rgba;
use std::fmt;

/// A box in pixels of the recognised picture, origin top-left.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn union(self, o: Rect) -> Rect {
        if self.w <= 0.0 || self.h <= 0.0 {
            return o;
        }
        let (x0, y0) = (self.x.min(o.x), self.y.min(o.y));
        let (x1, y1) = (
            (self.x + self.w).max(o.x + o.w),
            (self.y + self.h).max(o.y + o.h),
        );
        Rect {
            x: x0,
            y: y0,
            w: x1 - x0,
            h: y1 - y0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub text: String,
    pub rect: Rect,
    /// 0–1 when the engine tells (Vision), else `None`.
    pub confidence: Option<f32>,
    /// Words with their own boxes when the engine gives them (Windows); empty otherwise — then
    /// a part of the line is placed by its share of the characters.
    pub words: Vec<Word>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Word {
    pub text: String,
    pub rect: Rect,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OcrResult {
    pub lines: Vec<Line>,
    /// Languages the engine used (BCP 47).
    pub languages: Vec<String>,
    /// Asked for but not available here (e.g. `uk` on Windows).
    pub missing: Vec<String>,
}

impl OcrResult {
    pub fn text(&self) -> String {
        self.lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum OcrError {
    /// None of the asked-for languages is available; `available` lists what is.
    NoLanguage {
        available: Vec<String>,
    },
    Unsupported,
    Os(String),
}

impl fmt::Display for OcrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoLanguage { available } => {
                write!(
                    f,
                    "OCR: no such language here (available: {})",
                    available.join(", ")
                )
            }
            Self::Unsupported => write!(f, "OCR: not available on this system"),
            Self::Os(m) => write!(f, "OCR: {m}"),
        }
    }
}

impl std::error::Error for OcrError {}

pub trait Ocr: Send + Sync {
    /// Languages this machine can recognise now (BCP 47).
    fn languages(&self) -> Vec<String>;
    /// `languages` in order of preference (`["uk", "en"]`); empty = the user's languages.
    fn recognize(&self, img: &Rgba, languages: &[&str]) -> Result<OcrResult, OcrError>;
    /// A second reading for finding secrets (e-mail, keys, cards): engines that read Latin better
    /// with other settings do so here (the helper: English first). Default: the same reading.
    fn recognize_for_masking(&self, img: &Rgba, languages: &[&str]) -> Result<OcrResult, OcrError> {
        self.recognize(img, languages)
    }
}

/// Both readings for masking: the text one and, when it differs, the masking one — secrets are
/// looked for in each (duplicates are dropped by the masker).
pub fn read_for_masking(engine: &dyn Ocr, img: &Rgba) -> Option<OcrResult> {
    let mut text = engine.recognize(img, &[]).ok();
    if let Ok(more) = engine.recognize_for_masking(img, &[]) {
        match &mut text {
            Some(t) if t.lines != more.lines => t.lines.extend(more.lines),
            Some(_) => {}
            None => text = Some(more),
        }
    }
    text
}

pub fn system() -> Option<Box<dyn Ocr>> {
    #[cfg(windows)]
    return Some(match helper::find() {
        Some(exe) => Box::new(helper::TessHelper::new(exe)),
        None => Box::new(win::WinOcr),
    });
    #[cfg(target_os = "macos")]
    return Some(Box::new(mac::VisionOcr));
    #[cfg(not(any(windows, target_os = "macos")))]
    return None;
}

/// Splits `wanted` into (available tags to use, missing), matching by the primary subtag
/// (`uk` ↔ `uk-UA`, `en` ↔ `en-US`).
pub fn match_languages(wanted: &[&str], available: &[String]) -> (Vec<String>, Vec<String>) {
    // Russian is never used, even when the OS has it (owner, 29.09.2026).
    let available: Vec<String> = available
        .iter()
        .filter(|a| !is_russian(a))
        .cloned()
        .collect();
    let available = &available;
    let primary = |t: &str| {
        t.split(['-', '_'])
            .next()
            .unwrap_or("")
            .to_ascii_lowercase()
    };
    let mut use_ = Vec::new();
    let mut missing = Vec::new();
    for w in wanted {
        let exact = available.iter().find(|a| a.eq_ignore_ascii_case(w));
        let by_primary = || available.iter().find(|a| primary(a) == primary(w));
        match exact.or_else(by_primary) {
            Some(a) if !use_.contains(a) => use_.push(a.clone()),
            Some(_) => {}
            None => missing.push(w.to_string()),
        }
    }
    (use_, missing)
}

/// Russian by primary subtag — never offered or used (owner, 29.09.2026).
pub fn is_russian(tag: &str) -> bool {
    tag.split(['-', '_'])
        .next()
        .is_some_and(|p| p.eq_ignore_ascii_case("ru"))
}

/// Joins the MTA on this thread (WinRT from any thread; see znimok-win `com_thread`).
#[cfg(windows)]
pub(crate) fn winrt_thread() {
    use std::sync::Once;
    static PIN: Once = Once::new();
    PIN.call_once(|| {
        // SAFETY: pins the MTA for the process; the cookie is never released on purpose.
        let _ = unsafe { windows::Win32::System::Com::CoIncrementMTAUsage() };
    });
    thread_local! {
        static JOINED: () = {
            // SAFETY: plain apartment init; "already initialised" results are fine.
            let _ = unsafe {
                windows::Win32::System::WinRT::RoInitialize(
                    windows::Win32::System::WinRT::RO_INIT_MULTITHREADED,
                )
            };
        };
    }
    JOINED.with(|_| {});
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn languages_match_by_primary_subtag() {
        let avail = vec!["en-US".to_string(), "ru-RU".to_string()];
        assert_eq!(
            match_languages(&["uk", "en"], &avail),
            (vec!["en-US".to_string()], vec!["uk".to_string()])
        );
        // Russian is never used, even installed and asked for.
        assert_eq!(
            match_languages(&["ru", "en"], &avail),
            (vec!["en-US".to_string()], vec!["ru".to_string()])
        );
        let mac = vec!["uk-UA".to_string(), "en-US".to_string()];
        assert_eq!(
            match_languages(&["uk-UA", "en", "en-GB"], &mac),
            (vec!["uk-UA".to_string(), "en-US".to_string()], vec![])
        );
    }

    #[test]
    fn rect_union() {
        let a = Rect {
            x: 1.0,
            y: 2.0,
            w: 3.0,
            h: 4.0,
        };
        let b = Rect {
            x: 0.0,
            y: 5.0,
            w: 2.0,
            h: 3.0,
        };
        assert_eq!(
            a.union(b),
            Rect {
                x: 0.0,
                y: 2.0,
                w: 4.0,
                h: 6.0
            }
        );
        assert_eq!(Rect::default().union(b), b);
    }

    /// Renders known text with the system and reads it back — English everywhere, Ukrainian
    /// where the engine has it (checked against [`Ocr::languages`]).
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn recognises_a_rendered_line() {
        let Some(ocr) = system() else { return };
        let img = test_picture();
        let r = ocr.recognize(&img, &["en"]).unwrap();
        let text = r.text().to_lowercase();
        // Z and N of a 5×7 dot font are ambiguous (Vision reads «EMIMOK»); the rest is not.
        assert!(text.contains("imok"), "{text:?}");
        assert!(r.lines[0].rect.w > 10.0, "{:?}", r.lines);
    }

    /// Apple Vision reads Ukrainian (Windows OCR cannot — see the module docs).
    #[cfg(target_os = "macos")]
    #[test]
    fn vision_has_ukrainian() {
        let langs = system().unwrap().languages();
        assert!(langs.iter().any(|l| l == "uk-UA"), "{langs:?}");
    }

    /// «ZNIMOK» drawn as thick blocks: no font needed, any OCR reads it.
    #[cfg(any(windows, target_os = "macos"))]
    fn test_picture() -> Rgba {
        // 5×7 dot glyphs.
        const G: &[(char, [&str; 7])] = &[
            (
                'Z',
                [
                    "#####", "....#", "...#.", "..#..", ".#...", "#....", "#####",
                ],
            ),
            (
                'N',
                [
                    "#...#", "##..#", "#.#.#", "#..##", "#...#", "#...#", "#...#",
                ],
            ),
            (
                'I',
                [
                    "#####", "..#..", "..#..", "..#..", "..#..", "..#..", "#####",
                ],
            ),
            (
                'M',
                [
                    "#...#", "##.##", "#.#.#", "#...#", "#...#", "#...#", "#...#",
                ],
            ),
            (
                'O',
                [
                    ".###.", "#...#", "#...#", "#...#", "#...#", "#...#", ".###.",
                ],
            ),
            (
                'K',
                [
                    "#...#", "#..#.", "#.#..", "##...", "#.#..", "#..#.", "#...#",
                ],
            ),
        ];
        let word = "ZNIMOK";
        let (cell, pad) = (8u32, 40u32);
        let w = pad * 2 + word.len() as u32 * 6 * cell;
        let h = pad * 2 + 7 * cell;
        let mut px = vec![255u8; (w * h * 4) as usize];
        for (i, ch) in word.chars().enumerate() {
            let rows = G.iter().find(|(c, _)| *c == ch).unwrap().1;
            for (ry, row) in rows.iter().enumerate() {
                for (rx, b) in row.bytes().enumerate() {
                    if b != b'#' {
                        continue;
                    }
                    for dy in 0..cell {
                        for dx in 0..cell {
                            let x = pad + (i as u32 * 6 + rx as u32) * cell + dx;
                            let y = pad + ry as u32 * cell + dy;
                            let o = ((y * w + x) * 4) as usize;
                            px[o..o + 3].copy_from_slice(&[0, 0, 0]);
                        }
                    }
                }
            }
        }
        Rgba::new(w, h, px).unwrap()
    }
}
