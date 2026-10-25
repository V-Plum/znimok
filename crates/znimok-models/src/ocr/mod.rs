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

/// Languages of «text on the picture» (ZK-202), in order: Ukrainian, then English.
pub const TEXT_LANGUAGES: [&str; 2] = ["uk", "en"];

/// Text for the person (the editor's panel, the overlay, the tray, `znimok text`): the
/// [`TEXT_LANGUAGES`] asked for by name, so an engine that has only some of them says which are
/// missing instead of reading Cyrillic with an English model in silence; with none of them it
/// reads with the user's languages and reports both as missing.
pub fn read_text(engine: &dyn Ocr, img: &Rgba) -> Result<OcrResult, OcrError> {
    match engine.recognize(img, &TEXT_LANGUAGES) {
        Err(OcrError::NoLanguage { .. }) => {
            let mut r = engine.recognize(img, &[])?;
            r.missing = TEXT_LANGUAGES.iter().map(|l| l.to_string()).collect();
            Ok(r)
        }
        r => r,
    }
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

/// Text folded for finding a phrase among lines read (ZK-314): lower case, and the Cyrillic
/// letters Tesseract reads Latin ones as (Ноте for Home, Ргісіпд for Pricing, «а» in Find а Job)
/// as those Latin letters — one character for one, so positions in the folded text are
/// positions in the original. Applied to both the phrase and the line, a Ukrainian phrase still
/// finds Ukrainian text.
pub fn fold_for_find(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            // Capitals as their Latin capitals' small letters.
            'А' => 'a',
            'В' => 'b',
            'Е' | 'Ё' | 'Є' => 'e',
            'К' => 'k',
            'М' => 'm',
            'Н' => 'h',
            'О' => 'o',
            'Р' => 'p',
            'С' => 'c',
            'Т' => 't',
            'У' => 'y',
            'Х' => 'x',
            'І' | 'Ї' => 'i',
            'Ј' => 'j',
            'Ѕ' => 's',
            // Small letters as the Latin ones they are read for.
            'а' => 'a',
            'в' => 'b',
            'г' => 'r',
            'д' => 'g',
            'е' | 'ё' | 'є' => 'e',
            'з' => 's',
            'і' | 'ї' => 'i',
            'ј' => 'j',
            'к' => 'k',
            'м' => 'm',
            'н' => 'h',
            'о' => 'o',
            'п' => 'n',
            'р' => 'p',
            'с' => 'c',
            'т' => 'm',
            'у' => 'y',
            'х' => 'x',
            'ѕ' => 's',
            c => c.to_lowercase().next().unwrap_or(c),
        })
        .collect()
}

/// Where `needle` stands in `text` (ZK-316): `(start, end, misread)` in characters of `text`,
/// end exclusive. Compared folded (case, Cyrillic twins — [`fold_for_find`]) and without
/// spaces, so «QA Test 1» finds «QATest1». When `fuzzy`, a few characters may differ — OCR
/// misreads a letter (Тезі1 for Test1, ОА for QA) — at most one in three, only for phrases of
/// five characters or more; `misread` counts them, 0 for an exact match.
pub fn find_phrase(text: &str, needle: &str, fuzzy: bool) -> Option<(usize, usize, usize)> {
    // The text without spaces, and where each of its characters stands in the original.
    let mut hay: Vec<char> = Vec::new();
    let mut at: Vec<usize> = Vec::new();
    for (i, c) in fold_for_find(text).chars().enumerate() {
        if !c.is_whitespace() {
            hay.push(c);
            at.push(i);
        }
    }
    let pat: Vec<char> = fold_for_find(needle)
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    if pat.is_empty() || hay.is_empty() {
        return None;
    }
    let span = |s: usize, e: usize| (at[s], at[e - 1] + 1);
    // Exact first.
    if let Some(s) = hay.windows(pat.len()).position(|w| w == pat.as_slice()) {
        let (a, b) = span(s, s + pat.len());
        return Some((a, b, 0));
    }
    if !fuzzy || pat.len() < 5 {
        return None;
    }
    let limit = pat.len() / 3;
    // Approximate substring (Sellers): the least edits of `pat` against any substring of `hay`,
    // with where that substring starts.
    let (m, n) = (pat.len(), hay.len());
    let indel = |c: char| if c.is_ascii_digit() { limit + 1 } else { 1 };
    let mut prev: Vec<(usize, usize)> = (0..=n).map(|j| (0, j)).collect();
    for i in 1..=m {
        let mut cur = vec![(prev[0].0 + indel(pat[i - 1]), 0usize); n + 1];
        for j in 1..=n {
            let sub = prev[j - 1].0 + sub_cost(pat[i - 1], hay[j - 1], limit);
            let del = prev[j].0 + indel(pat[i - 1]);
            let ins = cur[j - 1].0 + indel(hay[j - 1]);
            cur[j] = if sub <= del && sub <= ins {
                (sub, prev[j - 1].1)
            } else if del <= ins {
                (del, prev[j].1)
            } else {
                (ins, cur[j - 1].1)
            };
        }
        prev = cur;
    }
    let (end, &(cost, start)) = prev
        .iter()
        .enumerate()
        .skip(1)
        .min_by_key(|(j, (c, _))| (*c, *j))?;
    if cost > limit || start >= end {
        return None;
    }
    let (a, b) = span(start, end);
    Some((a, b, cost))
}

