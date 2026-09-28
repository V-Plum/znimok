//! Face boxes on the device, for «сховати обличчя» suggestions (ZK-72). Only where faces are —
//! no identification, no landmarks, nothing leaves the machine.
//!
//! Windows: `Windows.Media.FaceAnalysis.FaceDetector` (part of the OS, works unpackaged; takes a
//! Gray8 picture). macOS: Apple Vision `VNDetectFaceRectanglesRequest`.

use crate::Rgba;
use crate::ocr::Rect;

#[derive(Debug, PartialEq)]
pub enum FaceError {
    Unsupported,
    Os(String),
}

impl std::fmt::Display for FaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported => write!(f, "face detection is not available here"),
            Self::Os(m) => write!(f, "face detection: {m}"),
        }
    }
}

impl std::error::Error for FaceError {}

/// Boxes of faces in pixels of `img`, origin top-left.
pub fn detect(img: &Rgba) -> Result<Vec<Rect>, FaceError> {
    #[cfg(windows)]
    return win::detect(img);
    #[cfg(target_os = "macos")]
    return mac::detect(img);
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = img;
        Err(FaceError::Unsupported)
    }
}

/// Luma of straight RGBA, transparency on white (BT.601 weights — what detectors are trained on).
/// Windows' detector takes Gray8; Vision reads the PNG itself.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn gray(img: &Rgba) -> Vec<u8> {
    img.pixels
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| {
            let a = p[3] as u32;
            let on_white = |c: u8| (c as u32 * a + 255 * (255 - a)) / 255;
            ((299 * on_white(p[0]) + 587 * on_white(p[1]) + 114 * on_white(p[2])) / 1000) as u8
        })
        .collect()
}

#[cfg(windows)]
mod win {
    use super::*;
    use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
    use windows::Media::FaceAnalysis::FaceDetector;
    use windows::Security::Cryptography::CryptographicBuffer;

    fn os(e: windows::core::Error) -> FaceError {
        FaceError::Os(e.message())
    }

    pub fn detect(img: &Rgba) -> Result<Vec<Rect>, FaceError> {
        crate::ocr::winrt_thread();
        if !FaceDetector::IsSupported().map_err(os)? {
            return Err(FaceError::Unsupported);
        }
        let buf = CryptographicBuffer::CreateFromByteArray(&gray(img)).map_err(os)?;
        let bitmap = SoftwareBitmap::CreateCopyFromBuffer(
            &buf,
            BitmapPixelFormat::Gray8,
            img.width as i32,
            img.height as i32,
        )
        .map_err(os)?;
        let det = FaceDetector::CreateAsync()
            .map_err(os)?
            .join()
            .map_err(os)?;
        let faces = det
            .DetectFacesAsync(&bitmap)
            .map_err(os)?
            .join()
            .map_err(os)?;
        let mut out = Vec::new();
        for f in faces {
            let b = f.FaceBox().map_err(os)?;
            out.push(Rect {
                x: b.X as f32,
                y: b.Y as f32,
                w: b.Width as f32,
                h: b.Height as f32,
            });
        }
        Ok(out)
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use super::*;
    use objc2::rc::{Allocated, Retained, autoreleasepool};
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use objc2_foundation::{NSArray, NSData, NSDictionary, NSError, NSRect, NSString};

    pub fn detect(img: &Rgba) -> Result<Vec<Rect>, FaceError> {
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(
            image::RgbaImage::from_raw(img.width, img.height, img.pixels.clone())
                .ok_or(FaceError::Unsupported)?,
        )
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(|e| FaceError::Os(e.to_string()))?;
        let png = png.into_inner();
        autoreleasepool(|_| {
            // SAFETY: Vision API through the runtime, as in ocr/mac.rs.
            unsafe {
                let req: Retained<AnyObject> =
                    msg_send![class!(VNDetectFaceRectanglesRequest), new];
                let data = NSData::with_bytes(&png);
                let options = NSDictionary::<NSString, AnyObject>::new();
                let a: Allocated<AnyObject> = msg_send![class!(VNImageRequestHandler), alloc];
                let handler: Retained<AnyObject> =
                    msg_send![a, initWithData: &*data, options: &*options];
                let reqs = NSArray::from_retained_slice(std::slice::from_ref(&req));
                let done: Result<(), Retained<NSError>> =
                    msg_send![&handler, performRequests: &*reqs, error: _];
                done.map_err(|e| FaceError::Os(e.localizedDescription().to_string()))?;
                let results: Option<Retained<NSArray<AnyObject>>> = msg_send![&req, results];
                let (w, h) = (img.width as f32, img.height as f32);
                Ok(results
                    .iter()
                    .flat_map(|a| a.iter())
                    .map(|obs| {
                        let b: NSRect = msg_send![&obs, boundingBox];
                        Rect {
                            x: b.origin.x as f32 * w,
                            y: (1.0 - (b.origin.y + b.size.height) as f32) * h,
                            w: b.size.width as f32 * w,
                            h: b.size.height as f32 * h,
                        }
                    })
                    .collect())
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gray_of_white_and_transparent() {
        let img = Rgba::new(2, 1, vec![255, 255, 255, 255, 0, 0, 0, 0]).unwrap();
        assert_eq!(gray(&img), [255, 255]);
    }

    /// A blank picture has no faces (and the OS call works at all). A real face is checked in
    /// znimok-mask's tests with a drawn schematic face where the engine accepts it.
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn blank_picture_has_no_faces() {
        let img = Rgba::new(320, 240, vec![230; 320 * 240 * 4]).unwrap();
        match detect(&img) {
            Ok(f) => assert!(f.is_empty(), "{f:?}"),
            Err(FaceError::Unsupported) => {}
            Err(e) => panic!("{e}"),
        }
    }
}
