use std::ffi::c_void;

use screencapturekit::prelude::*;
use screencapturekit::screenshot_manager::{CGImageExt, SCScreenshotManager};
use screencapturekit::shareable_content::SCShareableContentInfo;
use znimok_platform::{
    Capture, CaptureCaps, CaptureOptions, CaptureTarget, ColorInfo, Cursor, DisplayId, DisplayInfo,
    Frame, Permission, PermissionState, Permissions, PixelFormat, PlatformError, Point, Rect,
    Result, WindowId, WindowInfo, WindowList,
};

#[repr(C)]
#[derive(Clone, Copy)]
struct CGPoint {
    x: f64,
    y: f64,
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
    fn CGEventCreate(source: *const c_void) -> *mut c_void;
    fn CGEventGetLocation(event: *const c_void) -> CGPoint;
    fn CGWindowListCreate(option: u32, relative_to: u32) -> *const c_void;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(cf: *const c_void);
    fn CFArrayGetCount(array: *const c_void) -> isize;
    fn CFArrayGetValueAtIndex(array: *const c_void, idx: isize) -> *const c_void;
}

/// On-screen window ids, front to back (`kCGWindowListOptionOnScreenOnly`). ScreenCaptureKit
/// does not promise any order, and "click = the window under the pointer" needs the front one.
fn z_order() -> Vec<u32> {
    // SAFETY: plain CoreGraphics call; the array holds CGWindowIDs stored as pointer-sized
    // values (not objects), and is released below.
    unsafe {
        let arr = CGWindowListCreate(1, 0);
        if arr.is_null() {
            return Vec::new();
        }
        let n = CFArrayGetCount(arr);
        let ids = (0..n)
            .map(|i| CFArrayGetValueAtIndex(arr, i) as usize as u32)
            .collect();
        CFRelease(arr);
        ids
    }
}

fn has_screen_access() -> bool {
    // SAFETY: no arguments; answers from the TCC database without prompting.
    unsafe { CGPreflightScreenCaptureAccess() }
}

/// ScreenCaptureKit capture, pointer and permission — one value, like `WinCapture`.
#[derive(Default)]
pub struct MacCapture;

impl MacCapture {
    pub fn new() -> Self {
        Self
    }

    fn content(&self) -> Result<SCShareableContent> {
        if !has_screen_access() {
            return Err(PlatformError::PermissionDenied(Permission::ScreenRecording));
        }
        SCShareableContent::get()
            .map_err(|e| PlatformError::Other(format!("SCShareableContent: {e}")))
    }

    /// Filter for a whole display without this process's own windows (the editor, overlays).
    fn filter(content: &SCShareableContent, display: &SCDisplay) -> Result<SCContentFilter> {
        let me = std::process::id() as i32;
        let own: Vec<SCWindow> = content
            .windows()
            .into_iter()
            .filter(|w| {
                w.owning_application()
                    .map(|a| a.process_id() == me)
                    .unwrap_or(false)
            })
            .collect();
        let own_refs: Vec<&SCWindow> = own.iter().collect();
        SCContentFilter::create()
            .with_display(display)
            .with_excluding_windows(&own_refs)
            .build()
            .map_err(|e| PlatformError::Other(format!("SCContentFilter: {e}")))
    }

    fn info(content: &SCShareableContent, d: &SCDisplay) -> DisplayInfo {
        let f = d.frame();
        let bounds = Rect {
            x: f.origin.x.round() as i32,
            y: f.origin.y.round() as i32,
            width: f.size.width.round().max(1.0) as u32,
            height: f.size.height.round().max(1.0) as u32,
        };
        let ppu = Self::filter(content, d)
            .ok()
            .and_then(|flt| SCShareableContentInfo::for_filter(&flt))
            .map(|i| i.pixel_size().0 as f32 / bounds.width as f32)
            .filter(|s| *s > 0.0)
            .unwrap_or(1.0);
        DisplayInfo {
            id: DisplayId(d.display_id().to_string()),
            name: String::new(),
            bounds,
            work_area: bounds,
            scale_factor: ppu,
            pixels_per_unit: ppu,
            primary: bounds.x == 0 && bounds.y == 0,
            refresh_hz: None,
            color: ColorInfo::SDR,
        }
    }

