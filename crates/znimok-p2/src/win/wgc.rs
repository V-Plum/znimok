//! Windows.Graphics.Capture: a display or a window, one frame, FP16.
//!
//! - Yellow border: `GraphicsCaptureAccess::RequestAccessAsync(Borderless)` first — for an unpackaged
//!   Win32 process on Windows 11 26100+ it answers Allowed without a prompt; only then does
//!   `IsBorderRequired = false` take effect (otherwise it is silently ignored).
//! - Cursor: `IsCursorCaptureEnabled = false` — the product draws its own.
//! - Format: the pool is always R16G16B16A16Float. On an HDR display that is scRGB with values above
//!   1.0; on SDR it is the same content linearised (1.0 = SDR white), so one path serves both
//!   (BGRA8 on HDR gives a washed-out picture).
//! - The frame texture can be larger than the content (pool size vs item size): copy ContentSize only.

use std::time::{Duration, Instant};

use serde::Serialize;
use windows::Graphics::Capture::{
    Direct3D11CaptureFramePool, GraphicsCaptureAccess, GraphicsCaptureAccessKind,
    GraphicsCaptureItem, GraphicsCaptureSession,
};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Foundation::{HMODULE, HWND};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BOX, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ,
    D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Graphics::Gdi::HMONITOR;
use windows::Win32::System::WinRT::Direct3D11::{
    CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows_core::Interface;

use crate::tone::{Frame, Pixels, Transfer};

pub enum Target {
    Display(HMONITOR),
    Window(HWND),
}

#[derive(Debug, Serialize)]
pub struct WgcMeta {
    pub api: &'static str,
    /// Result of RequestAccessAsync(Borderless): "Allowed" is what removes the border.
    pub borderless_access: String,
    pub border_required_set: bool,
    pub cursor_disabled: bool,
    pub item_size: [i32; 2],
    pub content_size: [i32; 2],
    pub first_frame_ms: f64,
}

fn e2s(e: windows_core::Error) -> String {
    format!("{} (0x{:08X})", e.message(), e.code().0)
}

pub fn d3d11() -> Result<(ID3D11Device, ID3D11DeviceContext), String> {
    let (mut dev, mut ctx) = (None, None);
    // SAFETY: standard device creation with out-pointers to locals.
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut dev),
            None,
            Some(&mut ctx),
        )
    }
    .map_err(e2s)?;
    Ok((
        dev.ok_or("D3D11: немає пристрою")?,
        ctx.ok_or("D3D11: немає контексту")?,
    ))
}

/// RequestAccessAsync(Borderless) — "Allowed" means `IsBorderRequired = false` will be honoured.
pub fn request_borderless() -> String {
    match GraphicsCaptureAccess::RequestAccessAsync(GraphicsCaptureAccessKind::Borderless)
        .and_then(|op| op.join())
    {
        Ok(s) => match s.0 {
            0 => "DeniedBySystem".into(),
            1 => "NotDeclaredByApp".into(),
            2 => "DeniedByUser".into(),
            3 => "UserPromptRequired".into(),
            4 => "Allowed".into(),
            n => format!("AppCapabilityAccessStatus({n})"),
        },
        Err(e) => format!("недоступно: {}", e2s(e)),
    }
}

/// A running capture session nobody reads frames from — for the border probe. Closed on drop.
pub struct Held {
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
}

impl Drop for Held {
    fn drop(&mut self) {
        let _ = self.session.Close();
        let _ = self.pool.Close();
    }
}

pub fn hold(target: Target, border_required: bool) -> Result<Held, String> {
    let _ = request_borderless();
    let (dev, _ctx) = d3d11()?;
    let dxgi: IDXGIDevice = dev.cast().map_err(e2s)?;
    // SAFETY: a valid DXGI device.
    let d3d: IDirect3DDevice = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi) }
        .map_err(e2s)?
        .cast()
        .map_err(e2s)?;
    let interop =
        windows_core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>().map_err(e2s)?;
    // SAFETY: handles come from the monitor/window lists.
    let item: GraphicsCaptureItem = unsafe {
        match target {
            Target::Display(h) => interop.CreateForMonitor(h),
            Target::Window(h) => interop.CreateForWindow(h),
        }
    }
    .map_err(e2s)?;
    let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
        &d3d,
        DirectXPixelFormat::R16G16B16A16Float,
        1,
        item.Size().map_err(e2s)?,
    )
    .map_err(e2s)?;
    let session = pool.CreateCaptureSession(&item).map_err(e2s)?;
    session.SetIsBorderRequired(border_required).map_err(e2s)?;
    session.StartCapture().map_err(e2s)?;
    Ok(Held { pool, session })
}

