//! Screen capture for the prototype: the whole display under the pointer, as sRGB RGBA.
//! Windows — `znimok-win` (WGC, borderless; HDR tone-mapped by `Frame::to_srgb8`).
//! macOS — `znimok-mac` (ScreenCaptureKit; needs the Screen Recording permission).
//! Region selection comes with the overlay (ZK-39).

use znimok_core::Raster;
use znimok_platform::{Capture, CaptureOptions, CaptureTarget, Cursor, Frame, PlatformError};

pub enum Fail {
    /// macOS Screen Recording is not granted; the system prompt or the settings page was shown.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Permission,
    Other(String),
}

pub fn available() -> bool {
    cfg!(any(windows, target_os = "macos"))
}

#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
fn shoot(cap: &(impl Capture + Cursor)) -> Result<Frame, PlatformError> {
    let at = cap.position()?;
    let display = match cap.display_at(at)? {
        Some(d) => d,
        None => cap
            .displays()?
            .into_iter()
            .find(|d| d.primary)
            .ok_or_else(|| PlatformError::NotFound("дисплей".into()))?,
    };
    cap.capture(
        &CaptureTarget::Display { id: display.id },
        &CaptureOptions {
            cursor: false,
            keep_hdr: false,
        },
    )
}

#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
fn to_raster(frame: Frame) -> Raster {
    let rgba = frame.to_srgb8();
    Raster::new(frame.width, frame.height, rgba)
}

/// Runs on a worker thread (WinRT wants the MTA; ScreenCaptureKit calls back on the main queue).
#[cfg(windows)]
pub fn display_under_cursor() -> Result<Raster, Fail> {
    let cap = znimok_win::WinCapture::new();
    shoot(&cap)
        .map(to_raster)
        .map_err(|e| Fail::Other(e.to_string()))
}

#[cfg(target_os = "macos")]
pub fn display_under_cursor() -> Result<Raster, Fail> {
    use znimok_platform::{Permission, PermissionState, Permissions};
    let cap = znimok_mac::MacCapture::new();
    match shoot(&cap) {
        Ok(f) => Ok(to_raster(f)),
        Err(PlatformError::PermissionDenied(_)) => {
            // First time: the system prompt. After a refusal macOS never asks again, so open
            // the settings page. A new grant takes effect after the app restarts.
            if cap.request(Permission::ScreenRecording) != PermissionState::Granted {
                let _ = cap.open_settings(Permission::ScreenRecording);
            }
            Err(Fail::Permission)
        }
        Err(e) => Err(Fail::Other(e.to_string())),
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn display_under_cursor() -> Result<Raster, Fail> {
    Err(Fail::Other(
        "screen capture is not available on this system".into(),
    ))
}
