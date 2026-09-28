//! Screen capture for the prototype: the whole display under the pointer, as sRGB RGBA.
//! Windows — `znimok-win` (WGC, borderless; HDR tone-mapped by `Frame::to_srgb8`).
//! macOS — not wired yet (ZK-37 brings the ScreenCaptureKit implementation into the platform
//! layer); the button is disabled there with "Coming soon".

use znimok_core::Raster;

pub fn available() -> bool {
    cfg!(windows)
}

#[cfg(windows)]
pub fn display_under_cursor() -> Result<Raster, String> {
    use znimok_platform::{Capture, CaptureOptions, CaptureTarget, Cursor};
    let cap = znimok_win::WinCapture::new();
    let at = cap.position().map_err(|e| e.to_string())?;
    let display = match cap.display_at(at).map_err(|e| e.to_string())? {
        Some(d) => d,
        None => cap
            .displays()
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|d| d.primary)
            .ok_or("no displays")?,
    };
    let frame = cap
        .capture(
            &CaptureTarget::Display { id: display.id },
            &CaptureOptions {
                cursor: false,
                keep_hdr: false,
            },
        )
        .map_err(|e| e.to_string())?;
    let rgba = frame.to_srgb8();
    Ok(Raster::new(frame.width, frame.height, rgba))
}

#[cfg(not(windows))]
pub fn display_under_cursor() -> Result<Raster, String> {
    Err("screen capture is not available on this system yet".into())
}
