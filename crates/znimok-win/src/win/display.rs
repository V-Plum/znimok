//! Monitors → `DisplayInfo`: geometry, work area, DPI, refresh, friendly name, HDR state and the
//! SDR white level. Walks all DXGI adapters (a GPU can be listed twice, only one copy has outputs).

use windows::Win32::Devices::Display::{
    DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO,
    DISPLAYCONFIG_DEVICE_INFO_GET_SDR_WHITE_LEVEL, DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
    DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME, DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO,
    DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_SDR_WHITE_LEVEL,
    DISPLAYCONFIG_SOURCE_DEVICE_NAME, DISPLAYCONFIG_TARGET_DEVICE_NAME, DisplayConfigGetDeviceInfo,
    GetDisplayConfigBufferSizes, QDC_ONLY_ACTIVE_PATHS, QueryDisplayConfig,
};
use windows::Win32::Foundation::{LPARAM, RECT};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_COLOR_SPACE_RGB_FULL_G10_NONE_P709, DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020,
};
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1, IDXGIOutput6};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows_core::{BOOL, Interface};
use znimok_platform::{ColorInfo, DisplayId, DisplayInfo, Rect, Transfer};

/// Fallback SDR white when the system does not answer (LH: 200 nits on HDR, 80 on SDR).
const FALLBACK_WHITE_HDR: f32 = 200.0;
const FALLBACK_WHITE_SDR: f32 = 80.0;

pub struct Monitor {
    pub handle: HMONITOR,
    pub info: DisplayInfo,
}

pub fn wstr(buf: &[u16]) -> String {
    let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..n])
}

fn rect(r: RECT) -> Rect {
    Rect::new(
        r.left,
        r.top,
        (r.right - r.left).max(0) as u32,
        (r.bottom - r.top).max(0) as u32,
    )
}

unsafe extern "system" fn enum_proc(h: HMONITOR, _: HDC, _: *mut RECT, lp: LPARAM) -> BOOL {
    // SAFETY: lp is the &mut Vec passed by `list` for the duration of EnumDisplayMonitors.
    let v = unsafe { &mut *(lp.0 as *mut Vec<HMONITOR>) };
    v.push(h);
    BOOL(1)
}

/// All monitors, primary first.
pub fn list() -> Vec<Monitor> {
    super::com_thread();
    let mut handles: Vec<HMONITOR> = Vec::new();
    // SAFETY: the callback only pushes into `handles`, which outlives the call.
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(enum_proc),
            LPARAM(&mut handles as *mut _ as isize),
        );
    }
    let hdr_outputs = dxgi_hdr();
    let paths = displayconfig();
    let mut out = Vec::new();
    for h in handles {
        let mut mi = MONITORINFOEXW::default();
        mi.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        // SAFETY: MONITORINFOEXW starts with MONITORINFO and cbSize says which one it is.
        if !unsafe { GetMonitorInfoW(h, &mut mi as *mut _ as *mut MONITORINFO) }.as_bool() {
            continue;
        }
        let device = wstr(&mi.szDevice);
        let (mut dx, mut dy) = (96u32, 96u32);
        // SAFETY: valid monitor handle and out-pointers.
        let _ = unsafe { GetDpiForMonitor(h, MDT_EFFECTIVE_DPI, &mut dx, &mut dy) };
        let path = paths
            .iter()
            .find(|p| p.gdi_name.eq_ignore_ascii_case(&device));
        let hdr = hdr_outputs.contains(&(h.0 as isize))
            || path.and_then(|p| p.advanced_color).unwrap_or(false);
        let white = path.and_then(|p| p.sdr_white).unwrap_or(if hdr {
            FALLBACK_WHITE_HDR
        } else {
            FALLBACK_WHITE_SDR
        });
        let color = if hdr {
            ColorInfo {
                transfer: Transfer::ScRgb,
                sdr_white_nits: white,
                hdr: true,
            }
        } else {
            ColorInfo {
                sdr_white_nits: white,
                ..ColorInfo::SDR
            }
        };
        out.push(Monitor {
            handle: h,
            info: DisplayInfo {
                id: DisplayId(device.clone()),
                name: path
                    .map(|p| p.friendly.clone())
                    .filter(|n| !n.is_empty())
                    .unwrap_or(device),
                bounds: rect(mi.monitorInfo.rcMonitor),
                work_area: rect(mi.monitorInfo.rcWork),
                scale_factor: dx as f32 / 96.0,
                pixels_per_unit: 1.0,
                primary: mi.monitorInfo.dwFlags & 1 != 0,
                refresh_hz: path.and_then(|p| p.refresh),
                color,
            },
        });
    }
    out.sort_by_key(|m| !m.info.primary);
    out
}

