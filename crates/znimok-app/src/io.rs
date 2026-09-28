//! Pictures in and out: opening image files, export to PNG/JPEG/WebP, the clipboard.
//! The clipboard goes through `arboard` for now; the platform `Clipboard` trait (ZK-42) replaces
//! it once the Windows and macOS implementations land.

use std::io::Write;
use std::path::Path;

use znimok_core::Raster;

/// Any picture the `image` crate reads (PNG, JPEG, WebP, GIF, BMP) as straight RGBA.
pub fn load_image(path: &Path) -> Result<Raster, String> {
    let img = image::ImageReader::open(path)
        .map_err(|e| e.to_string())?
        .with_guessed_format()
        .map_err(|e| e.to_string())?
        .decode()
        .map_err(|e| e.to_string())?
        .to_rgba8();
    let (w, h) = img.dimensions();
    Ok(Raster::new(w, h, img.into_raw()))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    Png,
    Jpeg,
    Webp,
}

impl Format {
    pub fn for_path(path: &Path) -> Format {
        match path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .as_deref()
        {
            Some("jpg" | "jpeg") => Format::Jpeg,
            Some("webp") => Format::Webp,
            _ => Format::Png,
        }
    }
}

/// Same rules as `znimok export`: JPEG has no alpha, so transparent pixels go over white,
/// quality 92; WebP is lossless.
pub fn write_image(path: &Path, w: u32, h: u32, rgba: Vec<u8>) -> Result<(), String> {
    let img = image::RgbaImage::from_raw(w, h, rgba).ok_or("image buffer size mismatch")?;
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut out = std::io::BufWriter::new(file);
    let err = |e: image::ImageError| e.to_string();
    match Format::for_path(path) {
        Format::Png => image::DynamicImage::ImageRgba8(img)
            .write_to(&mut out, image::ImageFormat::Png)
            .map_err(err)?,
        Format::Webp => image::DynamicImage::ImageRgba8(img)
            .write_to(&mut out, image::ImageFormat::WebP)
            .map_err(err)?,
        Format::Jpeg => {
            let rgb = image::RgbImage::from_fn(w, h, |x, y| {
                let p = img.get_pixel(x, y).0;
                let a = p[3] as u32;
                let mix = |c: u8| ((c as u32 * a + 255 * (255 - a)) / 255) as u8;
                image::Rgb([mix(p[0]), mix(p[1]), mix(p[2])])
            });
            let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 92);
            rgb.write_with_encoder(enc).map_err(err)?;
        }
    }
    out.flush().map_err(|e| e.to_string())
}

pub fn copy_image(w: u32, h: u32, rgba: Vec<u8>) -> Result<(), String> {
    let mut cb = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    cb.set_image(arboard::ImageData {
        width: w as usize,
        height: h as usize,
        bytes: rgba.into(),
    })
    .map_err(|e| e.to_string())
}

/// The image on the clipboard, or `None` when there is none.
pub fn paste_image() -> Option<Raster> {
    let mut cb = arboard::Clipboard::new().ok()?;
    let img = cb.get_image().ok()?;
    let (w, h) = (img.width as u32, img.height as u32);
    let bytes = img.bytes.into_owned();
    (bytes.len() == (w * h * 4) as usize && w > 0 && h > 0).then(|| Raster::new(w, h, bytes))
}

pub const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "gif", "bmp"];