/// What one character read for another costs in [`find_phrase`]: nothing when the same; one
/// for a letter misread, or a digit for its look-alike letter (l/1, o/0); any other change to a
/// digit — Test 1 is not Test 2 — more than any allowance, as is a digit missing or added.
fn sub_cost(a: char, b: char, limit: usize) -> usize {
    // A digit is read only as its look-alike letter (and back).
    const LOOK: &[(char, char)] = &[
        ('1', 'l'),
        ('1', 'i'),
        ('0', 'o'),
        ('5', 's'),
        ('8', 'b'),
        ('2', 'z'),
    ];
    if a == b {
        0
    } else if a.is_ascii_digit() || b.is_ascii_digit() {
        if LOOK
            .iter()
            .any(|&(d, l)| (a, b) == (d, l) || (a, b) == (l, d))
        {
            1
        } else {
            limit + 1
        }
    } else {
        1
    }
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
mod fold_tests {
    use super::fold_for_find;

    /// Latin read as Cyrillic twins folds to the Latin; Ukrainian folds the same on both sides;
    /// one character for one (ZK-314).
    #[test]
    fn twins_fold_to_latin() {
        assert_eq!(fold_for_find("Find а Job"), fold_for_find("Find a Job"));
        assert_eq!(fold_for_find("Ноте"), "home");
        assert_eq!(fold_for_find("Ргісіпд"), "pricing");
        assert_eq!(fold_for_find("АРІ"), "api");
        let ua = "Надіслати на пошту";
        assert!(fold_for_find(ua).contains(&fold_for_find("надіслати")));
        assert_eq!(fold_for_find(ua).chars().count(), ua.chars().count());
    }

    /// Spaces do not count; a misread letter or two is allowed only when asked and nothing is
    /// exact (ZK-316).
    #[test]
    fn phrases_are_found_without_spaces_and_with_a_misread_letter() {
        use super::find_phrase;
        assert_eq!(
            find_phrase("QATest1 QATest2 Events", "QA Test 1", false),
            Some((0, 7, 0))
        );
        assert_eq!(
            find_phrase("Home Find а Job", "find a job", false),
            Some((5, 15, 0))
        );
        assert_eq!(find_phrase("ОА Тезі1 QATest2", "QA Test 1", false), None);
        let (a, b, miss) = find_phrase("ОА Тезі1 QATest2", "QA Test 1", true).unwrap();
        assert_eq!((a, b), (0, 8));
        assert!((1..=2).contains(&miss), "{miss}");
        // Another digit is another thing: «QATest2» is not «QA Test 1».
        assert_eq!(find_phrase("QATest2 Events", "QA Test 1", true), None);
        // Short phrases stay exact; far-off text is not «found».
        assert_eq!(find_phrase("Hone", "Home", true), None);
        assert_eq!(
            find_phrase("Events Theme Migration", "Find a Job", true),
            None
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An engine with only the given languages, answering like Windows OCR does.
    struct Only(Vec<String>);
    impl Ocr for Only {
        fn languages(&self) -> Vec<String> {
            self.0.clone()
        }
        fn recognize(&self, _: &Rgba, languages: &[&str]) -> Result<OcrResult, OcrError> {
            let (used, missing) = match_languages(languages, &self.0);
            if used.is_empty() && !languages.is_empty() {
                return Err(OcrError::NoLanguage {
                    available: self.0.clone(),
                });
            }
            Ok(OcrResult {
                lines: Vec::new(),
                languages: used,
                missing,
            })
        }
    }

    /// ZK-202: the text is asked for in Ukrainian and English by name; what the engine lacks
    /// is reported, and an engine with neither still reads (both reported missing).
    #[test]
    fn read_text_names_its_languages() {
        let img = Rgba::new(1, 1, vec![255; 4]).unwrap();
        let r = read_text(&Only(vec!["en-US".into()]), &img).unwrap();
        assert_eq!(
            (r.languages, r.missing),
            (vec!["en-US".to_string()], vec!["uk".to_string()])
        );
        let r = read_text(&Only(vec!["uk-UA".into(), "en-US".into()]), &img).unwrap();
        assert!(r.missing.is_empty() && r.languages[0] == "uk-UA");
        let r = read_text(&Only(vec!["de-DE".into()]), &img).unwrap();
        assert_eq!(r.missing, vec!["uk".to_string(), "en".to_string()]);
    }

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
