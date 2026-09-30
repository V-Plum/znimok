//! Pictures in and out: opening image files, export to PNG/JPEG/WebP, the clipboard.
//! The clipboard goes through `arboard` for now; the platform `Clipboard` trait (ZK-42) replaces
//! it once the Windows and macOS implementations land.

use std::path::Path;

use znimok_core::Raster;

/// A picture file as straight RGBA (ZK-65). The file is read into memory first, so it is not
/// held open (a cloud drive or another program can change it meanwhile). The `image` crate reads
/// PNG, JPEG, WebP, GIF and BMP by their content; anything else (HEIC, AVIF, TIFF…) goes to the
/// system's decoders — WIC on Windows (HEIC and AVIF need Microsoft's free extensions), ImageIO
/// on macOS.
pub fn load_image(path: &Path) -> Result<Raster, String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    let ours = image::load_from_memory(&data).map(|i| i.to_rgba8());
    match ours {
        Ok(img) => {
            let (w, h) = img.dimensions();
            Ok(Raster::new(w, h, img.into_raw()))
        }
        Err(e) => os_decode(&data).ok_or_else(|| e.to_string()),
    }
}

#[cfg(windows)]
fn os_decode(data: &[u8]) -> Option<Raster> {
    use windows::Win32::Graphics::Imaging::{
        CLSID_WICImagingFactory, GUID_WICPixelFormat32bppRGBA, IWICImagingFactory,
        WICBitmapDitherTypeNone, WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnDemand,
    };
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    };
    // SAFETY: COM for this thread (already on: S_FALSE; another mode: still usable), then
    // plain WIC calls on a stream over our own buffer, which outlives them.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
        let stream = factory.CreateStream().ok()?;
        stream.InitializeFromMemory(data).ok()?;
        let dec = factory
            .CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)
            .ok()?;
        let frame = dec.GetFrame(0).ok()?;
        let conv = factory.CreateFormatConverter().ok()?;
        conv.Initialize(
            &frame,
            &GUID_WICPixelFormat32bppRGBA,
            WICBitmapDitherTypeNone,
            None,
            0.0,
            WICBitmapPaletteTypeCustom,
        )
        .ok()?;
        let (mut w, mut h) = (0u32, 0u32);
        conv.GetSize(&mut w, &mut h).ok()?;
        if w == 0 || h == 0 || (w as u64) * (h as u64) > 400_000_000 {
            return None;
        }
        let mut buf = vec![0u8; (w * h * 4) as usize];
        conv.CopyPixels(std::ptr::null(), w * 4, &mut buf).ok()?;
        Some(Raster::new(w, h, buf))
    }
}

#[cfg(target_os = "macos")]
fn os_decode(data: &[u8]) -> Option<Raster> {
    use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep};
    use objc2_foundation::{NSData, NSDictionary};
    // ImageIO behind NSBitmapImageRep reads HEIC, AVIF, TIFF…; it hands back a PNG we decode.
    let png = objc2::exception::catch(|| {
        let d = NSData::with_bytes(data);
        let rep = NSBitmapImageRep::imageRepWithData(&d)?;
        // SAFETY: an empty properties dictionary.
        let out = unsafe {
            rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
        }?;
        Some(out.to_vec())
    })
    .ok()
    .flatten()?;
    let img = image::load_from_memory_with_format(&png, image::ImageFormat::Png)
        .ok()?
        .to_rgba8();
    let (w, h) = img.dimensions();
    Some(Raster::new(w, h, img.into_raw()))
}