    fn shoot(&self, content: &SCShareableContent, d: &SCDisplay) -> Result<Frame> {
        let info = Self::info(content, d);
        let filter = Self::filter(content, d)?;
        let (pw, ph) = SCShareableContentInfo::for_filter(&filter)
            .map(|i| i.pixel_size())
            .unwrap_or((
                (info.bounds.width as f32 * info.pixels_per_unit) as u32,
                (info.bounds.height as f32 * info.pixels_per_unit) as u32,
            ));
        let cfg = SCStreamConfiguration::new()
            .with_width(pw)
            .with_height(ph)
            .with_shows_cursor(false);
        let img = SCScreenshotManager::capture_image(&filter, &cfg).map_err(|e| {
            let msg = e.to_string();
            if msg.contains("TCC") || msg.contains("declined") {
                PlatformError::PermissionDenied(Permission::ScreenRecording)
            } else {
                PlatformError::Other(format!("SCScreenshotManager: {msg}"))
            }
        })?;
        let (w, h) = (img.width() as u32, img.height() as u32);
        let data = img
            .rgba_data()
            .map_err(|e| PlatformError::Other(format!("CGImage: {e}")))?;
        Frame {
            width: w,
            height: h,
            stride: w * 4,
            format: PixelFormat::Rgba8,
            color: ColorInfo::SDR,
            source: info.bounds,
            scale: w as f32 / info.bounds.width as f32,
            data,
        }
        .validate()
    }

    fn display_by_id<'a>(displays: &'a [SCDisplay], id: &DisplayId) -> Result<&'a SCDisplay> {
        displays
            .iter()
            .find(|d| d.display_id().to_string() == id.0)
            .ok_or_else(|| PlatformError::NotFound(format!("дисплей {}", id.0)))
    }
}

/// Cuts `rect` (desktop units) out of a whole-display frame.
fn crop(full: &Frame, rect: Rect) -> Result<Frame> {
    let s = full.scale;
    let x0 = (((rect.x - full.source.x) as f32) * s).floor().max(0.0) as u32;
    let y0 = (((rect.y - full.source.y) as f32) * s).floor().max(0.0) as u32;
    let x1 =
        ((((rect.x - full.source.x) as f32 + rect.width as f32) * s).ceil() as u32).min(full.width);
    let y1 = ((((rect.y - full.source.y) as f32 + rect.height as f32) * s).ceil() as u32)
        .min(full.height);
    if x1 <= x0 || y1 <= y0 {
        return Err(PlatformError::Other("порожня ділянка".into()));
    }
    let (w, h) = (x1 - x0, y1 - y0);
    let mut data = Vec::with_capacity((w * h * 4) as usize);
    for y in y0..y1 {
        let o = (y * full.stride + x0 * 4) as usize;
        data.extend_from_slice(&full.data[o..o + (w * 4) as usize]);
    }
    Frame {
        width: w,
        height: h,
        stride: w * 4,
        format: full.format,
        color: full.color.clone(),
        source: rect,
        scale: s,
        data,
    }
    .validate()
}

impl Capture for MacCapture {
    fn caps(&self) -> CaptureCaps {
        CaptureCaps {
            borderless: true,
            hdr: false,
            window_capture: false,
            needs_permission: Some(Permission::ScreenRecording),
            system_picker: false,
        }
    }

    fn displays(&self) -> Result<Vec<DisplayInfo>> {
        let content = self.content()?;
        Ok(content
            .displays()
            .iter()
            .map(|d| Self::info(&content, d))
            .collect())
    }

