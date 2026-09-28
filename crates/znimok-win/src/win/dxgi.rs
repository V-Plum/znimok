//! DXGI Desktop Duplication — the other API for a whole display (LH `CapGrabMonitor`).
//! Walk all adapters for this monitor's output; `DuplicateOutput1` with FP16 / 10-bit / BGRA8 so an
//! HDR desktop comes in its own format; skip the first (black) frame; honour RowPitch. Over RDP
//! duplication is refused (E_ACCESSDENIED) — WGC is the answer there.

use std::time::{Duration, Instant};

use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020, DXGI_FORMAT_B8G8R8A8_UNORM,
    DXGI_FORMAT_R10G10B10A2_UNORM, DXGI_FORMAT_R16G16B16A16_FLOAT,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_FRAME_INFO, IDXGIAdapter,
    IDXGIFactory1, IDXGIOutput6, IDXGIResource,
};
use windows::Win32::Graphics::Gdi::HMONITOR;
use windows_core::Interface;
use znimok_platform::{PixelFormat, PlatformError, Transfer};

use super::{Raw, e2p, read_texture};

/// One frame of the monitor; also says whether the desktop is in PQ (HDR10) or scRGB.
pub fn capture(mon: HMONITOR) -> znimok_platform::Result<(Raw, Transfer)> {
    // SAFETY: DXGI/D3D11 calls with owned interfaces.
    unsafe {
        let f = CreateDXGIFactory1::<IDXGIFactory1>().map_err(e2p)?;
        let mut found = None;
        let mut a = 0;
        'outer: while let Ok(ad) = f.EnumAdapters1(a) {
            a += 1;
            let mut o = 0;
            while let Ok(out) = ad.EnumOutputs(o) {
                o += 1;
                if out.GetDesc().is_ok_and(|d| d.Monitor == mon) {
                    found = Some((ad.clone(), out));
                    break 'outer;
                }
            }
        }
        let (ad, out) = found.ok_or(PlatformError::NotFound(
            "DXGI: немає виходу для цього монітора".into(),
        ))?;
        let out6: IDXGIOutput6 = out.cast().map_err(e2p)?;
        let cs = out6.GetDesc1().map(|d| d.ColorSpace).map_err(e2p)?;
        let (mut dev, mut ctx) = (None, None);
        let base: IDXGIAdapter = ad.cast().map_err(e2p)?;
        D3D11CreateDevice(
            &base,
            D3D_DRIVER_TYPE_UNKNOWN,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut dev),
            None,
            Some(&mut ctx),
        )
        .map_err(e2p)?;
        let (Some(dev), Some(ctx)) = (dev, ctx) else {
            return Err(PlatformError::Other("D3D11: немає пристрою".into()));
        };
        let formats = [
            DXGI_FORMAT_R16G16B16A16_FLOAT,
            DXGI_FORMAT_R10G10B10A2_UNORM,
            DXGI_FORMAT_B8G8R8A8_UNORM,
        ];
        let dup = out6.DuplicateOutput1(&dev, 0, &formats).map_err(e2p)?;
        let fmt = dup.GetDesc().ModeDesc.Format;
        let pf = if fmt == DXGI_FORMAT_R16G16B16A16_FLOAT {
            PixelFormat::Rgba16Float
        } else if fmt == DXGI_FORMAT_R10G10B10A2_UNORM {
            PixelFormat::Rgb10A2
        } else {
            PixelFormat::Bgra8
        };
        let t0 = Instant::now();
        let tex: ID3D11Texture2D = loop {
            if t0.elapsed() > Duration::from_millis(1200) {
                return Err(PlatformError::Other(
                    "DXGI: за 1,2 с жодного непорожнього кадру".into(),
                ));
            }
            let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut res: Option<IDXGIResource> = None;
            match dup.AcquireNextFrame(60, &mut info, &mut res) {
                Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => continue,
                Err(e) => return Err(e2p(e)),
                Ok(()) => {}
            }
            // The first frame is black: nothing presented yet.
            if info.AccumulatedFrames == 0 && info.LastPresentTime == 0 {
                let _ = dup.ReleaseFrame();
                continue;
            }
            match res.map(|r| r.cast::<ID3D11Texture2D>()) {
                Some(Ok(t)) => break t,
                _ => {
                    let _ = dup.ReleaseFrame();
                    return Err(PlatformError::Other("DXGI: кадр без текстури".into()));
                }
            }
        };
        let raw = read_texture(&dev, &ctx, &tex, u32::MAX, u32::MAX, pf);
        let _ = dup.ReleaseFrame();
        let transfer = match pf {
            PixelFormat::Rgba16Float => Transfer::ScRgb,
            PixelFormat::Rgb10A2 if cs == DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020 => {
                Transfer::Pq
            }
            _ => Transfer::Srgb,
        };
        Ok((raw?, transfer))
    }
}
