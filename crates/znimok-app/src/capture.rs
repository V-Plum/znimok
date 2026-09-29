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

#[derive(Clone, Debug)]
pub struct FrozenWindow {
    pub rect: PxRect,
    pub title: String,
    /// Platform window id, for a capture of the window alone (without what overlaps it).
    pub id: u64,
}

/// The screen frozen for the capture overlay (ZK-39): one display, or all of them side by side
/// in one frame (ZK-139), so a region can cross from one screen to the next.
pub struct Frozen {
    pub raster: Raster,
    /// What the frame covers, in desktop units (pixels on Windows, points on macOS).
    pub bounds: Rect,
    /// Top-level windows over it, front to back, clipped to it, in frame pixels.
    pub windows: Vec<FrozenWindow>,
    /// The displays in the frame; empty = one display, the whole frame.
    pub displays: Vec<FrozenDisplay>,
}

/// One display inside a frame of several.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrozenDisplay {
    /// In desktop units.
    pub bounds: Rect,
    /// Where it is in the frame, in frame pixels.
    pub rect: PxRect,
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

    /// The displays in the frame (one, the whole frame, when it holds a single display).
    pub fn parts(&self) -> Vec<FrozenDisplay> {
        if self.displays.is_empty() {
            vec![FrozenDisplay {
                bounds: self.bounds,
                rect: self.whole(),
            }]
        } else {
            self.displays.clone()
        }
    }

    /// The display with the middle of `r` (the first one if none).
    pub fn part_at(&self, r: PxRect) -> FrozenDisplay {
        let parts = self.parts();
        let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
        parts
            .iter()
            .find(|d| d.rect.contains(cx, cy))
            .copied()
            .unwrap_or(parts[0])
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
fn freeze_with(
    cap: &(impl Capture + Cursor + WindowList),
    same: Option<Rect>,
) -> Result<Frozen, PlatformError> {
    let at = cap.position()?;
    let all = cap.displays()?;
    // After a countdown: the display that was chosen, wherever the pointer went since.
    let chosen = match same {
        Some(b) => all.iter().find(|d| d.bounds == b).cloned(),
        None => None,
    };
    // Several displays: all of them in one frame (ZK-139), unless one was asked for.
    if chosen.is_none() && all.len() > 1 {
        return freeze_all(cap, all);
    }
    let display = match chosen {
        Some(d) => d,
        None => match cap.display_at(at)? {
            Some(d) => d,
            None => cap
                .displays()?
                .into_iter()
                .find(|d| d.primary)
                .ok_or_else(|| PlatformError::NotFound("дисплей".into()))?,
        },
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
            (px.w >= 8 && px.h >= 8).then_some(FrozenWindow {
                rect: px,
                title: w.title,
                id: w.id.0,
            })
        })
        .collect();
    Ok(Frozen {
        raster: to_raster(frame),
        bounds,
        windows,
        displays: Vec::new(),
    })
}

/// All displays in one frame, in a common pixel grid: as many frame pixels per desktop unit as
/// the densest display has (a Retina screen keeps its pixels; a plainer one is enlarged,
/// nearest neighbour). Gaps between displays of different sizes stay black.
#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
fn freeze_all(
    cap: &(impl Capture + Cursor + WindowList),
    all: Vec<znimok_platform::DisplayInfo>,
) -> Result<Frozen, PlatformError> {
    let mut frames = Vec::new();
    for d in &all {
        let f = cap.capture(
            &CaptureTarget::Display { id: d.id.clone() },
            &CaptureOptions {
                cursor: false,
                keep_hdr: false,
            },
        )?;
        frames.push((d.bounds, to_raster(f)));
    }
    let (raster, union, displays) = compose(&frames);
    let s = raster.width as f32 / union.width.max(1) as f32;
    let windows = cap
        .windows()
        .unwrap_or_default()
        .into_iter()
        .filter(|w| !w.minimized && !w.own && w.bounds.width > 0 && w.bounds.height > 0)
        .filter_map(|w| {
            let r = w.bounds.intersect(&union)?;
            let px = PxRect {
                x: ((r.x - union.x) as f32 * s).round() as i32,
                y: ((r.y - union.y) as f32 * s).round() as i32,
                w: (r.width as f32 * s).round() as i32,
                h: (r.height as f32 * s).round() as i32,
            };
            (px.w >= 8 && px.h >= 8).then_some(FrozenWindow {
                rect: px,
                title: w.title,
                id: w.id.0,
            })
        })
        .collect();
    Ok(Frozen {
        raster,
        bounds: union,
        windows,
        displays,
    })
}