/// HMONITORs of outputs in an HDR colour space (scRGB or HDR10).
fn dxgi_hdr() -> Vec<isize> {
    let mut v = Vec::new();
    // SAFETY: plain DXGI enumeration; every returned interface is owned by windows-rs.
    unsafe {
        let Ok(f) = CreateDXGIFactory1::<IDXGIFactory1>() else {
            return v;
        };
        let mut a = 0;
        while let Ok(ad) = f.EnumAdapters1(a) {
            a += 1;
            let mut o = 0;
            while let Ok(out) = ad.EnumOutputs(o) {
                o += 1;
                let Ok(d1) = out.cast::<IDXGIOutput6>().and_then(|o6| o6.GetDesc1()) else {
                    continue;
                };
                if d1.ColorSpace == DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020
                    || d1.ColorSpace == DXGI_COLOR_SPACE_RGB_FULL_G10_NONE_P709
                {
                    v.push(d1.Monitor.0 as isize);
                }
            }
        }
    }
    v
}

struct Path {
    gdi_name: String,
    friendly: String,
    refresh: Option<f32>,
    sdr_white: Option<f32>,
    advanced_color: Option<bool>,
}

fn displayconfig() -> Vec<Path> {
    let mut v = Vec::new();
    // SAFETY: sizes come from GetDisplayConfigBufferSizes; every request packet carries its own size.
    unsafe {
        let (mut np, mut nm) = (0u32, 0u32);
        if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut np, &mut nm).is_err() {
            return v;
        }
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); np as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); nm as usize];
        if QueryDisplayConfig(
            QDC_ONLY_ACTIVE_PATHS,
            &mut np,
            paths.as_mut_ptr(),
            &mut nm,
            modes.as_mut_ptr(),
            None,
        )
        .is_err()
        {
            return v;
        }
        paths.truncate(np as usize);
        for p in &paths {
            let mut src = DISPLAYCONFIG_SOURCE_DEVICE_NAME::default();
            src.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
            src.header.size = std::mem::size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32;
            src.header.adapterId = p.sourceInfo.adapterId;
            src.header.id = p.sourceInfo.id;
            if DisplayConfigGetDeviceInfo(&mut src.header) != 0 {
                continue;
            }
            let mut tgt = DISPLAYCONFIG_TARGET_DEVICE_NAME::default();
            tgt.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME;
            tgt.header.size = std::mem::size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32;
            tgt.header.adapterId = p.targetInfo.adapterId;
            tgt.header.id = p.targetInfo.id;
            let friendly = if DisplayConfigGetDeviceInfo(&mut tgt.header) == 0 {
                wstr(&tgt.monitorFriendlyDeviceName)
            } else {
                String::new()
            };
            // Request type 11 (GET_SDR_WHITE_LEVEL); 26 gave washed-out HDR shots in LH.
            let mut white = DISPLAYCONFIG_SDR_WHITE_LEVEL::default();
            white.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SDR_WHITE_LEVEL;
            white.header.size = std::mem::size_of::<DISPLAYCONFIG_SDR_WHITE_LEVEL>() as u32;
            white.header.adapterId = p.targetInfo.adapterId;
            white.header.id = p.targetInfo.id;
            let sdr_white = (DisplayConfigGetDeviceInfo(&mut white.header) == 0
                && white.SDRWhiteLevel > 0)
                .then(|| white.SDRWhiteLevel as f32 / 1000.0 * 80.0);
            let mut ac = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO::default();
            ac.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO;
            ac.header.size = std::mem::size_of::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO>() as u32;
            ac.header.adapterId = p.targetInfo.adapterId;
            ac.header.id = p.targetInfo.id;
            // bit 1 = advancedColorEnabled
            let advanced_color = (DisplayConfigGetDeviceInfo(&mut ac.header) == 0)
                .then_some(ac.Anonymous.value & 2 != 0);
            let r = p.targetInfo.refreshRate;
            let refresh = (r.Denominator != 0).then(|| r.Numerator as f32 / r.Denominator as f32);
            v.push(Path {
                gdi_name: wstr(&src.viewGdiDeviceName),
                friendly,
                refresh,
                sdr_white,
                advanced_color,
            });
        }
    }
    v
}