    fn capture(&self, target: &CaptureTarget, _opts: &CaptureOptions) -> Result<Frame> {
        let content = self.content()?;
        let displays = content.displays();
        match target {
            CaptureTarget::Display { id } => {
                let d = Self::display_by_id(&displays, id)?;
                self.shoot(&content, d)
            }
            CaptureTarget::Region { rect } => {
                // Never stitch displays (they may differ in scale and colour space).
                let d = displays
                    .iter()
                    .find(|d| Self::info(&content, d).bounds.intersect(rect) == Some(*rect))
                    .ok_or(PlatformError::Unsupported(
                        "ділянка на кількох дисплеях або поза ними",
                    ))?;
                let full = self.shoot(&content, d)?;
                crop(&full, *rect)
            }
            CaptureTarget::Window { .. } => Err(PlatformError::Unsupported(
                "знімок вікна на macOS (ZK-37, далі)",
            )),
            CaptureTarget::Picked { .. } => {
                Err(PlatformError::Unsupported("системний пікер (ZK-37, далі)"))
            }
        }
    }
}

impl Cursor for MacCapture {
    fn position(&self) -> Result<Point> {
        // SAFETY: a null source gives an event carrying the current pointer location in global
        // CoreGraphics coordinates (top-left origin); the event is released right after.
        unsafe {
            let ev = CGEventCreate(std::ptr::null());
            if ev.is_null() {
                return Err(PlatformError::Other("CGEventCreate".into()));
            }
            let p = CGEventGetLocation(ev);
            CFRelease(ev);
            Ok(Point {
                x: p.x.floor() as i32,
                y: p.y.floor() as i32,
            })
        }
    }
}

impl Permissions for MacCapture {
    fn status(&self, p: Permission) -> PermissionState {
        match p {
            Permission::ScreenRecording if has_screen_access() => PermissionState::Granted,
            Permission::ScreenRecording => PermissionState::NotDetermined,
            _ => PermissionState::NotDetermined,
        }
    }

    fn request(&self, p: Permission) -> PermissionState {
        match p {
            // SAFETY: no arguments; shows the system prompt the first time only.
            Permission::ScreenRecording if unsafe { CGRequestScreenCaptureAccess() } => {
                PermissionState::Granted
            }
            _ => self.status(p),
        }
    }

    fn open_settings(&self, p: Permission) -> Result<()> {
        let pane = match p {
            Permission::ScreenRecording => "Privacy_ScreenCapture",
            Permission::Microphone => "Privacy_Microphone",
            Permission::InputMonitoring => "Privacy_ListenEvent",
            Permission::Accessibility => "Privacy_Accessibility",
        };
        std::process::Command::new("open")
            .arg(format!(
                "x-apple.systempreferences:com.apple.preference.security?{pane}"
            ))
            .status()
            .map_err(|e| PlatformError::Other(e.to_string()))
            .map(|_| ())
    }
}

impl WindowList for MacCapture {
    /// Normal-level windows on screen, front to back, in desktop units (points).
    fn windows(&self) -> Result<Vec<WindowInfo>> {
        let content = self.content()?;
        let order = z_order();
        let me = std::process::id() as i32;
        let mut list: Vec<(usize, WindowInfo)> = content
            .windows()
            .into_iter()
            .filter(|w| w.is_on_screen() && w.window_layer() == 0)
            .map(|w| {
                let f = w.frame();
                let app = w.owning_application();
                let pid = app.as_ref().map(|a| a.process_id()).unwrap_or(0);
                let rank = order
                    .iter()
                    .position(|id| *id == w.window_id())
                    .unwrap_or(usize::MAX);
                let info = WindowInfo {
                    id: WindowId(w.window_id() as u64),
                    title: w.title().unwrap_or_default(),
                    app: app
                        .as_ref()
                        .map(|a| a.application_name())
                        .unwrap_or_default(),
                    pid: pid.max(0) as u32,
                    bounds: Rect {
                        x: f.origin.x.round() as i32,
                        y: f.origin.y.round() as i32,
                        width: f.size.width.round().max(0.0) as u32,
                        height: f.size.height.round().max(0.0) as u32,
                    },
                    display: None,
                    scale_factor: 1.0,
                    minimized: false,
                    own: pid == me,
                };
                (rank, info)
            })
            .collect();
        list.sort_by_key(|(rank, _)| *rank);
        Ok(list.into_iter().map(|(_, w)| w).collect())
    }
}
