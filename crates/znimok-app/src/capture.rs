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

impl FrozenDisplay {
    /// A piece of this display, frame pixels → desktop units (ZK-295). The frame has as many
    /// pixels to a desktop unit as the display is dense: one on Windows, two on a Retina screen
    /// (its units are points) — and in a frame of several displays each has its own count. The
    /// piece is clipped to the display; the whole display comes out as exactly its bounds.
    ///
    /// Everything past the overlay counts in desktop units (the recording, its frame and bar, a
    /// window's bounds): this is the one place frame pixels become them.
    pub fn to_desktop(&self, r: PxRect) -> Rect {
        let b = self.bounds;
        let kx = f64::from(self.rect.w.max(1)) / f64::from(b.width.max(1));
        let ky = f64::from(self.rect.h.max(1)) / f64::from(b.height.max(1));
        let edge = |v: i32, origin: i32, k: f64, max: u32| {
            (f64::from(v - origin) / k)
                .round()
                .clamp(0.0, f64::from(max)) as i32
        };
        let (x0, y0) = (
            edge(r.x, self.rect.x, kx, b.width),
            edge(r.y, self.rect.y, ky, b.height),
        );
        let (x1, y1) = (
            edge(r.x + r.w, self.rect.x, kx, b.width),
            edge(r.y + r.h, self.rect.y, ky, b.height),
        );
        Rect {
            x: b.x + x0,
            y: b.y + y0,
            width: (x1 - x0).max(2) as u32,
            height: (y1 - y0).max(2) as u32,
        }
    }
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

