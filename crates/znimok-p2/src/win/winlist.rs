//! Top-level windows in Z-order with their DWM bounds.
//!
//! LH traps: bounds come from `DWMWA_EXTENDED_FRAME_BOUNDS`, not `GetWindowRect` (Windows 11 has an
//! invisible resize shadow around every window); cloaked windows (other virtual desktops, suspended
//! UWP) are visible to `IsWindowVisible` but not on screen — skip them.

use serde::Serialize;
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, RECT};
use windows::Win32::Graphics::Dwm::{
    DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GWL_EXSTYLE, GetClassNameW, GetWindowLongW, GetWindowRect, GetWindowTextW,
    GetWindowThreadProcessId, IsIconic, IsWindowVisible, WS_EX_TOOLWINDOW,
};
use windows_core::{BOOL, PWSTR};

#[derive(Clone, Debug, Serialize)]
pub struct Window {
    #[serde(serialize_with = "ser_handle")]
    pub hwnd: isize,
    pub title: String,
    pub class: String,
    pub pid: u32,
    pub exe: String,
    /// DWM extended frame bounds, physical pixels.
    pub bounds: [i32; 4],
    /// GetWindowRect for comparison (includes the invisible shadow on Windows 11).
    pub window_rect: [i32; 4],
    pub dpi: u32,
    pub tool_window: bool,
}

fn ser_handle<S: serde::Serializer>(v: &isize, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&format!("0x{v:X}"))
}

impl Window {
    pub fn handle(&self) -> HWND {
        HWND(self.hwnd as *mut core::ffi::c_void)
    }
    pub fn size(&self) -> (i32, i32) {
        (
            self.bounds[2] - self.bounds[0],
            self.bounds[3] - self.bounds[1],
        )
    }
}

unsafe extern "system" fn enum_proc(h: HWND, lp: LPARAM) -> BOOL {
    // SAFETY: lp is the &mut Vec passed by `list` for the duration of EnumWindows.
    let v = unsafe { &mut *(lp.0 as *mut Vec<HWND>) };
    v.push(h);
    BOOL(1)
}

pub fn list() -> Result<Vec<Window>, String> {
    let mut handles: Vec<HWND> = Vec::new();
    // SAFETY: the callback only pushes into `handles`, which outlives the call.
    unsafe { EnumWindows(Some(enum_proc), LPARAM(&mut handles as *mut _ as isize)) }
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for h in handles {
        // SAFETY: every call gets a valid window handle (it may die meanwhile — then calls just fail).
        unsafe {
            if !IsWindowVisible(h).as_bool() || IsIconic(h).as_bool() {
                continue;
            }
            let mut cloaked = 0u32;
            let _ = DwmGetWindowAttribute(h, DWMWA_CLOAKED, &mut cloaked as *mut _ as *mut _, 4);
            if cloaked != 0 {
                continue;
            }
            let mut r = RECT::default();
            if DwmGetWindowAttribute(
                h,
                DWMWA_EXTENDED_FRAME_BOUNDS,
                &mut r as *mut _ as *mut _,
                std::mem::size_of::<RECT>() as u32,
            )
            .is_err()
                || r.right - r.left <= 0
                || r.bottom - r.top <= 0
            {
                continue;
            }
            let mut wr = RECT::default();
            let _ = GetWindowRect(h, &mut wr);
            let mut t = [0u16; 512];
            let n = GetWindowTextW(h, &mut t).max(0) as usize;
            let mut c = [0u16; 256];
            let m = GetClassNameW(h, &mut c).max(0) as usize;
            let mut pid = 0u32;
            GetWindowThreadProcessId(h, Some(&mut pid));
            out.push(Window {
                hwnd: h.0 as isize,
                title: String::from_utf16_lossy(&t[..n]),
                class: String::from_utf16_lossy(&c[..m]),
                pid,
                exe: exe_name(pid),
                bounds: [r.left, r.top, r.right, r.bottom],
                window_rect: [wr.left, wr.top, wr.right, wr.bottom],
                dpi: GetDpiForWindow(h),
                tool_window: GetWindowLongW(h, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 != 0,
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
