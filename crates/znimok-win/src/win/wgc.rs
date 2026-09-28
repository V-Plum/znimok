//! Windows.Graphics.Capture: one still frame of a display or a window.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use windows::Graphics::Capture::{
    Direct3D11CaptureFramePool, GraphicsCaptureAccess, GraphicsCaptureAccessKind,
    GraphicsCaptureItem, GraphicsCaptureSession,
};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Foundation::{HMODULE, HWND};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice, ID3D11Device,
    ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Graphics::Gdi::HMONITOR;
use windows::Win32::System::WinRT::Direct3D11::{
    CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows_core::Interface;
use znimok_platform::PixelFormat;

use super::{Raw, e2p, read_texture};

pub enum Item {
    Display(HMONITOR),
    Window(HWND),
}

pub fn supported() -> bool {
    super::com_thread();
    GraphicsCaptureSession::IsSupported().unwrap_or(false)
}

/// `RequestAccessAsync(Borderless)`, asked once per process: "Allowed" is what makes
/// `IsBorderRequired = false` take effect (otherwise it is silently ignored).
pub fn borderless() -> bool {
    super::com_thread();
    static ALLOWED: OnceLock<bool> = OnceLock::new();
    *ALLOWED.get_or_init(|| {
        GraphicsCaptureAccess::RequestAccessAsync(GraphicsCaptureAccessKind::Borderless)
            .and_then(|op| op.join())
            .is_ok_and(|s| s.0 == 4)
    })
}

pub fn d3d11() -> znimok_platform::Result<(ID3D11Device, ID3D11DeviceContext)> {
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
    .map_err(e2p)?;
    match (dev, ctx) {
        (Some(d), Some(c)) => Ok((d, c)),
        _ => Err(znimok_platform::PlatformError::Other(
            "D3D11: немає пристрою".into(),
        )),
    }
}

/// One frame. `fp16`: pool in R16G16B16A16Float (HDR display), else B8G8R8A8.
pub fn capture(item: Item, fp16: bool) -> znimok_platform::Result<Raw> {
    super::com_thread();
    let _ = borderless();
    let (dev, ctx) = d3d11()?;
    let dxgi: IDXGIDevice = dev.cast().map_err(e2p)?;
    // SAFETY: a valid DXGI device.
    let d3d: IDirect3DDevice = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi) }
        .map_err(e2p)?
        .cast()
        .map_err(e2p)?;
    let interop =
        windows_core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>().map_err(e2p)?;
    // SAFETY: handles come from the monitor/window lists; a stale one makes the call fail.
    let item: GraphicsCaptureItem = unsafe {
        match item {
            Item::Display(h) => interop.CreateForMonitor(h),
            Item::Window(h) => interop.CreateForWindow(h),
        }
    }
    .map_err(e2p)?;
    let size = item.Size().map_err(e2p)?;
    let (fmt, pf) = if fp16 {
        (
            DirectXPixelFormat::R16G16B16A16Float,
            PixelFormat::Rgba16Float,
        )
    } else {
        (
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            PixelFormat::Bgra8,
        )
    };
    let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(&d3d, fmt, 1, size).map_err(e2p)?;
    let session = pool.CreateCaptureSession(&item).map_err(e2p)?;
    let _ = session.SetIsBorderRequired(false);
    let _ = session.SetIsCursorCaptureEnabled(false);
    let close = || {
        let _ = session.Close();
        let _ = pool.Close();
    };
    session.StartCapture().map_err(e2p)?;
    let t0 = Instant::now();
    let frame = loop {
        if let Ok(f) = pool.TryGetNextFrame() {
            break f;
        }
        if t0.elapsed() > Duration::from_millis(2000) {
            close();
            return Err(znimok_platform::PlatformError::NotFound(
                "WGC: за 2 с жодного кадру (вікно згорнуте або сеанс без робочого столу)".into(),
            ));
        }
        std::thread::sleep(Duration::from_millis(2));
    };
    let result = (|| {
        let content = frame.ContentSize().map_err(e2p)?;
        let access: IDirect3DDxgiInterfaceAccess =
            frame.Surface().map_err(e2p)?.cast().map_err(e2p)?;
        // SAFETY: the frame surface is a D3D11 texture on `dev`.
        let tex: ID3D11Texture2D = unsafe { access.GetInterface() }.map_err(e2p)?;
        // The pool texture can be larger than the content: copy ContentSize only.
        read_texture(
            &dev,
            &ctx,
            &tex,
            content.Width as u32,
            content.Height as u32,
            pf,
        )
    })();
    let _ = frame.Close();
    close();
    result
}
