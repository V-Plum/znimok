//! Pictures for Claude: scaled down on our side by the same rule the API applies, so the preview
//! shows what the model will see and the token estimate is exact.
//!
//! Current models (Claude 4.7 and later) see at most a 2576 px long edge and 4784 visual tokens,
//! a token being a 28×28 block: `⌈w/28⌉·⌈h/28⌉`. Up to 20 images per request no stricter limit
//! applies. A picture is sent as PNG (text stays sharp); only if that exceeds the 10 MB base64
//! limit it becomes a high-quality JPEG.

use crate::Rgba;
use base64::Engine;

pub const MAX_LONG_EDGE: u32 = 2576;
pub const MAX_VISUAL_TOKENS: u64 = 4784;
/// Base64 size limit per image on the Claude API.
pub const MAX_BASE64_BYTES: usize = 10 * 1024 * 1024;
pub const MAX_IMAGES: usize = 20;

pub fn visual_tokens(w: u32, h: u32) -> u64 {
    (w as u64).div_ceil(28) * (h as u64).div_ceil(28)
}

/// The largest size with the same aspect ratio within both limits.
pub fn fitted_size(w: u32, h: u32) -> (u32, u32) {
    let mut scale = (MAX_LONG_EDGE as f64 / w.max(h) as f64).min(1.0);
    let size = |s: f64| {
        (
            ((w as f64 * s).floor() as u32).max(1),
            ((h as f64 * s).floor() as u32).max(1),
        )
    };
    let (mut fw, mut fh) = size(scale);
    while visual_tokens(fw, fh) > MAX_VISUAL_TOKENS {
        scale *= 0.99;
        (fw, fh) = size(scale);
    }
    (fw, fh)
}

/// One picture ready to send.
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    pub width: u32,
    pub height: u32,
    /// `image/png` or `image/jpeg`.
    pub media_type: &'static str,
    pub base64: String,
    /// What was sent, for the preview (the scaled picture itself).
    pub bytes: Vec<u8>,
    pub visual_tokens: u64,
}

#[derive(Debug, PartialEq)]
pub enum PrepError {
    Encode(String),
    TooLarge,
}

pub fn prepare(img: &Rgba) -> Result<Prepared, PrepError> {
    let (w, h) = fitted_size(img.width, img.height);
    let buf = image::RgbaImage::from_raw(img.width, img.height, img.pixels.clone())
        .ok_or_else(|| PrepError::Encode("pixel data does not match the size".into()))?;
    let scaled = if (w, h) == (img.width, img.height) {
        buf
    } else {
        image::imageops::resize(&buf, w, h, image::imageops::FilterType::Lanczos3)
    };
    let engine = &base64::engine::general_purpose::STANDARD;
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(scaled.clone())
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(|e| PrepError::Encode(e.to_string()))?;
    let png = png.into_inner();
    let (media_type, bytes) = if png.len().div_ceil(3) * 4 <= MAX_BASE64_BYTES {
        ("image/png", png)
    } else {
        // Flatten on white: JPEG has no alpha.
        let rgb = image::DynamicImage::ImageRgba8(scaled).to_rgb8();
        let mut jpg = std::io::Cursor::new(Vec::new());
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg, 90)
            .encode_image(&rgb)
            .map_err(|e| PrepError::Encode(e.to_string()))?;
        ("image/jpeg", jpg.into_inner())
    };
    let b64 = engine.encode(&bytes);
    if b64.len() > MAX_BASE64_BYTES {
        return Err(PrepError::TooLarge);
    }
    Ok(Prepared {
        width: w,
        height: h,
        media_type,
        base64: b64,
        bytes,
        visual_tokens: visual_tokens(w, h),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_follow_the_documented_table() {
        // From the vision docs, high-resolution tier.
        assert_eq!(fitted_size(1920, 1080), (1920, 1080));
        assert_eq!(visual_tokens(1920, 1080), 2691);
        assert_eq!(fitted_size(2000, 1500), (2000, 1500));
        assert_eq!(visual_tokens(1000, 1000), 1296);
        let (w, h) = fitted_size(3840, 2160);
        assert_eq!(w, 2576);
        assert!((1448..=1449).contains(&h), "{h}");
        assert!(visual_tokens(w, h) <= MAX_VISUAL_TOKENS);
    }

    #[test]
    fn token_limit_binds_for_square_pictures() {
        let (w, h) = fitted_size(4000, 4000);
        assert!(w.max(h) <= MAX_LONG_EDGE);
        assert!(visual_tokens(w, h) <= MAX_VISUAL_TOKENS);
        assert!(
            visual_tokens(w + 28, h + 28) > MAX_VISUAL_TOKENS,
            "{w}×{h} is not the largest"
        );
        assert_eq!(fitted_size(10, 5), (10, 5));
    }

    #[test]
    fn prepared_png_is_what_was_measured() {
        let img = Rgba::new(3000, 1000, vec![200; 3000 * 1000 * 4]).unwrap();
        let p = prepare(&img).unwrap();
        assert_eq!(p.media_type, "image/png");
        assert_eq!((p.width, p.height), fitted_size(3000, 1000));
        let back = image::load_from_memory(&p.bytes).unwrap();
        assert_eq!((back.width(), back.height()), (p.width, p.height));
        assert_eq!(p.visual_tokens, visual_tokens(p.width, p.height));
        use base64::Engine;
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(&p.base64)
                .unwrap(),
            p.bytes
        );
    }
}