pub fn capture(target: Target) -> Result<(Frame, WgcMeta), String> {
    let access = request_borderless();

    let (dev, ctx) = d3d11()?;
    let dxgi: IDXGIDevice = dev.cast().map_err(e2s)?;
    // SAFETY: a valid DXGI device.
    let d3d: IDirect3DDevice = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi) }
        .map_err(e2s)?
        .cast()
        .map_err(e2s)?;
    let interop =
        windows_core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>().map_err(e2s)?;
    // SAFETY: handles come from the monitor/window lists.
    let item: GraphicsCaptureItem = unsafe {
        match target {
            Target::Display(h) => interop.CreateForMonitor(h),
            Target::Window(h) => interop.CreateForWindow(h),
        }
    }
    .map_err(e2s)?;
    let size = item.Size().map_err(e2s)?;
    let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
        &d3d,
        DirectXPixelFormat::R16G16B16A16Float,
        1,
        size,
    )
    .map_err(e2s)?;
    let session = pool.CreateCaptureSession(&item).map_err(e2s)?;
    let border_required_set = session.SetIsBorderRequired(false).is_ok();
    let cursor_disabled = session.SetIsCursorCaptureEnabled(false).is_ok();
    let t0 = Instant::now();
    session.StartCapture().map_err(e2s)?;
    let frame = loop {
        if let Ok(f) = pool.TryGetNextFrame() {
            break f;
        }
        if t0.elapsed() > Duration::from_millis(2000) {
            let _ = session.Close();
            let _ = pool.Close();
            return Err("WGC: за 2 с не прийшло жодного кадру (вікно згорнуте або сеанс без робочого столу?)".into());
        }
        std::thread::sleep(Duration::from_millis(2));
    };
    let first_frame_ms = t0.elapsed().as_secs_f64() * 1000.0;
    let content = frame.ContentSize().map_err(e2s)?;
    let surface = frame.Surface().map_err(e2s)?;
    let access_if: IDirect3DDxgiInterfaceAccess = surface.cast().map_err(e2s)?;
    // SAFETY: the frame surface is a D3D11 texture on `dev`.
    let tex: ID3D11Texture2D = unsafe { access_if.GetInterface() }.map_err(e2s)?;
    let pixels = read_texture_f16(
        &dev,
        &ctx,
        &tex,
        content.Width as u32,
        content.Height as u32,
    )?;
    let _ = frame.Close();
    let _ = session.Close();
    let _ = pool.Close();
    let (w, h) = pixels.1;
    Ok((
        Frame {
            width: w,
            height: h,
            pixels: Pixels::F16(pixels.0),
            transfer: Transfer::ScRgb,
        },
        WgcMeta {
            api: "wgc",
            borderless_access: access,
            border_required_set,
            cursor_disabled,
            item_size: [size.Width, size.Height],
            content_size: [content.Width, content.Height],
            first_frame_ms: (first_frame_ms * 10.0).round() / 10.0,
        },
    ))
}

/// Copy the top-left `w`×`h` of an FP16 texture through a staging texture. Honours RowPitch
/// (≠ width × 8 — the LH trap that sheared frames in CAPS-16).
pub fn read_texture_f16(
    dev: &ID3D11Device,
    ctx: &ID3D11DeviceContext,
    tex: &ID3D11Texture2D,
    w: u32,
    h: u32,
) -> Result<(Vec<u16>, (u32, u32)), String> {
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    // SAFETY: valid texture; staging copy, map, row-by-row read within RowPitch × h.
    unsafe {
        tex.GetDesc(&mut desc);
        let (w, h) = (w.min(desc.Width), h.min(desc.Height));
        let sdesc = D3D11_TEXTURE2D_DESC {
            Width: w,
            Height: h,
            MipLevels: 1,
            ArraySize: 1,
            Format: desc.Format,
            SampleDesc: desc.SampleDesc,
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
        };
        let mut staging = None;
        dev.CreateTexture2D(&sdesc, None, Some(&mut staging))
            .map_err(e2s)?;
        let staging = staging.ok_or("staging: None")?;
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
            .map_err(e2s)?;
        let row = (w * 4) as usize; // u16 per row
        let mut out = Vec::with_capacity(row * h as usize);
        for y in 0..h as usize {
            let p = (m.pData as *const u8)
                .add(y * m.RowPitch as usize)
                .cast::<u16>();
            out.extend_from_slice(std::slice::from_raw_parts(p, row));
        }
        ctx.Unmap(&staging, 0);
        Ok((out, (w, h)))
    }
}