#[cfg(not(any(windows, target_os = "macos")))]
fn os_decode(_: &[u8]) -> Option<Raster> {
    None
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
/// Writes PNG / JPEG / WebP by the extension. With `meta` (and the "write metadata" switch on)
/// the file carries the title, description, author, rights, tags and the shot's time (ZK-61);
/// the file's own time becomes the shot's time either way.
pub fn write_image(
    path: &Path,
    w: u32,
    h: u32,
    rgba: Vec<u8>,
    meta: Option<&crate::filemeta::FileMeta>,
) -> Result<(), String> {
    let opts = Encode {
        format: Format::for_path(path),
        quality: 92,
        lossless: true,
        white_bg: false,
    };
    let bytes = encode(w, h, &rgba, opts, meta)?;
    std::fs::write(path, bytes).map_err(|e| e.to_string())?;
    if let Some(m) = meta {
        crate::filemeta::set_file_time(path, m.created_ms);
    }
    Ok(())
}

/// How a picture is written (ZK-187): the format, its quality, transparency over white.
#[derive(Clone, Copy, Debug)]
pub struct Encode {
    pub format: Format,
    /// JPEG, and WebP when not lossless (ZK-197): 1–100.
    pub quality: u8,
    /// WebP only: without loss (image-webp) instead of libwebp's lossy coding.
    pub lossless: bool,
    /// Transparent pixels over white (JPEG always, having no alpha).
    pub white_bg: bool,
}

/// The file's bytes. Metadata goes in when `meta` is given and writing it is switched on.
pub fn encode(
    w: u32,
    h: u32,
    rgba: &[u8],
    opts: Encode,
    meta: Option<&crate::filemeta::FileMeta>,
) -> Result<Vec<u8>, String> {
    use image::ImageEncoder;
    let written = meta.filter(|_| crate::filemeta::enabled());
    let over_white = |p: &[u8]| {
        let a = p[3] as u32;
        let mix = |c: u8| ((c as u32 * a + 255 * (255 - a)) / 255) as u8;
        [mix(p[0]), mix(p[1]), mix(p[2])]
    };
    let flat: Option<Vec<u8>> = (opts.white_bg && opts.format != Format::Jpeg).then(|| {
        rgba.as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| {
                let [r, g, b] = over_white(p);
                [r, g, b, 255]
            })
            .collect()
    });
    let rgba = flat.as_deref().unwrap_or(rgba);
    if rgba.len() != w as usize * h as usize * 4 {
        return Err("image buffer size mismatch".into());
    }
    let mut out = Vec::new();
    let err = |e: image::ImageError| e.to_string();
    match opts.format {
        Format::Png => {
            let mut enc = png::Encoder::new(&mut out, w, h);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            if let Some(m) = written {
                for (k, t) in m.png_text() {
                    enc.add_itxt_chunk(k.to_string(), t)
                        .map_err(|e| e.to_string())?;
                }
            }
            let mut wr = enc.write_header().map_err(|e| e.to_string())?;
            wr.write_image_data(rgba).map_err(|e| e.to_string())?;
            wr.finish().map_err(|e| e.to_string())?;
        }
        Format::Webp if !opts.lossless => {
            // libwebp (ZK-197); alpha is kept (an ALPH chunk) unless flattened above.
            let enc = webp::Encoder::from_rgba(rgba, w, h);
            let q = opts.quality.clamp(1, 100) as f32;
            let bytes = enc
                .encode_simple(false, q)
                .map_err(|e| format!("WebP: {e:?}"))?;
            out = bytes.to_vec();
            if let Some(m) = written {
                out = webp_with_exif(&out, w, h, &m.exif()).unwrap_or(out);
            }
        }
        Format::Webp => {
            let mut enc = image::codecs::webp::WebPEncoder::new_lossless(&mut out);
            if let Some(m) = written {
                let _ = enc.set_exif_metadata(m.exif());
            }
            enc.write_image(rgba, w, h, image::ExtendedColorType::Rgba8)
                .map_err(err)?;
        }
        Format::Jpeg => {
            let rgb: Vec<u8> = rgba
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| over_white(p))
                .collect();
            let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(
                &mut out,
                opts.quality.clamp(1, 100),
            );
            if let Some(m) = written {
                let _ = enc.set_exif_metadata(m.exif());
            }
            enc.write_image(&rgb, w, h, image::ExtendedColorType::Rgb8)
                .map_err(err)?;
        }
    }
    Ok(out)
}

/// libwebp's file with an EXIF chunk added (ZK-197; its simple encoder writes none): the
/// extended header (VP8X) is added or gets the EXIF flag, the chunk goes last, the RIFF size
/// follows. `None` for anything that is not a WebP this function understands.
fn webp_with_exif(file: &[u8], w: u32, h: u32, exif: &[u8]) -> Option<Vec<u8>> {
    if file.len() < 20 || &file[0..4] != b"RIFF" || &file[8..12] != b"WEBP" {
        return None;
    }
    let chunk = |fourcc: &[u8], payload: &[u8]| {
        let mut c = fourcc.to_vec();
        c.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        c.extend_from_slice(payload);
        if payload.len() % 2 == 1 {
            c.push(0);
        }
        c
    };
    let mut body = file[12..].to_vec();
    if &body[0..4] == b"VP8X" {
        body[8] |= 0x08;
    } else {
        let (cw, ch) = (w.checked_sub(1)?, h.checked_sub(1)?);
        let mut x = vec![0x08, 0, 0, 0];
        x.extend_from_slice(&cw.to_le_bytes()[..3]);
        x.extend_from_slice(&ch.to_le_bytes()[..3]);
        let mut with = chunk(b"VP8X", &x);
        with.extend_from_slice(&body);
        body = with;
    }
    body.extend_from_slice(&chunk(b"EXIF", exif));
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&((body.len() + 4) as u32).to_le_bytes());
    out.extend_from_slice(b"WEBP");
    out.extend_from_slice(&body);
    Some(out)
}

