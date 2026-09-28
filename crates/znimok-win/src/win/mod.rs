mod display;
mod dxgi;
mod wgc;
mod winlist;

use std::sync::Once;

use windows::Win32::Foundation::POINT;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BOX, D3D11_CPU_ACCESS_READ, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
use znimok_platform::{
    Capture, CaptureCaps, CaptureOptions, CaptureTarget, ColorInfo, Cursor, DisplayInfo, Frame,
    Permission, PermissionState, Permissions, PixelFormat, PlatformError, Point, Rect, Result,
    Transfer, WindowInfo, WindowList,
};

/// Make the process Per-Monitor-v2 DPI aware. Call at start-up, before any window exists (later
/// calls cannot change the DPI mode); [`WinCapture::new`] calls it too.
pub fn init_process() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // SAFETY: process-wide setting; failure (already set) is harmless.
        unsafe {
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }
    });
}

/// Join the multithreaded apartment on the calling thread, once per thread, and stay in it.
/// Every entry point that touches WinRT/COM calls this: relying on another thread's MTA crashed
/// (ACCESS_VIOLATION) when that thread ended while this one still held WinRT objects.
pub(crate) fn com_thread() {
    thread_local! {
        static JOINED: () = {
            // SAFETY: plain apartment init; S_FALSE (already in MTA) and RPC_E_CHANGED_MODE (an STA
            // thread, e.g. a UI thread — WinRT capture works there too) are both fine to ignore.
            let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
        };
    }
    JOINED.with(|_| {});
}

pub(crate) fn e2p(e: windows_core::Error) -> PlatformError {
    PlatformError::Os {
        code: i64::from(e.code().0 as u32),
        message: e.message(),
    }
}

/// Pixels read back from a texture: tight rows (stride = width × bpp).
pub(crate) struct Raw {
    width: u32,
    height: u32,
    format: PixelFormat,
    data: Vec<u8>,
}

/// Copy the top-left `w`×`h` (clamped to the texture) through a staging texture, row by row within
/// RowPitch (≠ width × bpp — the LH trap that sheared frames in CAPS-16).
pub(crate) fn read_texture(
    dev: &ID3D11Device,
    ctx: &ID3D11DeviceContext,
    tex: &ID3D11Texture2D,
    w: u32,
    h: u32,
    format: PixelFormat,
) -> Result<Raw> {
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    // SAFETY: valid texture; staging copy, map, reads within RowPitch × h.
    unsafe {
        tex.GetDesc(&mut desc);
        let (w, h) = (w.min(desc.Width), h.min(desc.Height));
        let sdesc = D3D11_TEXTURE2D_DESC {
            Width: w,
            Height: h,
            MipLevels: 1,
            ArraySize: 1,
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
            ..desc
        };
        let mut staging = None;
        dev.CreateTexture2D(&sdesc, None, Some(&mut staging))
            .map_err(e2p)?;
        let staging = staging.ok_or(PlatformError::Other("staging: None".into()))?;
        let bx = D3D11_BOX {
            left: 0,
            top: 0,
            front: 0,
            right: w,
            bottom: h,
            back: 1,
        };
        ctx.CopySubresourceRegion(&staging, 0, 0, 0, 0, tex, 0, Some(&bx));
        let mut m = D3D11_MAPPED_SUBRESOURCE::default();
        ctx.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut m))
            .map_err(e2p)?;
        let row = (w * format.bytes_per_pixel()) as usize;
        let mut data = Vec::with_capacity(row * h as usize);
        for y in 0..h as usize {
            let p = (m.pData as *const u8).add(y * m.RowPitch as usize);
            data.extend_from_slice(std::slice::from_raw_parts(p, row));
        }
        ctx.Unmap(&staging, 0);
        Ok(Raw {
            width: w,
            height: h,
            format,
            data,
        })
    }
}

/// Which Windows API takes display shots.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Api {
    /// Windows.Graphics.Capture (Borderless); falls back to DXGI for a display if WGC fails.
    Wgc,
    /// DXGI Desktop Duplication for displays and regions; windows still go through WGC.
    Dxgi,
}

/// `Capture` + `WindowList` + `Cursor` + `Permissions` on Windows.
pub struct WinCapture {
    api: Api,
}

impl Default for WinCapture {
    fn default() -> Self {
        Self::new()
    }
}

impl WinCapture {
    pub fn new() -> Self {
        Self::with_api(Api::Wgc)
    }

    pub fn with_api(api: Api) -> Self {
        init_process();
        Self { api }
    }

    fn monitor(&self, pred: impl Fn(&DisplayInfo) -> bool) -> Result<display::Monitor> {
        display::list()
            .into_iter()
            .find(|m| pred(&m.info))
            .ok_or(PlatformError::NotFound("дисплей".into()))
    }

    /// Whole display as a frame.
    fn shoot_display(&self, m: &display::Monitor) -> Result<Frame> {
        let d = &m.info;
        let use_dxgi = self.api == Api::Dxgi || !wgc::supported();
        let (raw, transfer) = if use_dxgi {
            dxgi::capture(m.handle)?
        } else {
            match wgc::capture(wgc::Item::Display(m.handle), d.color.hdr) {
                Ok(raw) => {
                    let t = if raw.format == PixelFormat::Rgba16Float {
                        Transfer::ScRgb
                    } else {
                        Transfer::Srgb
                    };
                    (raw, t)
                }
                // WGC unavailable in this session (e.g. service desktop): try duplication.
                Err(e) => dxgi::capture(m.handle).map_err(|_| e)?,
            }
        };
        let color = color_for(d, transfer);
        frame(raw, color, d.bounds)
    }
}

