//! Clipboard on Windows (ZK-42, ZK-62), by the Little Helpers rules (inventory_screenshots §5.5):
//!
//! - **write:** the registered `PNG` format (alpha, metadata) + `CF_DIB` (24-bit, bottom-up, rows
//!   padded to 4 bytes, transparency flattened on white). **No `CF_BITMAP`:** with it the Win+V
//!   history drops the whole entry (CAPS-62); Windows synthesises it from `CF_DIB` anyway. Files go
//!   as `CF_HDROP` + «Preferred DropEffect» = copy (Explorer pastes a copy, not a move); text as
//!   `CF_UNICODETEXT`.
//! - **read:** PNG first (`CF_DIBV5` is ambiguous: top-down heights, premultiplied alpha), then
//!   `CF_DIBV5`, `CF_DIB` (Windows makes both from a `CF_BITMAP`), files, text. A picture whose
//!   alpha is zero everywhere is made opaque (old programs leave the fourth byte at 0).
//!
//! `SetClipboardData` fails when the clipboard has no owner window, so every write opens it with a
//! throw-away message-only window of this thread.

use std::path::PathBuf;
use std::time::Duration;

use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{
    GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};
use windows::Win32::System::Ole::{CF_DIB, CF_DIBV5, CF_HDROP, CF_UNICODETEXT};
use windows::Win32::UI::Shell::{DROPFILES, DragQueryFileW, HDROP};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, HWND_MESSAGE, WINDOW_EX_STYLE, WINDOW_STYLE,
};
use windows::core::w;
use znimok_platform::{ClipImage, ClipItem, Clipboard, PlatformError, Result};

#[derive(Default)]
pub struct WinClipboard;

impl WinClipboard {
    pub fn new() -> Self {
        Self
    }
}

fn os(e: windows::core::Error) -> PlatformError {
    PlatformError::Os {
        code: e.code().0 as i64,
        message: e.message(),
    }
}

fn format(name: windows::core::PCWSTR) -> u32 {
    // SAFETY: a static NUL-terminated name.
    unsafe { RegisterClipboardFormatW(name) }
}

/// An open clipboard; closes it and destroys the owner window on drop.
struct Open(Option<HWND>);

impl Open {
    fn new(with_owner: bool) -> Result<Self> {
        let hwnd = if with_owner {
            // SAFETY: a message-only STATIC window of this thread, destroyed in drop.
            Some(
                unsafe {
                    CreateWindowExW(
                        WINDOW_EX_STYLE(0),
                        w!("STATIC"),
                        w!("Znimok clipboard"),
                        WINDOW_STYLE(0),
                        0,
                        0,
                        0,
                        0,
                        Some(HWND_MESSAGE),
                        None,
                        None,
                        None,
                    )
                }
                .map_err(os)?,
            )
        } else {
            None
        };
        // Another program may hold the clipboard for a moment.
        let mut last = None;
        for _ in 0..20 {
            // SAFETY: plain call.
            match unsafe { OpenClipboard(hwnd) } {
                Ok(()) => return Ok(Self(hwnd)),
                Err(e) => last = Some(e),
            }
            std::thread::sleep(Duration::from_millis(15));
        }
        if let Some(h) = hwnd {
            // SAFETY: our window.
            let _ = unsafe { DestroyWindow(h) };
        }
        Err(PlatformError::Busy(format!(
            "clipboard: {}",
            last.map(|e| e.message()).unwrap_or_default()
        )))
    }
}

impl Drop for Open {
    fn drop(&mut self) {
        // SAFETY: we opened it; the window is ours.
        unsafe {
            let _ = CloseClipboard();
            if let Some(h) = self.0 {
                let _ = DestroyWindow(h);
            }
        }
    }
}

