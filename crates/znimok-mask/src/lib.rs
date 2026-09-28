//! Smart masking (ZK-72): what on a screenshot is worth hiding, found on the device.
//!
//! 1. Text: on-device OCR ([`znimok_models::ocr`]) → [`patterns::find`] on every line → the box
//!    of the matched part (word boxes when the engine gives them, else the share of the line's
//!    characters, with a margin).
//! 2. Faces: [`znimok_models::faces`] (Apple Vision / Windows.Media.FaceAnalysis).
//! 3. [`Suggestion`]s the user reviews («прийняти всі / окремі»), then [`commands`] turns the
//!    accepted ones into `AddObject` Hide marks — one undo step with the caller's merge key.
//!
//! Text secrets are covered with a solid plate by default: blurred or pixelated text can
//! sometimes be read back. Faces get a blur. Nothing leaves the machine.

pub mod patterns;

pub use patterns::{Kind, mask_text};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use znimok_core::{Command, Data, HideMode, IRect, MergeKey, Object, Rgb, Style};
use znimok_models::ocr::{Line, OcrResult, Rect};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Suggestion {
    pub kind: Kind,
    /// Pixels of the picture.
    pub rect: IRect,
    /// For the review list: the found text with its middle hidden (`sk-a…Uv`), `None` for faces.
    pub preview: Option<String>,
    pub mode: HideMode,
}

/// Margin around a text box, as a share of its height (OCR boxes hug the glyphs).
const PAD: f32 = 0.25;

/// Suggestions for text lines and face boxes of a `w`×`h` picture.
pub fn suggest(ocr: Option<&OcrResult>, faces: &[Rect], w: u32, h: u32) -> Vec<Suggestion> {
    let mut out = Vec::new();
    for line in ocr.map(|o| o.lines.as_slice()).unwrap_or_default() {
        for hit in patterns::find(&line.text) {
            let r = span_rect(line, hit.start, hit.end);
            let pad = r.h * PAD;
            out.push(Suggestion {
                kind: hit.kind,
                rect: to_irect(
                    Rect {
                        x: r.x - pad,
                        y: r.y - pad,
                        w: r.w + 2.0 * pad,
                        h: r.h + 2.0 * pad,
                    },
                    w,
                    h,
                ),
                preview: Some(preview(&line.text[hit.start..hit.end])),
                mode: HideMode::Plate,
            });
        }
    }
    for f in faces {
        // A little more than the detector's box: hair and ears identify too.
        let (px, py) = (f.w * 0.15, f.h * 0.2);
        out.push(Suggestion {
            kind: Kind::Face,
            rect: to_irect(
                Rect {
                    x: f.x - px,
                    y: f.y - py,
                    w: f.w + 2.0 * px,
                    h: f.h + 2.0 * py,
                },
                w,
                h,
            ),
            preview: None,
            mode: HideMode::Blur,
        });
    }
    out.retain(|s| s.rect.w > 0 && s.rect.h > 0);
    out
}

/// Hide marks for the accepted suggestions, one undo step (`merge`).
pub fn commands(accepted: &[Suggestion], merge: Option<MergeKey>) -> Vec<Command> {
    accepted
        .iter()
        .map(|s| {
            let mut obj = Object::new(
                s.rect,
                Data::Hide {
                    mode: s.mode,
                    strength: 60,
                },
            );
            obj.style = Style {
                color: Rgb::BLACK,
                ..Style::default()
            };
            obj.name = Some(match &s.preview {
                Some(p) => format!("{:?}: {p}", s.kind),
                None => format!("{:?}", s.kind),
            });
            Command::AddObject {
                object: obj,
                select: false,
                merge: merge.clone(),
            }
        })
        .collect()
}

/// `sk-ant-api03-AbCd…` → `sk-a…Uv`: enough to recognise it in the list, not enough to use it.
pub fn preview(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= 6 {
        return "•".repeat(chars.len().max(3));
    }
    let head: String = chars[..4].iter().collect();
    let tail: String = chars[chars.len() - 2..].iter().collect();
    format!("{head}…{tail}")
}

