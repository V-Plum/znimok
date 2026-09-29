//! What the first-run guide and the settings ask the OS (ZK-57): start at login (Run key on
//! Windows, SMAppService on macOS — ZK-77) and, on macOS, the screen-recording permission.

use znimok_platform::{Autostart, AutostartState};

fn autostart() -> Option<Box<dyn Autostart>> {
    #[cfg(windows)]
    {
        znimok_win::RunKeyAutostart::for_current_exe()
            .ok()
            .map(|a| Box::new(a) as Box<dyn Autostart>)
    }
    #[cfg(target_os = "macos")]
    {
        Some(Box::new(znimok_mac::MacAutostart::new()))
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        None
    }
}

pub fn autostart_state() -> AutostartState {
    autostart()
        .and_then(|a| a.state().ok())
        .unwrap_or(AutostartState::Unavailable)
}

/// On: registers (and on Windows clears "disabled" in Task Manager — the user chose it here).
/// macOS may still want the user's approval in System Settings; that page opens then.
pub fn set_autostart(on: bool) -> Result<AutostartState, String> {
    let a = autostart().ok_or("unavailable")?;
    a.set_enabled(on).map_err(|e| e.to_string())?;
    let st = a.state().map_err(|e| e.to_string())?;
    #[cfg(target_os = "macos")]
    if on && st == AutostartState::NeedsApproval {
        znimok_mac::MacAutostart::open_login_items();
    }
    Ok(st)
}

/// Screen recording is allowed (always on Windows).
pub fn screen_ok() -> bool {
    #[cfg(target_os = "macos")]
    {
        use znimok_platform::{Permission, PermissionState, Permissions};
        matches!(
            znimok_mac::MacCapture::new().status(Permission::ScreenRecording),
            PermissionState::Granted | PermissionState::NotNeeded
        )
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

/// "Allow": the system prompt the first time, the settings page after that.
pub fn ask_screen() {
    #[cfg(target_os = "macos")]
    {
        use znimok_platform::{Permission, PermissionState, Permissions};
        let c = znimok_mac::MacCapture::new();
        if c.request(Permission::ScreenRecording) != PermissionState::Granted {
            let _ = c.open_settings(Permission::ScreenRecording);
        }
    }
}

/// The system's own light / dark (not the window's: on macOS our window carries its own
/// appearance, so winit's window theme would only echo it back). Read at start and every few
/// seconds (ZK-46).
pub fn system_dark() -> bool {
    #[cfg(windows)]
    {
        use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
        use windows::core::w;
        let mut v: u32 = 1;
        let mut len = std::mem::size_of::<u32>() as u32;
        // SAFETY: a DWORD read into a u32 of the stated size.
        let r = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
                w!("AppsUseLightTheme"),
                RRF_RT_REG_DWORD,
                None,
                Some((&mut v as *mut u32).cast()),
                Some(&mut len),
            )
        };
        // No value (older systems): light.
        r.is_ok() && v == 0
    }
    #[cfg(target_os = "macos")]
    {
        use objc2_foundation::{NSString, NSUserDefaults};
        let style = NSUserDefaults::standardUserDefaults()
            .stringForKey(&NSString::from_str("AppleInterfaceStyle"));
        style.is_some_and(|s| s.to_string().eq_ignore_ascii_case("dark"))
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        true
    }
}