    /// What a choice in the recording overlay records (ZK-180): its display and the piece of
    /// it, in desktop units — through [`FrozenDisplay::to_desktop`], never frame pixels as they
    /// are (ZK-295).
    pub fn video_choice(
        &self,
        rect: PxRect,
        source: &'static str,
        window: Option<u64>,
    ) -> crate::rec::Choice {
        let d = self.part_at(rect);
        crate::rec::Choice {
            display: d.bounds,
            frame: d.to_desktop(rect),
            window,
            source,
        }
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
/// What an agent asked for (ZK-242: on macOS only the app may capture, so the MCP server asks
/// it): a display, a window alone or a region, as sRGB RGBA. Runs on a worker thread.
#[cfg(windows)]
pub fn take(target: &CaptureTarget) -> Result<Raster, Fail> {
    let cap = znimok_win::WinCapture::new();
    take_with(&cap, target).or_else(|e| match target {
        // WGC refuses some remote and virtual displays: Desktop Duplication then.
        CaptureTarget::Window { .. } => Err(e),
        _ => take_with(
            &znimok_win::WinCapture::with_api(znimok_win::Api::Dxgi),
            target,
        ),
    })
}

#[cfg(target_os = "macos")]
pub fn take(target: &CaptureTarget) -> Result<Raster, Fail> {
    use znimok_platform::{Permission, PermissionState, Permissions};
    let cap = znimok_mac::MacCapture::new();
    match cap.capture(
        target,
        &CaptureOptions {
            cursor: false,
            keep_hdr: false,
        },
    ) {
        Ok(f) => Ok(to_raster(f)),
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
pub fn take(_target: &CaptureTarget) -> Result<Raster, Fail> {
    Err(Fail::Other(
        "screen capture is not available on this system".into(),
    ))
}

#[cfg(windows)]
fn take_with(cap: &impl Capture, target: &CaptureTarget) -> Result<Raster, Fail> {
    cap.capture(
        target,
        &CaptureOptions {
            cursor: false,
            keep_hdr: false,
        },
    )
    .map(to_raster)
    .map_err(|e| Fail::Other(e.to_string()))
}

/// The displays and the windows on screen (front to back, not Znimok's own, not minimised),
/// for an agent.
#[cfg(windows)]
pub fn targets() -> (
    Vec<znimok_platform::DisplayInfo>,
    Vec<znimok_platform::WindowInfo>,
) {
    let cap = znimok_win::WinCapture::new();
    (
        cap.displays().unwrap_or_default(),
        cap.windows()
            .unwrap_or_default()
            .into_iter()
            .filter(|w| !w.own && !w.minimized)
            .collect(),
    )
}

#[cfg(target_os = "macos")]
pub fn targets() -> (
    Vec<znimok_platform::DisplayInfo>,
    Vec<znimok_platform::WindowInfo>,
) {
    let cap = znimok_mac::MacCapture::new();
    (
        cap.displays().unwrap_or_default(),
        cap.windows()
            .unwrap_or_default()
            .into_iter()
            .filter(|w| !w.own && !w.minimized)
            .collect(),
    )
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn targets() -> (
    Vec<znimok_platform::DisplayInfo>,
    Vec<znimok_platform::WindowInfo>,
) {
    (Vec::new(), Vec::new())
}

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

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: i32, y: i32, width: u32, height: u32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    fn px(x: i32, y: i32, w: i32, h: i32) -> PxRect {
        PxRect { x, y, w, h }
    }

    /// ZK-295: a Retina display's frame has two pixels to the point. The whole display chosen in
    /// the overlay is the display's bounds (it was «a region of 3600 × 2338 points» of a display
    /// of 1800 × 1169 — recorded into a quarter of the video), a region is where it was dragged.
    #[test]
    fn frame_pixels_become_desktop_units() {
        // A MacBook Pro 14 in «More Space»: 1800 x 1169 points, 3600 x 2338 pixels.
        let retina = FrozenDisplay {
            bounds: rect(0, 0, 1800, 1169),
            rect: px(0, 0, 3600, 2338),
        };
        assert_eq!(retina.to_desktop(px(0, 0, 3600, 2338)), retina.bounds);
        assert_eq!(
            retina.to_desktop(px(200, 150, 1280, 720)),
            rect(100, 75, 640, 360)
        );
        // A piece reaching outside is clipped to the display.
        assert_eq!(
            retina.to_desktop(px(3000, 2000, 2000, 2000)),
            rect(1500, 1000, 300, 169)
        );
        // Windows: frame pixels are the desktop's, only the origin moves.
        let second = FrozenDisplay {
            bounds: rect(-1920, 120, 1920, 1080),
            rect: px(0, 0, 1920, 1080),
        };
        assert_eq!(second.to_desktop(second.rect), second.bounds);
        assert_eq!(
            second.to_desktop(px(200, 150, 640, 360)),
            rect(-1720, 270, 640, 360)
        );
        // Two displays in one frame on a common grid (the densest one's): a plain display
        // enlarged twice next to a Retina one — each with its own count.
        let plain = FrozenDisplay {
            bounds: rect(1800, 0, 1920, 1080),
            rect: px(3600, 0, 3840, 2160),
        };
        assert_eq!(plain.to_desktop(plain.rect), plain.bounds);
        assert_eq!(
            plain.to_desktop(px(3600 + 400, 200, 800, 600)),
            rect(1800 + 200, 100, 400, 300)
        );
        // A sliver stays something to record.
        assert_eq!(retina.to_desktop(px(10, 10, 1, 1)).width, 2);
    }

    /// The recording's choice from the overlay, whatever the gesture.
    #[test]
    fn the_overlay_s_choice_for_a_recording() {
        let frozen = Frozen {
            raster: Raster::solid(3600, 2338, znimok_core::Rgb::new(0, 0, 0)),
            bounds: rect(0, 0, 1800, 1169),
            windows: Vec::new(),
            displays: Vec::new(),
        };
        // A click on the desktop: the whole display, in its own units.
        let c = frozen.video_choice(frozen.whole(), "screen", None);
        assert_eq!((c.display, c.frame), (frozen.bounds, frozen.bounds));
        assert_eq!(c.source, "screen");
        // A region, and a window with its id.
        let c = frozen.video_choice(px(200, 150, 1280, 720), "region", None);
        assert_eq!(c.frame, rect(100, 75, 640, 360));
        let c = frozen.video_choice(px(440, 262, 2720, 1720), "window", Some(7));
        assert_eq!((c.frame, c.window), (rect(220, 131, 1360, 860), Some(7)));
    }
}