/// Moves `bytes` into a movable global block owned by the clipboard after `SetClipboardData`.
fn put(fmt: u32, bytes: &[u8]) -> Result<()> {
    // SAFETY: a fresh block of the right size, locked while copied into; freed only if the
    // clipboard did not take it.
    unsafe {
        let h = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1)).map_err(os)?;
        let p = GlobalLock(h);
        if p.is_null() {
            let _ = GlobalFree(Some(h));
            return Err(PlatformError::Other("GlobalLock".into()));
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), p.cast::<u8>(), bytes.len());
        let _ = GlobalUnlock(h);
        if let Err(e) = SetClipboardData(fmt, Some(HANDLE(h.0))) {
            let _ = GlobalFree(Some(h));
            return Err(os(e));
        }
    }
    Ok(())
}

/// The bytes of a clipboard format, if present.
fn get(fmt: u32) -> Option<Vec<u8>> {
    // SAFETY: the handle belongs to the clipboard and stays valid while it is open; locked while
    // copied out.
    unsafe {
        IsClipboardFormatAvailable(fmt).ok()?;
        let h = GetClipboardData(fmt).ok()?;
        let g = HGLOBAL(h.0);
        let size = GlobalSize(g);
        let p = GlobalLock(g);
        if p.is_null() {
            return None;
        }
        let v = std::slice::from_raw_parts(p.cast::<u8>(), size).to_vec();
        let _ = GlobalUnlock(g);
        Some(v)
    }
}

fn read_files() -> Option<Vec<PathBuf>> {
    // SAFETY: as in `get`; DragQueryFileW reads the clipboard's HDROP.
    unsafe {
        IsClipboardFormatAvailable(CF_HDROP.0 as u32).ok()?;
        let h = GetClipboardData(CF_HDROP.0 as u32).ok()?;
        let drop = HDROP(h.0);
        let n = DragQueryFileW(drop, u32::MAX, None);
        let mut out = Vec::new();
        for i in 0..n {
            let len = DragQueryFileW(drop, i, None) as usize;
            let mut buf = vec![0u16; len + 1];
            let got = DragQueryFileW(drop, i, Some(&mut buf)) as usize;
            out.push(PathBuf::from(String::from_utf16_lossy(&buf[..got])));
        }
        (!out.is_empty()).then_some(out)
    }
}

impl Clipboard for WinClipboard {
    fn write(&self, items: &[ClipItem]) -> Result<()> {
        // Encode before taking the clipboard: it is held as briefly as possible.
        let mut blobs: Vec<(u32, Vec<u8>)> = Vec::new();
        for item in items {
            match item {
                ClipItem::Image(img) => {
                    check(img)?;
                    let png = match &img.png {
                        Some(p) => p.clone(),
                        None => encode_png(img.width, img.height, &img.rgba)?,
                    };
                    blobs.push((format(w!("PNG")), png));
                    blobs.push((
                        CF_DIB.0 as u32,
                        dib_from_rgba(img.width, img.height, &img.rgba),
                    ));
                }
                ClipItem::Files(files) => {
                    blobs.push((CF_HDROP.0 as u32, hdrop(files)));
                    // DROPEFFECT_COPY: pasting in Explorer copies the file.
                    blobs.push((
                        format(w!("Preferred DropEffect")),
                        1u32.to_le_bytes().to_vec(),
                    ));
                }
                ClipItem::Text(t) => {
                    let u: Vec<u16> = t.encode_utf16().chain([0]).collect();
                    let bytes = u.iter().flat_map(|c| c.to_le_bytes()).collect();
                    blobs.push((CF_UNICODETEXT.0 as u32, bytes));
                }
            }
        }
        let _open = Open::new(true)?;
        // SAFETY: the clipboard is open with our owner window.
        unsafe { EmptyClipboard() }.map_err(os)?;
        for (fmt, bytes) in &blobs {
            put(*fmt, bytes)?;
        }
        Ok(())
    }

