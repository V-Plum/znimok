//! Top-level windows front to back, with DWM bounds.
//!
//! Bounds come from `DWMWA_EXTENDED_FRAME_BOUNDS`, not `GetWindowRect` (Windows 11 keeps an invisible
//! resize shadow around every window). Cloaked windows (other virtual desktops, suspended UWP) pass
//! `IsWindowVisible` but are not on screen — skipped, as are minimized and zero-size ones.
//! The list must be taken **before** the overlay is shown, or the overlay is the first entry.

use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, RECT};
use windows::Win32::Graphics::Dwm::{
    DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MONITORINFOEXW, MonitorFromWindow,
};
use windows::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    QueryFullProcessImageNameW,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindow, IsWindowVisible,
};
use windows_core::{BOOL, PWSTR};
use znimok_platform::{DisplayId, Rect, WindowId, WindowInfo};

use super::display::wstr;

pub fn hwnd(id: WindowId) -> HWND {
    HWND(id.0 as usize as *mut core::ffi::c_void)
}

unsafe extern "system" fn enum_proc(h: HWND, lp: LPARAM) -> BOOL {
    // SAFETY: lp is the &mut Vec passed by `list` for the duration of EnumWindows.
    let v = unsafe { &mut *(lp.0 as *mut Vec<HWND>) };
    v.push(h);
    BOOL(1)
}

/// DWM visible frame of a window, `None` if it has none (gone, zero size).
pub fn dwm_bounds(h: HWND) -> Option<Rect> {
    let mut r = RECT::default();
    // SAFETY: valid out-pointer of the documented size.
    let ok = unsafe {
        DwmGetWindowAttribute(
            h,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut r as *mut _ as *mut _,
            std::mem::size_of::<RECT>() as u32,
        )
    }
    .is_ok();
    (ok && r.right > r.left && r.bottom > r.top).then(|| {
        Rect::new(
            r.left,
            r.top,
            (r.right - r.left) as u32,
            (r.bottom - r.top) as u32,
        )
    })
}

pub fn is_alive(h: HWND) -> bool {
    // SAFETY: IsWindow accepts any value.
    unsafe { IsWindow(Some(h)).as_bool() }
}

pub fn is_minimized(h: HWND) -> bool {
    // SAFETY: as above.
    unsafe { IsIconic(h).as_bool() }
}

pub fn display_of(h: HWND) -> Option<DisplayId> {
    // SAFETY: MonitorFromWindow never fails with DEFAULTTONEAREST; MONITORINFOEXW sized by cbSize.
    unsafe {
        let m = MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFOEXW::default();
        mi.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        GetMonitorInfoW(m, &mut mi as *mut _ as *mut MONITORINFO)
            .as_bool()
            .then(|| DisplayId(wstr(&mi.szDevice)))
    }
}

pub fn list() -> Result<Vec<WindowInfo>, String> {
    let mut handles: Vec<HWND> = Vec::new();
    // SAFETY: the callback only pushes into `handles`, which outlives the call.
    unsafe { EnumWindows(Some(enum_proc), LPARAM(&mut handles as *mut _ as isize)) }
        .map_err(|e| e.to_string())?;
    // SAFETY: no arguments.
    let me = unsafe { GetCurrentProcessId() };
    let mut out = Vec::new();
    for h in handles {
        // SAFETY: every call gets a window handle (it may die meanwhile — then calls just fail).
        unsafe {
            if !IsWindowVisible(h).as_bool() || IsIconic(h).as_bool() {
                continue;
            }
            let mut cloaked = 0u32;
            let _ = DwmGetWindowAttribute(h, DWMWA_CLOAKED, &mut cloaked as *mut _ as *mut _, 4);
            if cloaked != 0 {
                continue;
            }
            let Some(bounds) = dwm_bounds(h) else {
                continue;
            };
            let mut t = [0u16; 512];
            let n = GetWindowTextW(h, &mut t).max(0) as usize;
            let mut pid = 0u32;
            GetWindowThreadProcessId(h, Some(&mut pid));
            let dpi = GetDpiForWindow(h);
            out.push(WindowInfo {
                id: WindowId(h.0 as usize as u64),
                title: String::from_utf16_lossy(&t[..n]),
                app: exe_name(pid),
                pid,
                bounds,
                display: display_of(h),
                scale_factor: if dpi > 0 { dpi as f32 / 96.0 } else { 1.0 },
                minimized: false,
                own: pid == me,
            });
        }
    }
    Ok(out)
}

fn exe_name(pid: u32) -> String {
    // SAFETY: the handle is closed below; the buffer length is passed in `len`.
    unsafe {
        let Ok(p) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return String::new();
        };
        let mut buf = [0u16; 520];
        let mut len = buf.len() as u32;
        let ok =
            QueryFullProcessImageNameW(p, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len)
                .is_ok();
        let _ = CloseHandle(p);
        if !ok {
            return String::new();
        }
        let full = String::from_utf16_lossy(&buf[..len as usize]);
        full.rsplit('\\').next().unwrap_or(&full).to_string()
    }
}
