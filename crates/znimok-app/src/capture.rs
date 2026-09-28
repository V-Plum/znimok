//! Screen capture for the prototype: the whole display under the pointer, as sRGB RGBA.
//! Windows — `znimok-win` (WGC, borderless; HDR tone-mapped by `Frame::to_srgb8`).
//! macOS — `znimok-mac` (ScreenCaptureKit; needs the Screen Recording permission).
//! Region selection comes with the overlay (ZK-39).

use znimok_core::Raster;
use znimok_platform::{
    Capture, CaptureOptions, CaptureTarget, Cursor, Frame, PlatformError, Rect, WindowList,
};

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

/// A rectangle in frame pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PxRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl PxRect {
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
}

/// The display under the pointer, frozen for the capture overlay (ZK-39).
pub struct Frozen {
    pub raster: Raster,
    /// The display in desktop units (pixels on Windows, points on macOS).
    pub bounds: Rect,
    /// Top-level windows over this display, front to back, clipped to it, in frame pixels.
    pub windows: Vec<(PxRect, String)>,
}

impl Frozen {
    /// A piece of the frozen frame (clamped to it).
    pub fn crop(&self, r: PxRect) -> Option<Raster> {
        let (fw, fh) = (self.raster.width as i32, self.raster.height as i32);
        let (x0, y0) = (r.x.clamp(0, fw), r.y.clamp(0, fh));
        let (x1, y1) = ((r.x + r.w).clamp(0, fw), (r.y + r.h).clamp(0, fh));
        if x1 - x0 < 1 || y1 - y0 < 1 {
            return None;
        }
        let mut rgba = Vec::with_capacity(((x1 - x0) * (y1 - y0) * 4) as usize);
        for y in y0..y1 {
            let o = ((y * fw + x0) * 4) as usize;
            rgba.extend_from_slice(&self.raster.rgba[o..o + ((x1 - x0) * 4) as usize]);
        }
        Some(Raster::new((x1 - x0) as u32, (y1 - y0) as u32, rgba))
    }

    pub fn whole(&self) -> PxRect {
        PxRect {
            x: 0,
            y: 0,
            w: self.raster.width as i32,
            h: self.raster.height as i32,
        }
    }
}

#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
fn freeze_with(cap: &(impl Capture + Cursor + WindowList)) -> Result<Frozen, PlatformError> {
    let at = cap.position()?;
    let display = match cap.display_at(at)? {
        Some(d) => d,
        None => cap
            .displays()?
            .into_iter()
            .find(|d| d.primary)
            .ok_or_else(|| PlatformError::NotFound("дисплей".into()))?,
    };
    let frame = cap.capture(
        &CaptureTarget::Display {
            id: display.id.clone(),
        },
        &CaptureOptions {
            cursor: false,
            keep_hdr: false,
        },
    )?;
    let bounds = display.bounds;
    let k = frame.width as f32 / bounds.width.max(1) as f32;
    let windows = cap
        .windows()
        .unwrap_or_default()
        .into_iter()
        .filter(|w| !w.minimized && !w.own && w.bounds.width > 0 && w.bounds.height > 0)
        .filter_map(|w| {
            let r = w.bounds.intersect(&bounds)?;
            let px = PxRect {
                x: ((r.x - bounds.x) as f32 * k).round() as i32,
                y: ((r.y - bounds.y) as f32 * k).round() as i32,
                w: (r.width as f32 * k).round() as i32,
                h: (r.height as f32 * k).round() as i32,
            };
            (px.w >= 8 && px.h >= 8).then_some((px, w.title))
        })
        .collect();
    Ok(Frozen {
        raster: to_raster(frame),
        bounds,
        windows,
    })
}

#[cfg(windows)]
pub fn freeze() -> Result<Frozen, Fail> {
    freeze_with(&znimok_win::WinCapture::new()).map_err(|e| Fail::Other(e.to_string()))
}

#[cfg(target_os = "macos")]
pub fn freeze() -> Result<Frozen, Fail> {
    use znimok_platform::{Permission, PermissionState, Permissions};
    let cap = znimok_mac::MacCapture::new();
    match freeze_with(&cap) {
        Ok(f) => Ok(f),
        Err(PlatformError::PermissionDenied(_)) => {
            if cap.request(Permission::ScreenRecording) != PermissionState::Granted {
                let _ = cap.open_settings(Permission::ScreenRecording);
            }
            Err(Fail::Permission)
        }
        Err(e) => Err(Fail::Other(e.to_string())),
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn freeze() -> Result<Frozen, Fail> {
    Err(Fail::Other(
        "screen capture is not available on this system".into(),
    ))
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