/// Colour of a frame from display `d` delivered with `transfer`: HDR formats carry the display's
/// SDR white (needed for tone mapping); 8-bit frames are plain sRGB.
fn color_for(d: &DisplayInfo, transfer: Transfer) -> ColorInfo {
    match transfer {
        Transfer::Srgb => ColorInfo {
            sdr_white_nits: d.color.sdr_white_nits,
            ..ColorInfo::SDR
        },
        t => ColorInfo {
            transfer: t,
            sdr_white_nits: d.color.sdr_white_nits,
            hdr: d.color.hdr,
        },
    }
}

fn frame(raw: Raw, color: ColorInfo, source: Rect) -> Result<Frame> {
    let stride = raw.width * raw.format.bytes_per_pixel();
    Frame {
        width: raw.width,
        height: raw.height,
        stride,
        format: raw.format,
        color,
        source,
        scale: 1.0,
        data: raw.data,
    }
    .validate()
}

/// Cut `r` (pixels, relative to the frame) out of a frame.
fn crop(f: &Frame, r: Rect, source: Rect) -> Result<Frame> {
    let bpp = f.format.bytes_per_pixel() as usize;
    if r.x < 0 || r.y < 0 || r.right() as u32 > f.width || r.bottom() as u32 > f.height {
        return Err(PlatformError::Other(format!(
            "ділянка {r:?} поза кадром {}×{}",
            f.width, f.height
        )));
    }
    let mut data = Vec::with_capacity(r.width as usize * r.height as usize * bpp);
    for y in r.y as u32..r.bottom() as u32 {
        let row = f.row(y);
        data.extend_from_slice(&row[r.x as usize * bpp..r.right() as usize * bpp]);
    }
    Frame {
        width: r.width,
        height: r.height,
        stride: r.width * bpp as u32,
        format: f.format,
        color: f.color.clone(),
        source,
        scale: f.scale,
        data,
    }
    .validate()
}

impl Capture for WinCapture {
    fn caps(&self) -> CaptureCaps {
        CaptureCaps {
            borderless: wgc::supported() && wgc::borderless(),
            hdr: true,
            window_capture: wgc::supported(),
            needs_permission: None,
            system_picker: false,
        }
    }

    fn displays(&self) -> Result<Vec<DisplayInfo>> {
        Ok(display::list().into_iter().map(|m| m.info).collect())
    }

    /// `keep_hdr = false` does not tone map yet (ZK-38): an HDR display still gives FP16; SDR
    /// displays always give BGRA8.
    fn capture(&self, target: &CaptureTarget, _opts: &CaptureOptions) -> Result<Frame> {
        match target {
            CaptureTarget::Display { id } => {
                let m = self.monitor(|d| &d.id == id)?;
                self.shoot_display(&m)
            }
            CaptureTarget::Region { rect } => {
                // Never stitch displays (they may differ in colour space and white level).
                let m = self
                    .monitor(|d| d.bounds.intersect(rect) == Some(*rect))
                    .map_err(|_| {
                        PlatformError::Unsupported("ділянка на кількох дисплеях або поза ними")
                    })?;
                let full = self.shoot_display(&m)?;
                crop(&full, rect.relative_to(m.info.bounds.origin()), *rect)
            }
            CaptureTarget::Window { id } => {
                let h = winlist::hwnd(*id);
                if !winlist::is_alive(h) {
                    return Err(PlatformError::NotFound(format!("вікно 0x{:X}", id.0)));
                }
                if winlist::is_minimized(h) {
                    return Err(PlatformError::NotFound(format!(
                        "вікно 0x{:X} згорнуте",
                        id.0
                    )));
                }
                let bounds = winlist::dwm_bounds(h).ok_or(PlatformError::NotFound(format!(
                    "вікно 0x{:X} без меж",
                    id.0
                )))?;
                let disp = winlist::display_of(h);
                let d = display::list()
                    .into_iter()
                    .find(|m| Some(&m.info.id) == disp.as_ref())
                    .map(|m| m.info);
                let hdr = d.as_ref().is_some_and(|d| d.color.hdr);
                let raw = wgc::capture(wgc::Item::Window(h), hdr)?;
                let t = if raw.format == PixelFormat::Rgba16Float {
                    Transfer::ScRgb
                } else {
                    Transfer::Srgb
                };
                let color = d.as_ref().map_or(ColorInfo::SDR, |d| color_for(d, t));
                frame(raw, color, bounds)
            }
            CaptureTarget::Picked { .. } => {
                Err(PlatformError::Unsupported("системний пікер (лише macOS)"))
            }
        }
    }
}

impl WindowList for WinCapture {
    fn windows(&self) -> Result<Vec<WindowInfo>> {
        winlist::list().map_err(PlatformError::Other)
    }
}

impl Cursor for WinCapture {
    fn position(&self) -> Result<Point> {
        let mut p = POINT::default();
        // SAFETY: valid out-pointer.
        unsafe { GetCursorPos(&mut p) }.map_err(e2p)?;
        Ok(Point { x: p.x, y: p.y })
    }
}

/// Windows has no permission gate for screen capture, and microphone access is a system setting
/// the app cannot query without packaging — everything is `NotNeeded`.
impl Permissions for WinCapture {
    fn status(&self, _p: Permission) -> PermissionState {
        PermissionState::NotNeeded
    }
    fn request(&self, _p: Permission) -> PermissionState {
        PermissionState::NotNeeded
    }
    fn open_settings(&self, _p: Permission) -> Result<()> {
        Err(PlatformError::Unsupported("дозволи на Windows не потрібні"))
    }
}