    fn read(&self) -> Result<Vec<ClipItem>> {
        let _open = Open::new(false)?;
        let mut items = Vec::new();
        let image = get(format(w!("PNG")))
            .and_then(|png| {
                let (w, h, rgba) = decode_png(&png)?;
                Some(ClipImage {
                    width: w,
                    height: h,
                    rgba,
                    png: Some(png),
                })
            })
            .or_else(|| {
                [CF_DIBV5, CF_DIB].iter().find_map(|f| {
                    let (w, h, rgba) = rgba_from_dib(&get(f.0 as u32)?)?;
                    Some(ClipImage {
                        width: w,
                        height: h,
                        rgba,
                        png: None,
                    })
                })
            });
        if let Some(mut img) = image {
            fix_opacity(&mut img.rgba);
            items.push(ClipItem::Image(img));
        }
        if let Some(files) = read_files() {
            items.push(ClipItem::Files(files));
        }
        if let Some(bytes) = get(CF_UNICODETEXT.0 as u32) {
            let u: Vec<u16> = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_le_bytes(*c))
                .take_while(|&c| c != 0)
                .collect();
            items.push(ClipItem::Text(String::from_utf16_lossy(&u)));
        }
        Ok(items)
    }
}

fn check(img: &ClipImage) -> Result<()> {
    let n = img.width as usize * img.height as usize * 4;
    if img.width == 0 || img.height == 0 || img.rgba.len() != n {
        return Err(PlatformError::Other(format!(
            "image {}×{} with {} bytes",
            img.width,
            img.height,
            img.rgba.len()
        )));
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Formats (pure, tested without a clipboard)

fn encode_png(w: u32, h: u32, rgba: &[u8]) -> Result<Vec<u8>> {
    let mut out = std::io::Cursor::new(Vec::new());
    image::write_buffer_with_format(
        &mut out,
        rgba,
        w,
        h,
        image::ExtendedColorType::Rgba8,
        image::ImageFormat::Png,
    )
    .map_err(|e| PlatformError::Other(format!("PNG: {e}")))?;
    Ok(out.into_inner())
}

fn decode_png(png: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let img = image::load_from_memory_with_format(png, image::ImageFormat::Png).ok()?;
    let rgba = img.into_rgba8();
    Some((rgba.width(), rgba.height(), rgba.into_raw()))
}

/// `BITMAPINFOHEADER` + 24-bit bottom-up pixels, rows padded to 4 bytes; transparency on white.
fn dib_from_rgba(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    let stride = (w as usize * 3).div_ceil(4) * 4;
    let mut out = Vec::with_capacity(40 + stride * h as usize);
    for v in [40u32, w] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&(h as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // planes
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
    out.extend_from_slice(&((stride * h as usize) as u32).to_le_bytes());
    out.extend_from_slice(&[0u8; 16]); // resolution, colours used / important
    for y in (0..h as usize).rev() {
        let row = &rgba[y * w as usize * 4..(y + 1) * w as usize * 4];
        let start = out.len();
        for p in row.as_chunks::<4>().0 {
            let a = p[3] as u32;
            let on_white = |c: u8| ((c as u32 * a + 255 * (255 - a) + 127) / 255) as u8;
            out.extend_from_slice(&[on_white(p[2]), on_white(p[1]), on_white(p[0])]);
        }
        out.resize(start + stride, 0);
    }
    out
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// A packed DIB (`BITMAPINFOHEADER` / V4 / V5, 24 or 32 bits, `BI_RGB` or `BI_BITFIELDS`) to
/// straight RGBA. Other layouts (palettes, compression) → `None`; Windows offers PNG or a 24/32-bit
/// DIB for anything a screenshot tool gets.
fn rgba_from_dib(b: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let hsize = u32_at(b, 0)? as usize;
    if !(40..=124).contains(&hsize) {
        return None;
    }
    let w = u32_at(b, 4)? as i32;
    let h = u32_at(b, 8)? as i32;
    let bits = u16::from_le_bytes(b.get(14..16)?.try_into().ok()?);
    let compression = u32_at(b, 16)?;
    let colors_used = u32_at(b, 32)? as usize;
    if w <= 0 || h == 0 || w > 1 << 15 || h.unsigned_abs() > 1 << 15 {
        return None;
    }
    let (w, top_down, h) = (w as usize, h < 0, h.unsigned_abs() as usize);
    // Masks: inside a V4/V5 header, or right after a plain header with BI_BITFIELDS.
    let (masks, mut data) = match (compression, bits) {
        (0, 24) => (None, hsize),
        (0, 32) => (Some([0xff0000, 0xff00, 0xff, 0xff00_0000]), hsize),
        (3, 32) if hsize >= 56 => (
            Some([
                u32_at(b, 40)?,
                u32_at(b, 44)?,
                u32_at(b, 48)?,
                u32_at(b, 52)?,
            ]),
            hsize,
        ),
        (3, 32) => (
            Some([u32_at(b, 40)?, u32_at(b, 44)?, u32_at(b, 48)?, 0]),
            hsize + 12,
        ),
        _ => return None,
    };
    data += colors_used * 4;
    let bpp = bits as usize / 8;
    let stride = (w * bpp).div_ceil(4) * 4;
    if b.len() < data + stride * h {
        return None;
    }
    let chan = |px: u32, mask: u32| -> u8 {
        if mask == 0 {
            return 255;
        }
        let v = (px & mask) >> mask.trailing_zeros();
        let max = mask >> mask.trailing_zeros();
        (v * 255 / max.max(1)) as u8
    };
    let mut rgba = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        let src_y = if top_down { y } else { h - 1 - y };
        let row = &b[data + src_y * stride..][..w * bpp];
        for p in row.chunks_exact(bpp) {
            match masks {
                None => rgba.extend_from_slice(&[p[2], p[1], p[0], 255]),
                Some([r, g, bl, a]) => {
                    let px = u32::from_le_bytes([p[0], p[1], p[2], p[3]]);
                    rgba.extend_from_slice(&[chan(px, r), chan(px, g), chan(px, bl), chan(px, a)]);
                }
            }
        }
    }
    Some((w as u32, h as u32, rgba))
}

/// Alpha zero everywhere means «no alpha» (LH `CapFixOpacity`), not an invisible picture.
fn fix_opacity(rgba: &mut [u8]) {
    if rgba.as_chunks::<4>().0.iter().all(|p| p[3] == 0) {
        for p in rgba.as_chunks_mut::<4>().0 {
            p[3] = 255;
        }
    }
}

/// `DROPFILES` + wide paths, each NUL-terminated, then one more NUL.
fn hdrop(files: &[PathBuf]) -> Vec<u8> {
    let head = std::mem::size_of::<DROPFILES>();
    let mut out = vec![0u8; head];
    out[..4].copy_from_slice(&(head as u32).to_le_bytes()); // pFiles
    out[head - 4..].copy_from_slice(&1u32.to_le_bytes()); // fWide
    for f in files {
        for c in f.as_os_str().to_string_lossy().encode_utf16().chain([0]) {
            out.extend_from_slice(&c.to_le_bytes());
        }
    }
    out.extend_from_slice(&[0, 0]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(w: u32, h: u32, alpha: u8) -> Vec<u8> {
        (0..w * h)
            .flat_map(|i| [(i * 37) as u8, (i * 11 + 5) as u8, (255 - i) as u8, alpha])
            .collect()
    }

    #[test]
    fn dib_round_trip_24_bit_with_padding() {
        let (w, h) = (5, 3); // 15 bytes a row → padded to 16
        let rgba = picture(w, h, 255);
        let dib = dib_from_rgba(w, h, &rgba);
        assert_eq!(dib.len(), 40 + 16 * 3);
        assert_eq!(rgba_from_dib(&dib), Some((w, h, rgba)));
    }

    #[test]
    fn dib_flattens_transparency_on_white() {
        let dib = dib_from_rgba(1, 1, &[0, 0, 0, 0]);
        assert_eq!(&dib[40..43], &[255, 255, 255]);
        let dib = dib_from_rgba(1, 1, &[200, 100, 0, 128]);
        let (_, _, px) = rgba_from_dib(&dib).unwrap();
        assert_eq!(px, [227, 177, 127, 255]);
    }

    /// A top-down 32-bit V5 DIB with an alpha mask, as browsers put it.
    #[test]
    fn dibv5_top_down_with_alpha() {
        let (w, h) = (2u32, 2u32);
        let mut b = vec![0u8; 124];
        b[0..4].copy_from_slice(&124u32.to_le_bytes());
        b[4..8].copy_from_slice(&w.to_le_bytes());
        b[8..12].copy_from_slice(&(-(h as i32)).to_le_bytes());
        b[12..14].copy_from_slice(&1u16.to_le_bytes());
        b[14..16].copy_from_slice(&32u16.to_le_bytes());
        b[16..20].copy_from_slice(&3u32.to_le_bytes());
        for (i, m) in [0x00ff0000u32, 0xff00, 0xff, 0xff00_0000]
            .iter()
            .enumerate()
        {
            b[40 + i * 4..44 + i * 4].copy_from_slice(&m.to_le_bytes());
        }
        // BGRA rows, top first.
        b.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]);
        let (gw, gh, px) = rgba_from_dib(&b).unwrap();
        assert_eq!((gw, gh), (2, 2));
        assert_eq!(&px[..8], &[3, 2, 1, 4, 7, 6, 5, 8]);
        assert_eq!(&px[8..], &[11, 10, 9, 12, 15, 14, 13, 16]);
    }

    #[test]
    fn plain_32_bit_dib_with_zero_alpha_becomes_opaque() {
        let mut b = vec![0u8; 40];
        b[0..4].copy_from_slice(&40u32.to_le_bytes());
        b[4..8].copy_from_slice(&1u32.to_le_bytes());
        b[8..12].copy_from_slice(&1i32.to_le_bytes());
        b[14..16].copy_from_slice(&32u16.to_le_bytes());
        b.extend_from_slice(&[10, 20, 30, 0]);
        let (_, _, mut px) = rgba_from_dib(&b).unwrap();
        assert_eq!(px, [30, 20, 10, 0]);
        fix_opacity(&mut px);
        assert_eq!(px, [30, 20, 10, 255]);
        // A partly transparent picture keeps its alpha.
        let mut some = vec![1, 2, 3, 0, 4, 5, 6, 9];
        fix_opacity(&mut some);
        assert_eq!(some[3], 0);
    }

    #[test]
    fn broken_dibs_are_refused() {
        assert_eq!(rgba_from_dib(&[]), None);
        let mut b = dib_from_rgba(4, 4, &picture(4, 4, 255));
        b.truncate(60);
        assert_eq!(rgba_from_dib(&b), None);
        let mut pal = dib_from_rgba(1, 1, &[0, 0, 0, 255]);
        pal[14] = 8; // 8-bit palette: not supported
        assert_eq!(rgba_from_dib(&pal), None);
    }

    #[test]
    fn png_round_trip() {
        let rgba = picture(7, 5, 200);
        let png = encode_png(7, 5, &rgba).unwrap();
        assert_eq!(decode_png(&png), Some((7, 5, rgba)));
    }

    #[test]
    fn hdrop_layout() {
        let b = hdrop(&[PathBuf::from(r"C:\a.png"), PathBuf::from(r"D:\ї.png")]);
        assert_eq!(u32_at(&b, 0), Some(20));
        assert_eq!(u32_at(&b, 16), Some(1));
        let wide: Vec<u16> = b[20..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect();
        let text = String::from_utf16_lossy(&wide);
        assert_eq!(text, "C:\\a.png\0D:\\ї.png\0\0");
    }
}