/// The picture scaled to `w2`×`h2` (Lanczos down, Catmull–Rom up); the same size is returned as is.
pub fn scaled(w: u32, h: u32, rgba: Vec<u8>, w2: u32, h2: u32) -> (u32, u32, Vec<u8>) {
    let (w2, h2) = (w2.max(1), h2.max(1));
    if (w2, h2) == (w, h) {
        return (w, h, rgba);
    }
    let Some(img) = image::RgbaImage::from_raw(w, h, rgba) else {
        return (0, 0, Vec::new());
    };
    let filter = if w2 < w {
        image::imageops::FilterType::Lanczos3
    } else {
        image::imageops::FilterType::CatmullRom
    };
    let out = image::imageops::resize(&img, w2, h2, filter);
    (w2, h2, out.into_raw())
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

pub const IMAGE_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "webp", "gif", "bmp", "heic", "heif", "avif", "tif", "tiff",
];

#[cfg(test)]
mod tests {
    use super::*;

    /// ZK-197: libwebp's WebP gets smaller with a lower quality and stays smaller than the
    /// lossless one; transparency survives; the EXIF chunk is readable by another decoder.
    #[test]
    fn webp_with_quality() {
        let (w, h) = (240u32, 160u32);
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let n = ((x * 7919 + y * 104_729) % 97) as u8;
                let a = if x < 20 { 0 } else { 255 };
                rgba.extend_from_slice(&[(x as u8).wrapping_add(n), (y as u8) ^ n, 128, a]);
            }
        }
        let enc = |quality, lossless| {
            let opts = Encode {
                format: Format::Webp,
                quality,
                lossless,
                white_bg: false,
            };
            encode(w, h, &rgba, opts, None).unwrap()
        };
        let (low, high, exact) = (enc(40, false), enc(90, false), enc(90, true));
        assert!(
            low.len() < high.len() && high.len() < exact.len(),
            "{} {} {}",
            low.len(),
            high.len(),
            exact.len()
        );
        let back = image::load_from_memory(&low).unwrap().to_rgba8();
        assert_eq!(back.dimensions(), (w, h));
        assert!(back.get_pixel(5, 5)[3] < 10 && back.get_pixel(100, 5)[3] > 245);

        let exif = b"MM\0*\0\0\0\x08\0\0".to_vec();
        let with = webp_with_exif(&low, w, h, &exif).unwrap();
        use image::ImageDecoder;
        let mut dec = image::codecs::webp::WebPDecoder::new(std::io::Cursor::new(&with)).unwrap();
        assert_eq!(dec.exif_metadata().unwrap(), Some(exif));
        assert_eq!(dec.dimensions(), (w, h));
    }

    /// A 2 × 1 uncompressed RGB TIFF, written by hand: the `image` crate here has no TIFF, so
    /// this goes through the system decoder (ZK-65).
    fn tiff() -> Vec<u8> {
        let mut t = b"II*\0".to_vec();
        t.extend_from_slice(&8u32.to_le_bytes());
        let entries: [(u16, u16, u32, u32); 9] = [
            (256, 3, 1, 2),   // width
            (257, 3, 1, 1),   // height
            (258, 3, 3, 122), // bits per sample → offset
            (259, 3, 1, 1),   // no compression
            (262, 3, 1, 2),   // RGB
            (273, 4, 1, 128), // strip offset
            (277, 3, 1, 3),   // samples per pixel
            (278, 3, 1, 1),   // rows per strip
            (279, 4, 1, 6),   // strip bytes
        ];
        t.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        for (tag, ty, n, v) in entries {
            t.extend_from_slice(&tag.to_le_bytes());
            t.extend_from_slice(&ty.to_le_bytes());
            t.extend_from_slice(&n.to_le_bytes());
            t.extend_from_slice(&v.to_le_bytes());
        }
        t.extend_from_slice(&0u32.to_le_bytes());
        assert_eq!(t.len(), 122);
        for _ in 0..3 {
            t.extend_from_slice(&8u16.to_le_bytes());
        }
        assert_eq!(t.len(), 128);
        t.extend_from_slice(&[255, 0, 0, 0, 0, 255]);
        t
    }

    #[test]
    #[cfg(any(windows, target_os = "macos"))]
    fn system_decoder_reads_what_image_does_not() {
        let dir = std::env::temp_dir().join(format!("znimok-io-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("two.tif");
        std::fs::write(&path, tiff()).unwrap();
        let r = load_image(&path).expect("the system decodes TIFF");
        assert_eq!((r.width, r.height), (2, 1));
        assert_eq!(&r.rgba[..4], &[255, 0, 0, 255]);
        assert_eq!(&r.rgba[4..8], &[0, 0, 255, 255]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
