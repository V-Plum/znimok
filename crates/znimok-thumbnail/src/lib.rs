//! Thumbnails of `.znimok` files in Explorer (ZK-76).
//!
//! A COM in-process server with one class, [`CLSID`], implementing `IInitializeWithStream` +
//! `IThumbnailProvider`. Explorer runs such handlers in an isolated surrogate process and hands
//! them a stream, not a path. We read only the head of the file — the stored `THMB` block (a PNG
//! the app renders when it saves, ≤ 320×240) — never the pixels, so a huge document costs the
//! same as a small one and a damaged pixel block cannot hurt Explorer.
//!
//! Registration is per user (`HKCU\Software\Classes`, no admin rights): `regsvr32 /s
//! znimok_thumbnail.dll` from the installer, or [`register`] from the app.
//!
//! The pure part — [`thumbnail_rgba`] — is platform-independent and shared with the macOS Quick
//! Look extension.

/// `{787777D8-E076-4FDC-8065-F7282E5D3F86}` — Znimok's thumbnail handler. Never change it: Windows
/// caches thumbnails by handler.
pub const CLSID_STR: &str = "{787777D8-E076-4FDC-8065-F7282E5D3F86}";

/// How much of the file is read before the stored thumbnail must have appeared.
#[cfg(windows)]
const HEAD_LIMIT: usize = 64 << 20;

/// A stored thumbnail is at most 320×240 (the app writes it); a file claiming more is not ours —
/// decoding it unbounded in Explorer's or Finder's process would be a memory bomb (ZK-113).
pub fn thumb_limits() -> znimok_format::Limits {
    znimok_format::Limits::thumbnail()
}

/// The stored thumbnail of a `.znimok` file, scaled to fit `size`×`size`, as straight RGBA.
/// `None` if the bytes are not a Znimok document or it has no thumbnail.
pub fn thumbnail_rgba(head: &[u8], size: u32) -> Option<(u32, u32, Vec<u8>)> {
    let peek = znimok_format::peek(head).ok()?;
    let png = peek.thumbnail_png?;
    let r = znimok_format::decode_png(&png, &thumb_limits()).ok()?;
    let img = image::RgbaImage::from_raw(r.width, r.height, r.rgba)?;
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        return None;
    }
    let scale = (size as f32 / w.max(h) as f32).min(1.0);
    let (tw, th) = (
        ((w as f32 * scale).round() as u32).max(1),
        ((h as f32 * scale).round() as u32).max(1),
    );
    let img = if (tw, th) == (w, h) {
        img
    } else {
        image::imageops::resize(&img, tw, th, image::imageops::FilterType::Triangle)
    };
    Some((tw, th, img.into_raw()))
}

/// The stored thumbnail PNG of a `.znimok` file, for the macOS Quick Look extension (Swift).
/// Returns a buffer to free with [`znimok_thumbnail_free`], or null (not a document, no
/// thumbnail, or the head is not complete yet — then call again with more of the file).
///
/// # Safety
/// `data` points to `len` readable bytes; `out_len` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn znimok_thumbnail_png(
    data: *const u8,
    len: usize,
    out_len: *mut usize,
) -> *mut u8 {
    if data.is_null() || out_len.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: the caller's contract.
    let head = unsafe { std::slice::from_raw_parts(data, len) };
    let Some(png) = znimok_format::peek(head).ok().and_then(|p| p.thumbnail_png) else {
        return std::ptr::null_mut();
    };
    // Checked here with the bounded decoder, so Quick Look never decodes an oversized one.
    if znimok_format::decode_png(&png, &thumb_limits()).is_err() {
        return std::ptr::null_mut();
    }
    let b = png.into_boxed_slice();
    // SAFETY: the caller's contract.
    unsafe { *out_len = b.len() };
    Box::into_raw(b).cast()
}

/// Frees a buffer from [`znimok_thumbnail_png`].
///
/// # Safety
/// `p` and `len` exactly as returned; each buffer freed once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn znimok_thumbnail_free(p: *mut u8, len: usize) {
    if !p.is_null() {
        // SAFETY: rebuilds the box handed out above.
        drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(p, len)) });
    }
}

#[cfg(windows)]
mod win;

#[cfg(windows)]
pub use win::{Scope, register, register_in, unregister, unregister_in};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_thumbnail_is_read_and_scaled() {
        let mut doc = znimok_format_doc();
        let big = znimok_format::write(
            &doc,
            &znimok_format::WriteOptions {
                thumbnail: Some(solid(320, 200, [10, 200, 30, 255])),
                ..Default::default()
            },
        );
        let (w, h, px) = thumbnail_rgba(&big, 96).unwrap();
        assert_eq!((w, h), (96, 60));
        assert_eq!(&px[..4], &[10, 200, 30, 255]);
        // Smaller than asked: kept as it is.
        assert_eq!(thumbnail_rgba(&big, 1000).unwrap().0, 320);
        // No thumbnail stored, or not a document.
        doc.name = "без мініатюри".into();
        let bare = znimok_format::write(&doc, &Default::default());
        assert!(thumbnail_rgba(&bare, 96).is_none());
        assert!(thumbnail_rgba(b"PNG? no", 96).is_none());
    }

    #[test]
    fn c_api_for_quick_look() {
        let bytes = znimok_format::write(
            &znimok_format_doc(),
            &znimok_format::WriteOptions {
                thumbnail: Some(solid(32, 20, [1, 2, 3, 255])),
                ..Default::default()
            },
        );
        let mut len = 0usize;
        // SAFETY: valid buffer and out pointer; the result is freed once.
        unsafe {
            let p = znimok_thumbnail_png(bytes.as_ptr(), bytes.len(), &mut len);
            assert!(!p.is_null() && len > 8);
            let png = std::slice::from_raw_parts(p, len);
            assert_eq!(&png[1..4], b"PNG");
            znimok_thumbnail_free(p, len);
            assert!(znimok_thumbnail_png(b"nope".as_ptr(), 4, &mut len).is_null());
        }
    }

    /// A thumbnail claiming a huge size is refused without allocating it (ZK-113).
    #[test]
    fn oversized_stored_thumbnail_is_refused() {
        let bytes = znimok_format::write(
            &znimok_format_doc(),
            &znimok_format::WriteOptions {
                thumbnail: Some(solid(2000, 10, [1, 2, 3, 255])),
                ..Default::default()
            },
        );
        assert!(thumbnail_rgba(&bytes, 96).is_none());
        let mut len = 0usize;
        // SAFETY: valid buffer and out pointer.
        assert!(unsafe { znimok_thumbnail_png(bytes.as_ptr(), bytes.len(), &mut len) }.is_null());
    }

    pub(crate) fn solid(w: u32, h: u32, c: [u8; 4]) -> znimok_core::Raster {
        znimok_core::Raster::new(
            w,
            h,
            c.iter()
                .copied()
                .cycle()
                .take((w * h * 4) as usize)
                .collect(),
        )
    }

    pub(crate) fn znimok_format_doc() -> znimok_core::Document {
        znimok_core::Document::from_raster("t", solid(640, 400, [255, 255, 255, 255]))
    }
}
