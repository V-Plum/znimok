//! Windows.Media.Ocr. One engine per language; with several languages asked for, the first
//! available one is used (Windows OCR does not mix scripts in one pass).

use super::{Line, Ocr, OcrError, OcrResult, Rect, Word, match_languages, winrt_thread};
use crate::Rgba;
use windows::Globalization::Language;
use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Security::Cryptography::CryptographicBuffer;
use windows::core::HSTRING;

pub struct WinOcr;

fn os(e: windows::core::Error) -> OcrError {
    OcrError::Os(e.message())
}

impl Ocr for WinOcr {
    fn languages(&self) -> Vec<String> {
        winrt_thread();
        OcrEngine::AvailableRecognizerLanguages()
            .map(|v| {
                v.into_iter()
                    .filter_map(|l| l.LanguageTag().ok().map(|t| t.to_string()))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn recognize(&self, img: &Rgba, languages: &[&str]) -> Result<OcrResult, OcrError> {
        winrt_thread();
        let available = self.languages();
        let (use_, missing) = match_languages(languages, &available);
        let engine = match use_.first() {
            Some(tag) => OcrEngine::TryCreateFromLanguage(
                &Language::CreateLanguage(&HSTRING::from(tag)).map_err(os)?,
            )
            .map_err(os)?,
            None if languages.is_empty() => {
                OcrEngine::TryCreateFromUserProfileLanguages().map_err(os)?
            }
            None => return Err(OcrError::NoLanguage { available }),
        };
        let used = engine
            .RecognizerLanguage()
            .and_then(|l| l.LanguageTag())
            .map(|t| t.to_string())
            .unwrap_or_default();

        // The engine takes at most MaxImageDimension per side.
        let max = OcrEngine::MaxImageDimension().map_err(os)?.max(1);
        let scale = (max as f32 / img.width.max(img.height) as f32).min(1.0);
        let (w, h) = (
            ((img.width as f32 * scale) as u32).max(1),
            ((img.height as f32 * scale) as u32).max(1),
        );
        let rgba = if scale < 1.0 {
            let buf = image::RgbaImage::from_raw(img.width, img.height, img.pixels.clone())
                .ok_or(OcrError::Unsupported)?;
            image::imageops::resize(&buf, w, h, image::imageops::FilterType::Triangle).into_raw()
        } else {
            img.pixels.clone()
        };
        // BGRA, transparency flattened on white.
        let bgra: Vec<u8> = rgba
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| {
                let a = p[3] as u32;
                let f = |c: u8| ((c as u32 * a + 255 * (255 - a)) / 255) as u8;
                [f(p[2]), f(p[1]), f(p[0]), 255]
            })
            .collect();
        let buffer = CryptographicBuffer::CreateFromByteArray(&bgra).map_err(os)?;
        let bitmap = SoftwareBitmap::CreateCopyWithAlphaFromBuffer(
            &buffer,
            BitmapPixelFormat::Bgra8,
            w as i32,
            h as i32,
            BitmapAlphaMode::Ignore,
        )
        .map_err(os)?;
        let result = engine
            .RecognizeAsync(&bitmap)
            .map_err(os)?
            .join()
            .map_err(os)?;
        let back = 1.0 / scale;
        let mut lines = Vec::new();
        for line in result.Lines().map_err(os)? {
            let mut rect = Rect::default();
            let mut words = Vec::new();
            for word in line.Words().map_err(os)? {
                let r = word.BoundingRect().map_err(os)?;
                let wr = Rect {
                    x: r.X * back,
                    y: r.Y * back,
                    w: r.Width * back,
                    h: r.Height * back,
                };
                rect = rect.union(wr);
                words.push(Word {
                    text: word.Text().map_err(os)?.to_string(),
                    rect: wr,
                });
            }
            lines.push(Line {
                text: line.Text().map_err(os)?.to_string(),
                rect,
                confidence: None,
                words,
            });
        }
        Ok(OcrResult {
            lines,
            languages: vec![used],
            missing,
        })
    }
}