/// Frames of displays (desktop bounds, pixels) → one frame over their union.
pub fn compose(frames: &[(Rect, Raster)]) -> (Raster, Rect, Vec<FrozenDisplay>) {
    let s = frames
        .iter()
        .map(|(b, r)| r.width as f32 / b.width.max(1) as f32)
        .fold(1.0f32, f32::max);
    let x0 = frames.iter().map(|(b, _)| b.x).min().unwrap_or(0);
    let y0 = frames.iter().map(|(b, _)| b.y).min().unwrap_or(0);
    let x1 = frames
        .iter()
        .map(|(b, _)| b.x + b.width as i32)
        .max()
        .unwrap_or(1);
    let y1 = frames
        .iter()
        .map(|(b, _)| b.y + b.height as i32)
        .max()
        .unwrap_or(1);
    let union = Rect::new(x0, y0, (x1 - x0) as u32, (y1 - y0) as u32);
    let (cw, ch) = (
        (union.width as f32 * s).round() as u32,
        (union.height as f32 * s).round() as u32,
    );
    let mut out = vec![0u8; cw as usize * ch as usize * 4];
    for p in out.chunks_mut(4) {
        p[3] = 255;
    }
    let mut displays = Vec::new();
    for (b, r) in frames {
        let rect = PxRect {
            x: ((b.x - x0) as f32 * s).round() as i32,
            y: ((b.y - y0) as f32 * s).round() as i32,
            w: (b.width as f32 * s).round() as i32,
            h: (b.height as f32 * s).round() as i32,
        };
        for ty in 0..rect.h.max(0) {
            let dy = rect.y + ty;
            if dy < 0 || dy >= ch as i32 {
                continue;
            }
            let sy = ((ty as i64 * r.height as i64) / rect.h.max(1) as i64) as usize;
            for tx in 0..rect.w.max(0) {
                let dx = rect.x + tx;
                if dx < 0 || dx >= cw as i32 {
                    continue;
                }
                let sx = ((tx as i64 * r.width as i64) / rect.w.max(1) as i64) as usize;
                let si = (sy * r.width as usize + sx) * 4;
                let di = (dy as usize * cw as usize + dx as usize) * 4;
                out[di..di + 4].copy_from_slice(&r.rgba[si..si + 4]);
            }
        }
        displays.push(FrozenDisplay { bounds: *b, rect });
    }
    (Raster::new(cw, ch, out), union, displays)
}

pub fn freeze() -> Result<Frozen, Fail> {
    freeze_display(None)
}

/// A fresh frame of the display with these bounds (the scrolling capture, ZK-141).
pub fn display_frame(same: Rect) -> Result<Raster, Fail> {
    freeze_display(Some(same)).map(|f| f.raster)
}

/// A fresh frame of the display with these bounds (`None`: the one under the pointer).
#[cfg(windows)]
pub fn freeze_display(same: Option<Rect>) -> Result<Frozen, Fail> {
    freeze_with(&znimok_win::WinCapture::new(), same).map_err(|e| Fail::Other(e.to_string()))
}

#[cfg(target_os = "macos")]
pub fn freeze_display(same: Option<Rect>) -> Result<Frozen, Fail> {
    use znimok_platform::{Permission, PermissionState, Permissions};
    let cap = znimok_mac::MacCapture::new();
    match freeze_with(&cap, same) {
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

/// The window alone, unoccluded (WGC window capture / ScreenCaptureKit window filter).
/// Runs on a worker thread.
#[cfg(windows)]
pub fn capture_window(id: u64) -> Result<Raster, Fail> {
    window_with(&znimok_win::WinCapture::new(), id)
}

#[cfg(target_os = "macos")]
pub fn capture_window(id: u64) -> Result<Raster, Fail> {
    window_with(&znimok_mac::MacCapture::new(), id)
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn capture_window(_id: u64) -> Result<Raster, Fail> {
    Err(Fail::Other(
        "window capture is not available on this system".into(),
    ))
}

#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
fn window_with(cap: &impl Capture, id: u64) -> Result<Raster, Fail> {
    cap.capture(
        &CaptureTarget::Window {
            id: znimok_platform::WindowId(id),
        },
        &CaptureOptions {
            cursor: false,
            keep_hdr: false,
        },
    )
    .map(to_raster)
    .map_err(|e| Fail::Other(e.to_string()))
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn freeze_display(_same: Option<Rect>) -> Result<Frozen, Fail> {
    Err(Fail::Other(
        "screen capture is not available on this system".into(),
    ))
}

#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
fn to_raster(frame: Frame) -> Raster {
    // HDR frames are tone mapped on the GPU (ZK-38; ~20× faster at 4K), else on the CPU.
    let rgba = znimok_gpu::to_srgb8(&frame);
    Raster::new(frame.width, frame.height, rgba)
}

/// ZK-38: with an HDR display the tone-mapping device is made ahead of the first capture
/// (≈0.5 s once), on a thread of its own. macOS gives SDR frames, so there is nothing to do.
pub fn warm_up_tone() {
    #[cfg(windows)]
    std::thread::spawn(|| {
        use znimok_platform::Capture;
        let hdr = znimok_win::WinCapture::new()
            .displays()
            .is_ok_and(|d| d.iter().any(|d| d.color.hdr));
        if hdr {
            let _ = znimok_gpu::ToneMapper::shared();
        }
    });
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
