//! DXGI Desktop Duplication — the alternative for a whole display (LH `CapGrabMonitor`).
//!
//! LH traps: walk all adapters to find the output of this monitor; `DuplicateOutput1` with FP16,
//! 10-bit and BGRA8 so an HDR desktop comes in its own format; the first frame is black
//! (`AccumulatedFrames == 0 && LastPresentTime == 0`) — skip it, the next comes within ~16 ms even on a
//! still screen; RowPitch ≠ width × bpp. Over RDP duplication is refused (E_ACCESSDENIED) — WGC is
//! the answer there.

use std::time::{Duration, Instant};

use serde::Serialize;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BOX, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ,
    D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
    D3D11CreateDevice, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020, DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM,
    DXGI_FORMAT_R10G10B10A2_UNORM, DXGI_FORMAT_R16G16B16A16_FLOAT,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_FRAME_INFO, IDXGIAdapter,
    IDXGIFactory1, IDXGIOutput6, IDXGIResource,
};
use windows::Win32::Graphics::Gdi::HMONITOR;
use windows_core::Interface;

use crate::tone::{Frame, Pixels, Transfer};

#[derive(Debug, Serialize)]
pub struct DxgiMeta {
    pub api: &'static str,
    pub adapter: String,
    pub format: String,
    pub color_space: String,
    pub skipped_frames: u32,
    pub first_frame_ms: f64,
}

fn e2s(e: windows_core::Error) -> String {
    format!("{} (0x{:08X})", e.message(), e.code().0)
}

pub fn capture(mon: HMONITOR) -> Result<(Frame, DxgiMeta), String> {
    // SAFETY: DXGI/D3D11 calls with owned interfaces; the mapped memory is read within RowPitch × height.
    unsafe {
        let f = CreateDXGIFactory1::<IDXGIFactory1>().map_err(e2s)?;
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
        let (ad, out) = found.ok_or("DXGI: не знайдено виходу для цього монітора")?;
        let adapter_name = ad
            .GetDesc1()
            .map(|d| {
                String::from_utf16_lossy(&d.Description)
                    .trim_end_matches('\0')
                    .to_string()
            })
            .unwrap_or_default();
        let out6: IDXGIOutput6 = out.cast().map_err(e2s)?;
        let cs = out6.GetDesc1().map(|d| d.ColorSpace).map_err(e2s)?;
        let (mut dev, mut ctx) = (None, None);
        let base: IDXGIAdapter = ad.cast().map_err(e2s)?;
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
        .map_err(e2s)?;
        let (dev, ctx) = (dev.ok_or("D3D11: None")?, ctx.ok_or("D3D11: None")?);
        let formats = [
            DXGI_FORMAT_R16G16B16A16_FLOAT,
            DXGI_FORMAT_R10G10B10A2_UNORM,
            DXGI_FORMAT_B8G8R8A8_UNORM,
        ];
        let dup = out6
            .DuplicateOutput1(&dev, 0, &formats)
            .map_err(|e| format!("DuplicateOutput1: {}", e2s(e)))?;
        let ddesc = dup.GetDesc();
        let fmt: DXGI_FORMAT = ddesc.ModeDesc.Format;
        let t0 = Instant::now();
        let mut skipped = 0;
        let tex: ID3D11Texture2D = loop {
            if t0.elapsed() > Duration::from_millis(1200) {
                return Err("DXGI: за 1,2 с жодного непорожнього кадру".into());
            }
            let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut res: Option<IDXGIResource> = None;
            match dup.AcquireNextFrame(60, &mut info, &mut res) {
                Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => continue,
                Err(e) => return Err(format!("AcquireNextFrame: {}", e2s(e))),
                Ok(()) => {}
            }
            if info.AccumulatedFrames == 0 && info.LastPresentTime == 0 {
                skipped += 1;
                let _ = dup.ReleaseFrame();
                continue;
            }
            let r = res.ok_or("кадр без ресурсу")?;
            break r.cast().map_err(e2s)?;
        };
        let first_frame_ms = t0.elapsed().as_secs_f64() * 1000.0;
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        tex.GetDesc(&mut desc);
        let (w, h) = (desc.Width, desc.Height);
        let sdesc = D3D11_TEXTURE2D_DESC {
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
            MipLevels: 1,
            ArraySize: 1,
            ..desc
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
        ctx.CopySubresourceRegion(&staging, 0, 0, 0, 0, &tex, 0, Some(&bx));
        let _ = dup.ReleaseFrame();
        let mut m = D3D11_MAPPED_SUBRESOURCE::default();
        ctx.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut m))
            .map_err(e2s)?;
        let bpp = if fmt == DXGI_FORMAT_R16G16B16A16_FLOAT {
            8
        } else {
            4
        };
        let mut bytes = Vec::with_capacity((w * h * bpp) as usize);
        for y in 0..h as usize {
            let p = (m.pData as *const u8).add(y * m.RowPitch as usize);
            bytes.extend_from_slice(std::slice::from_raw_parts(p, (w * bpp) as usize));
        }
        ctx.Unmap(&staging, 0);
        let pq = cs == DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020;
        let (pixels, transfer, fname) = if fmt == DXGI_FORMAT_R16G16B16A16_FLOAT {
            (
                Pixels::F16(
                    bytes
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|c| u16::from_le_bytes(*c))
                        .collect(),
                ),
                Transfer::ScRgb,
                "R16G16B16A16_FLOAT",
            )
        } else if fmt == DXGI_FORMAT_R10G10B10A2_UNORM {
            let words = bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|c| u32::from_le_bytes(*c))
                .collect();
            (
                Pixels::Rgb10a2(words),
                if pq { Transfer::Pq2020 } else { Transfer::Srgb },
                "R10G10B10A2_UNORM",
            )
        } else {
            (Pixels::Bgra8(bytes), Transfer::Srgb, "B8G8R8A8_UNORM")
        };
        Ok((
            Frame {
                width: w,
                height: h,
                pixels,
                transfer,
            },
            DxgiMeta {
                api: "dxgi",
                adapter: adapter_name,
                format: fname.into(),
                color_space: super::display::color_space_name(cs.0),
                skipped_frames: skipped,
                first_frame_ms: (first_frame_ms * 10.0).round() / 10.0,
            },
        ))
    }
}
