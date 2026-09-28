//! Apple Vision `VNRecognizeTextRequest` (accurate, language correction on). Vision mixes the
//! asked-for languages in one pass (Ukrainian + English together). Called through the Objective-C
//! runtime directly — no extra binding crate for a handful of messages.

use super::{Line, Ocr, OcrError, OcrResult, Rect, match_languages};
use crate::Rgba;
use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::AnyObject;
use objc2::{AnyThread, class, msg_send};
use objc2_foundation::{NSArray, NSData, NSDictionary, NSError, NSRect, NSString};

#[link(name = "Vision", kind = "framework")]
unsafe extern "C" {}

pub struct VisionOcr;

/// `VNRequestTextRecognitionLevelAccurate`.
const ACCURATE: isize = 0;

fn err(e: Retained<NSError>) -> OcrError {
    OcrError::Os(e.localizedDescription().to_string())
}

fn new_request() -> Retained<AnyObject> {
    // SAFETY: plain alloc/init of a Vision class.
    unsafe {
        let r: Retained<AnyObject> = msg_send![class!(VNRecognizeTextRequest), new];
        let _: () = msg_send![&r, setRecognitionLevel: ACCURATE];
        let _: () = msg_send![&r, setUsesLanguageCorrection: true];
        r
    }
}

fn supported(req: &AnyObject) -> Vec<String> {
    // SAFETY: documented selector; the error form maps to `Result`.
    let r: Result<Retained<NSArray<NSString>>, Retained<NSError>> =
        unsafe { msg_send![req, supportedRecognitionLanguagesAndReturnError: _] };
    r.map(|a| a.iter().map(|s| s.to_string()).collect())
        .unwrap_or_default()
}

impl Ocr for VisionOcr {
    fn languages(&self) -> Vec<String> {
        autoreleasepool(|_| supported(&new_request()))
    }

    fn recognize(&self, img: &Rgba, languages: &[&str]) -> Result<OcrResult, OcrError> {
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(
            image::RgbaImage::from_raw(img.width, img.height, img.pixels.clone())
                .ok_or(OcrError::Unsupported)?,
        )
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(|e| OcrError::Os(e.to_string()))?;
        let png = png.into_inner();

        autoreleasepool(|_| {
            let req = new_request();
            let available = supported(&req);
            let (use_, missing) = match_languages(languages, &available);
            if use_.is_empty() && !languages.is_empty() {
                return Err(OcrError::NoLanguage { available });
            }
            if !use_.is_empty() {
                let tags: Vec<Retained<NSString>> =
                    use_.iter().map(|t| NSString::from_str(t)).collect();
                let arr = NSArray::from_retained_slice(&tags);
                // SAFETY: an array of language tags, as documented.
                let _: () = unsafe { msg_send![&req, setRecognitionLanguages: &*arr] };
            }
            let data = NSData::with_bytes(&png);
            let options = NSDictionary::<NSString, AnyObject>::new();
            // SAFETY: documented initialiser taking image data and an options dictionary.
            let handler: Retained<AnyObject> = unsafe {
                msg_send![
                    AnyObject::alloc_class(class!(VNImageRequestHandler)),
                    initWithData: &*data,
                    options: &*options
                ]
            };
            let reqs = NSArray::from_retained_slice(std::slice::from_ref(&req));
            // SAFETY: as above; the error form maps to `Result`.
            let done: Result<(), Retained<NSError>> =
                unsafe { msg_send![&handler, performRequests: &*reqs, error: _] };
            done.map_err(err)?;
            // SAFETY: after a successful perform, `results` is an array of observations.
            let results: Option<Retained<NSArray<AnyObject>>> = unsafe { msg_send![&req, results] };
            let (w, h) = (img.width as f32, img.height as f32);
            let mut lines = Vec::new();
            for obs in results.iter().flat_map(|a| a.iter()) {
                // SAFETY: VNRecognizedTextObservation API.
                unsafe {
                    let top: Retained<NSArray<AnyObject>> = msg_send![&obs, topCandidates: 1usize];
                    let Some(best) = top.firstObject() else {
                        continue;
                    };
                    let text: Retained<NSString> = msg_send![&best, string];
                    let confidence: f32 = msg_send![&best, confidence];
                    // Normalised, origin bottom-left.
                    let b: NSRect = msg_send![&obs, boundingBox];
                    lines.push(Line {
                        text: text.to_string(),
                        rect: Rect {
                            x: b.origin.x as f32 * w,
                            y: (1.0 - (b.origin.y + b.size.height) as f32) * h,
                            w: b.size.width as f32 * w,
                            h: b.size.height as f32 * h,
                        },
                        confidence: Some(confidence),
                    });
                }
            }
            Ok(OcrResult {
                lines,
                languages: use_,
                missing,
            })
        })
    }
}
