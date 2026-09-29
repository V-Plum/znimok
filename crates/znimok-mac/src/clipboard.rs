//! Clipboard on macOS (ZK-42, ZK-62): `NSPasteboard.generalPasteboard`.
//!
//! - **write:** one pasteboard item with `public.png` (alpha, metadata) + `public.tiff` (older
//!   programs) and, for «Копіювати як файл», the `public.file-url` of the first file on the same
//!   item — Finder pastes the file, Messages / Telegram attach it, image editors take the pixels.
//!   Further files get their own items; text goes as `public.utf8-plain-text`.
//! - **read:** PNG first, then TIFF, then anything `NSImage` understands (JPEG, PDF…) through its
//!   TIFF form; file URLs of all items; text. Pixels are converted to PNG by AppKit and decoded
//!   here, so there is one decoder. Alpha zero everywhere → opaque, as on Windows.

use std::path::PathBuf;

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{AllocAnyThread, ClassType};
use objc2_app_kit::{
    NSBitmapImageFileType, NSBitmapImageRep, NSImage, NSPasteboard, NSPasteboardItem,
    NSPasteboardTypeFileURL, NSPasteboardTypePNG, NSPasteboardTypeString, NSPasteboardTypeTIFF,
    NSPasteboardWriting,
};
use objc2_foundation::{NSArray, NSData, NSDictionary, NSString, NSURL};
use znimok_platform::{ClipImage, ClipItem, Clipboard, PlatformError, Result};

#[derive(Default)]
pub struct MacClipboard;

impl MacClipboard {
    pub fn new() -> Self {
        Self
    }
}

fn err(what: &str) -> PlatformError {
    PlatformError::Other(format!("clipboard: {what}"))
}

/// `+[NSPasteboard generalPasteboard]` is nil without a login session (ssh, a launch daemon):
/// objc2's wrapper panics on that, so it is called by hand and nil becomes an error.
fn general() -> Result<Retained<NSPasteboard>> {
    // SAFETY: a class method without arguments returning an autoreleased object or nil.
    let pb: Option<Retained<NSPasteboard>> =
        unsafe { objc2::msg_send![NSPasteboard::class(), generalPasteboard] };
    pb.ok_or(PlatformError::Unsupported(
        "no pasteboard in this session (no GUI login, e.g. ssh)",
    ))
}

impl Clipboard for MacClipboard {
    fn write(&self, items: &[ClipItem]) -> Result<()> {
        let mut out: Vec<Retained<NSPasteboardItem>> = Vec::new();
        let mut image_item: Option<Retained<NSPasteboardItem>> = None;
        for item in items {
            match item {
                ClipItem::Image(img) => {
                    let png = match &img.png {
                        Some(p) => p.clone(),
                        None => encode_png(img)?,
                    };
                    let it = NSPasteboardItem::new();
                    let png = NSData::with_bytes(&png);
                    // SAFETY: framework constants.
                    let (t_png, t_tiff) = unsafe { (NSPasteboardTypePNG, NSPasteboardTypeTIFF) };
                    it.setData_forType(&png, t_png);
                    if let Some(tiff) = NSBitmapImageRep::imageRepWithData(&png)
                        .and_then(|r| r.TIFFRepresentation())
                    {
                        it.setData_forType(&tiff, t_tiff);
                    }
                    image_item = Some(it.clone());
                    out.push(it);
                }
                ClipItem::Files(files) => {
                    for (i, f) in files.iter().enumerate() {
                        let url = NSURL::fileURLWithPath(&NSString::from_str(&f.to_string_lossy()));
                        let Some(s) = url.absoluteString() else {
                            continue;
                        };
                        // SAFETY: framework constant.
                        let t = unsafe { NSPasteboardTypeFileURL };
                        match (&image_item, i) {
                            (Some(it), 0) => {
                                it.setString_forType(&s, t);
                            }
                            _ => {
                                let it = NSPasteboardItem::new();
                                it.setString_forType(&s, t);
                                out.push(it);
                            }
                        }
                    }
                }
                ClipItem::Text(text) => {
                    let it = NSPasteboardItem::new();
                    // SAFETY: framework constant.
                    it.setString_forType(&NSString::from_str(text), unsafe {
                        NSPasteboardTypeString
                    });
                    out.push(it);
                }
            }
        }
        let pb = general()?;
        pb.clearContents();
        let objs: Vec<Retained<ProtocolObject<dyn NSPasteboardWriting>>> =
            out.into_iter().map(ProtocolObject::from_retained).collect();
        if objs.is_empty() || pb.writeObjects(&NSArray::from_retained_slice(&objs)) {
            Ok(())
        } else {
            Err(err("the pasteboard refused the items"))
        }
    }