/// The box of bytes `start..end` of the line: from word boxes when the engine gave them,
/// otherwise by the share of characters (proportional fonts make it approximate; the margin
/// covers it).
fn span_rect(line: &Line, start: usize, end: usize) -> Rect {
    if !line.words.is_empty() {
        // Words joined by single spaces rebuild the line text (Windows OCR does that).
        let mut at = 0usize;
        let mut rect = Rect::default();
        for w in &line.words {
            let (ws, we) = (at, at + w.text.len());
            if we > start && ws < end {
                rect = rect.union(w.rect);
            }
            at = we + 1;
        }
        if rect.w > 0.0 {
            return rect;
        }
    }
    let total = line.text.chars().count().max(1) as f32;
    let before = line.text[..start].chars().count() as f32;
    let inside = line.text[start..end].chars().count() as f32;
    Rect {
        x: line.rect.x + line.rect.w * before / total,
        y: line.rect.y,
        w: line.rect.w * inside / total,
        h: line.rect.h,
    }
}

fn to_irect(r: Rect, w: u32, h: u32) -> IRect {
    let x0 = r.x.floor().clamp(0.0, w as f32) as i32;
    let y0 = r.y.floor().clamp(0.0, h as f32) as i32;
    let x1 = (r.x + r.w).ceil().clamp(0.0, w as f32) as i32;
    let y1 = (r.y + r.h).ceil().clamp(0.0, h as f32) as i32;
    IRect::new(x0, y0, x1 - x0, y1 - y0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use znimok_models::ocr::Word;

    fn line(text: &str, rect: Rect, words: Vec<Word>) -> Line {
        Line {
            text: text.into(),
            rect,
            confidence: None,
            words,
        }
    }

    fn r(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }

    #[test]
    fn word_boxes_give_the_exact_span() {
        let l = line(
            "password: hunter2",
            r(10.0, 20.0, 170.0, 20.0),
            vec![
                Word {
                    text: "password:".into(),
                    rect: r(10.0, 20.0, 90.0, 20.0),
                },
                Word {
                    text: "hunter2".into(),
                    rect: r(110.0, 20.0, 70.0, 20.0),
                },
            ],
        );
        let ocr = OcrResult {
            lines: vec![l],
            languages: vec![],
            missing: vec![],
        };
        let s = suggest(Some(&ocr), &[], 400, 100);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].kind, Kind::Secret);
        // 110..180 × 20..40 plus a quarter of the height around.
        assert_eq!(s[0].rect, IRect::new(105, 15, 80, 30));
        assert_eq!(s[0].mode, HideMode::Plate);
        assert_eq!(s[0].preview.as_deref(), Some("hunt…r2"));
    }

    #[test]
    fn without_words_the_share_of_characters() {
        // 10 characters over 100 px: «a@b.test» is characters 2..10.
        let l = line("к a@b.test", r(0.0, 0.0, 100.0, 10.0), vec![]);
        let ocr = OcrResult {
            lines: vec![l],
            languages: vec![],
            missing: vec![],
        };
        let s = suggest(Some(&ocr), &[], 200, 50);
        assert_eq!(s[0].kind, Kind::Email);
        let x = s[0].rect.x;
        assert!((17..=20).contains(&x), "{:?}", s[0].rect);
        assert!(s[0].rect.x + s[0].rect.w >= 100);
    }

    #[test]
    fn faces_are_blurred_with_a_margin_and_clamped() {
        let s = suggest(None, &[r(5.0, 5.0, 100.0, 100.0)], 300, 300);
        assert_eq!(s[0].kind, Kind::Face);
        assert_eq!(s[0].mode, HideMode::Blur);
        assert_eq!(s[0].rect, IRect::new(0, 0, 120, 125));
    }

    #[test]
    fn commands_add_named_black_plates_in_one_step() {
        let s = vec![Suggestion {
            kind: Kind::Card,
            rect: IRect::new(1, 2, 30, 10),
            preview: Some(preview("4111 1111 1111 1111")),
            mode: HideMode::Plate,
        }];
        let c = commands(&s, Some(MergeKey::Drag { id: 7 }));
        let Command::AddObject { object, merge, .. } = &c[0] else {
            panic!()
        };
        assert_eq!(object.rect, IRect::new(1, 2, 30, 10));
        assert_eq!(object.style.color, Rgb::BLACK);
        assert!(matches!(
            object.data,
            Data::Hide {
                mode: HideMode::Plate,
                ..
            }
        ));
        assert_eq!(object.name.as_deref(), Some("Card: 4111…11"));
        assert_eq!(merge, &Some(MergeKey::Drag { id: 7 }));
        // The command is valid for the editor.
        let mut ed = znimok_core::Editor::new(znimok_core::Document::from_raster(
            "t",
            znimok_core::Raster::solid(64, 64, Rgb::WHITE),
        ));
        for cmd in c {
            ed.apply(cmd).unwrap();
        }
        assert_eq!(ed.doc.objects.len(), 1);
    }
}