    fn read(&self) -> Result<Vec<ClipItem>> {
        let pb = general()?;
        let mut items = Vec::new();
        // SAFETY: framework constants.
        let (t_png, t_tiff, t_url, t_text) = unsafe {
            (
                NSPasteboardTypePNG,
                NSPasteboardTypeTIFF,
                NSPasteboardTypeFileURL,
                NSPasteboardTypeString,
            )
        };
        let from_png = |png: Vec<u8>, keep: bool| {
            decode_png(&png).map(|(w, h, rgba)| ClipImage {
                width: w,
                height: h,
                rgba,
                png: keep.then_some(png),
            })
        };
        let image = pb
            .dataForType(t_png)
            .and_then(|d| from_png(d.to_vec(), true))
            .or_else(|| {
                let tiff = pb.dataForType(t_tiff).or_else(|| {
                    NSImage::initWithPasteboard(NSImage::alloc(), &pb)
                        .and_then(|i| i.TIFFRepresentation())
                })?;
                from_png(png_from_image_data(&tiff)?, false)
            });
        if let Some(mut img) = image {
            fix_opacity(&mut img.rgba);
            items.push(ClipItem::Image(img));
        }
        let files: Vec<PathBuf> = pb
            .pasteboardItems()
            .map(|all| {
                all.iter()
                    .filter_map(|it| {
                        let s = it.stringForType(t_url)?;
                        let url = NSURL::URLWithString(&s)?;
                        url.path().map(|p| PathBuf::from(p.to_string()))
                    })
                    .collect()
            })
            .unwrap_or_default();
        if !files.is_empty() {
            items.push(ClipItem::Files(files));
        }
        if let Some(s) = pb.stringForType(t_text) {
            items.push(ClipItem::Text(s.to_string()));
        }
        Ok(items)
    }
}

/// Any image data AppKit reads (TIFF, JPEG…) → PNG bytes.
fn png_from_image_data(data: &NSData) -> Option<Vec<u8>> {
    let rep = NSBitmapImageRep::imageRepWithData(data)?;
    // SAFETY: an empty properties dictionary is valid for PNG.
    let png = unsafe {
        rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
    }?;
    Some(png.to_vec())
}

fn encode_png(img: &ClipImage) -> Result<Vec<u8>> {
    if img.width == 0 || img.height == 0 || img.rgba.len() != (img.width * img.height * 4) as usize
    {
        return Err(err("image size and pixel data differ"));
    }
    let mut out = std::io::Cursor::new(Vec::new());
    image::write_buffer_with_format(
        &mut out,
        &img.rgba,
        img.width,
        img.height,
        image::ExtendedColorType::Rgba8,
        image::ImageFormat::Png,
    )
    .map_err(|e| err(&format!("PNG: {e}")))?;
    Ok(out.into_inner())
}

fn decode_png(png: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    // Any program can put a PNG on the clipboard: a small file that inflates to gigabytes must
    // be refused before the pixels are allocated (ZK-113).
    let mut r = image::ImageReader::with_format(std::io::Cursor::new(png), image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(1 << 15);
    limits.max_image_height = Some(1 << 15);
    limits.max_alloc = Some(1 << 30);
    r.limits(limits);
    let rgba = r.decode().ok()?.into_rgba8();
    Some((rgba.width(), rgba.height(), rgba.into_raw()))
}

fn fix_opacity(rgba: &mut [u8]) {
    if rgba.as_chunks::<4>().0.iter().all(|p| p[3] == 0) {
        for p in rgba.as_chunks_mut::<4>().0 {
            p[3] = 255;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use znimok_platform::conformance;

    /// Without a GUI session (ssh) the calls fail cleanly instead of panicking.
    #[test]
    fn no_session_is_an_error_not_a_panic() {
        if general().is_err() {
            assert!(MacClipboard::new().read().is_err());
            assert!(
                MacClipboard::new()
                    .write(&[ClipItem::Text("x".into())])
                    .is_err()
            );
        }
    }

    #[test]
    fn opacity_rule() {
        let mut px = vec![1, 2, 3, 0, 4, 5, 6, 0];
        fix_opacity(&mut px);
        assert_eq!(px, [1, 2, 3, 255, 4, 5, 6, 255]);
    }

    /// The real pasteboard — only on CI or with `ZNIMOK_LIVE_CLIPBOARD=1` (it replaces what the
    /// person copied).
    #[test]
    fn live_pasteboard() {
        if std::env::var_os("CI").is_none() && std::env::var_os("ZNIMOK_LIVE_CLIPBOARD").is_none() {
            eprintln!("skipped: set ZNIMOK_LIVE_CLIPBOARD=1 to use the real pasteboard");
            return;
        }
        let c = MacClipboard::new();
        if general().is_err() {
            eprintln!("skipped: no pasteboard in this session (ssh)");
            return;
        }
        conformance::clipboard(&c).unwrap();

        let dir = std::env::temp_dir().join(format!("znimok-live-clip-{}", std::process::id()));
        let rgba: Vec<u8> = (0..12 * 8).flat_map(|i| [i as u8, 90, 200, 255]).collect();
        let img = ClipImage {
            width: 12,
            height: 8,
            rgba: rgba.clone(),
            png: None,
        };
        let file = znimok_platform::clipfile::write_clip_file(&dir, "Знімок 1", b"png").unwrap();
        c.write(&znimok_platform::clipfile::image_with_file(
            img,
            file.clone(),
        ))
        .unwrap();
        let got = c.read().unwrap();
        assert!(
            matches!(&got[0], ClipItem::Image(i) if i.rgba == rgba && i.png.is_some()),
            "{got:?}"
        );
        let canon = file.canonicalize().unwrap();
        assert!(
            got.iter().any(|i| matches!(i, ClipItem::Files(f)
                if f.len() == 1 && f[0].canonicalize().ok().as_ref() == Some(&canon))),
            "{got:?}"
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
