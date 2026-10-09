//! The host's registration for the browsers, in the user's profile (no administrator):
//!
//! - Windows: the manifest in `%LOCALAPPDATA%\Znimok\NativeMessaging`, and its path as the default
//!   value of `HKCU\<browser key>\NativeMessagingHosts\…` ([`KEYS`]). Every channel of Chrome
//!   (Beta, Dev, Canary) reads the `Google\Chrome` key, every channel of Edge the `Microsoft\Edge`
//!   one; Opera reads Chrome's;
//! - macOS: the manifest in `~/Library/Application Support/<browser>/NativeMessagingHosts`, where
//!   every channel has a folder of its own ([`MAC_BROWSERS`]: Chrome Canary is
//!   `Google/Chrome Canary`). Written for the browsers that are there (their folder exists), and
//!   always for Chrome, Edge and Chromium as before. ZK-300: the extension in Chrome Canary said
//!   «Znimok not found» with the app running — there was no manifest for it to start the host.
//!
//! The app registers on every start (cheap and idempotent: it follows the app when it moves); the
//! uninstaller removes it.

use std::path::{Path, PathBuf};

use crate::{EXTENSION_IDS, HOST_NAME};

/// The host manifest for `host_exe` (the `znimok` CLI).
pub fn manifest(host_exe: &Path) -> String {
    let origins: Vec<String> = EXTENSION_IDS
        .iter()
        .map(|id| format!("chrome-extension://{id}/"))
        .collect();
    serde_json::to_string_pretty(&serde_json::json!({
        "name": HOST_NAME,
        "description": "Znimok — the browser log of a recording",
        "path": host_exe.to_string_lossy(),
        "type": "stdio",
        "allowed_origins": origins,
    }))
    .unwrap_or_default()
}

#[cfg(windows)]
const KEYS: [&str; 5] = [
    "Software\\Google\\Chrome\\NativeMessagingHosts",
    "Software\\Microsoft\\Edge\\NativeMessagingHosts",
    "Software\\Chromium\\NativeMessagingHosts",
    "Software\\BraveSoftware\\Brave-Browser\\NativeMessagingHosts",
    "Software\\Vivaldi\\NativeMessagingHosts",
];

/// The browsers' folders in `~/Library/Application Support` (each holds `NativeMessagingHosts`);
/// the first three get the manifest even when they are not installed (yet).
pub const MAC_BROWSERS: [&str; 19] = [
    "Google/Chrome",
    "Microsoft Edge",
    "Chromium",
    "Google/Chrome Beta",
    "Google/Chrome Dev",
    "Google/Chrome Canary",
    "Google/Chrome for Testing",
    "Microsoft Edge Beta",
    "Microsoft Edge Dev",
    "Microsoft Edge Canary",
    "BraveSoftware/Brave-Browser",
    "BraveSoftware/Brave-Browser-Beta",
    "BraveSoftware/Brave-Browser-Nightly",
    "Vivaldi",
    "Vivaldi Snapshot",
    "com.operasoftware.Opera",
    "com.operasoftware.OperaGX",
    "Arc/User Data",
    "Thorium",
];

/// The `NativeMessagingHosts` folders under `base` (`~/Library/Application Support`) to write
/// the manifest into: the browsers that are there, and always the first three.
pub fn mac_host_dirs(base: &Path) -> Vec<PathBuf> {
    MAC_BROWSERS
        .iter()
        .enumerate()
        .filter(|(i, b)| *i < 3 || base.join(b).is_dir())
        .map(|(_, b)| base.join(b).join("NativeMessagingHosts"))
        .collect()
}

#[cfg(windows)]
fn manifest_path() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|d| {
        PathBuf::from(d)
            .join("Znimok")
            .join("NativeMessaging")
            .join(format!("{HOST_NAME}.json"))
    })
}

#[cfg(target_os = "macos")]
fn browser_dirs() -> Vec<PathBuf> {
    let Some(home) = std::env::var_os("HOME") else {
        return Vec::new();
    };
    mac_host_dirs(&PathBuf::from(home).join("Library/Application Support"))
}

/// Registers `host_exe` as the host for the Chromium browsers (see the module's doc).
pub fn register(host_exe: &Path) -> Result<(), String> {
    let m = manifest(host_exe);
    #[cfg(windows)]
    {
        let path = manifest_path().ok_or("no LOCALAPPDATA")?;
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        if std::fs::read_to_string(&path).ok().as_deref() != Some(m.as_str()) {
            std::fs::write(&path, &m).map_err(|e| e.to_string())?;
        }
        for k in KEYS {
            win::set_default(&format!("{k}\\{HOST_NAME}"), &path.to_string_lossy())?;
        }
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        for d in browser_dirs() {
            std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
            let p = d.join(format!("{HOST_NAME}.json"));
            if std::fs::read_to_string(&p).ok().as_deref() != Some(m.as_str()) {
                std::fs::write(&p, &m).map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = m;
        Err("not on this system".into())
    }
}

/// Removes the registration.
pub fn unregister() -> Result<(), String> {
    #[cfg(windows)]
    {
        for k in KEYS {
            win::delete(&format!("{k}\\{HOST_NAME}"));
        }
        if let Some(p) = manifest_path() {
            let _ = std::fs::remove_file(p);
        }
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        for d in browser_dirs() {
            let _ = std::fs::remove_file(d.join(format!("{HOST_NAME}.json")));
        }
        Ok(())
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        Ok(())
    }
}

/// Where the manifest the browsers read lies (for checks).
pub fn registered_manifest() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let p = win::get_default(&format!("{}\\{HOST_NAME}", KEYS[0]))?;
        Some(PathBuf::from(p))
    }
    #[cfg(target_os = "macos")]
    {
        browser_dirs()
            .into_iter()
            .map(|d| d.join(format!("{HOST_NAME}.json")))
            .find(|p| p.is_file())
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        None
    }
}

#[cfg(windows)]
mod win {
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ,
        RRF_RT_REG_SZ, RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegGetValueW, RegSetValueExW,
    };
    use windows::core::{HSTRING, PCWSTR};

    pub fn set_default(subkey: &str, value: &str) -> Result<(), String> {
        let mut key = HKEY::default();
        // SAFETY: registry calls on the current user's hive with owned strings; the key is closed.
        unsafe {
            let r = RegCreateKeyExW(
                HKEY_CURRENT_USER,
                &HSTRING::from(subkey),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_WRITE | KEY_READ,
                None,
                &mut key,
                None,
            );
            if r != ERROR_SUCCESS {
                return Err(format!("RegCreateKeyEx {subkey}: {r:?}"));
            }
            let wide: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
            let bytes = std::slice::from_raw_parts(wide.as_ptr().cast::<u8>(), wide.len() * 2);
            let r = RegSetValueExW(key, PCWSTR::null(), None, REG_SZ, Some(bytes));
            let _ = RegCloseKey(key);
            if r != ERROR_SUCCESS {
                return Err(format!("RegSetValueEx {subkey}: {r:?}"));
            }
        }
        Ok(())
    }

    pub fn get_default(subkey: &str) -> Option<String> {
        let mut buf = vec![0u16; 2048];
        let mut len = (buf.len() * 2) as u32;
        // SAFETY: a read into our own buffer of `len` bytes.
        let r = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                &HSTRING::from(subkey),
                PCWSTR::null(),
                RRF_RT_REG_SZ,
                None,
                Some(buf.as_mut_ptr().cast()),
                Some(&mut len),
            )
        };
        if r != ERROR_SUCCESS {
            return None;
        }
        let n = (len as usize / 2).saturating_sub(1);
        Some(String::from_utf16_lossy(&buf[..n]))
    }

    pub fn delete(subkey: &str) {
        // SAFETY: deletes our own key under the current user's hive.
        unsafe {
            let _ = RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(subkey));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_manifest_names_the_host_and_the_extension() {
        let m: serde_json::Value =
            serde_json::from_str(&manifest(Path::new("C:/Program Files/Znimok/znimok.exe")))
                .unwrap();
        assert_eq!(m["name"], HOST_NAME);
        assert_eq!(m["type"], "stdio");
        assert_eq!(
            m["allowed_origins"],
            serde_json::json!([
                format!("chrome-extension://{}/", crate::DEV_EXTENSION_ID),
                format!("chrome-extension://{}/", crate::STORE_EXTENSION_ID),
            ])
        );
    }
}

#[cfg(test)]
mod host_dirs {
    use super::*;

    /// ZK-300: Chrome Canary (and the other channels and Chromium browsers) get the manifest when
    /// they are installed; the three of old always do; absent ones are not created.
    #[test]
    fn the_manifest_goes_to_every_installed_browser() {
        let base = std::env::temp_dir().join(format!("znimok-reg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("Google/Chrome Canary")).unwrap();
        std::fs::create_dir_all(base.join("BraveSoftware/Brave-Browser")).unwrap();
        std::fs::create_dir_all(base.join("Arc/User Data")).unwrap();
        let dirs = mac_host_dirs(&base);
        let has = |b: &str| dirs.contains(&base.join(b).join("NativeMessagingHosts"));
        for b in [
            "Google/Chrome",
            "Microsoft Edge",
            "Chromium",
            "Google/Chrome Canary",
            "BraveSoftware/Brave-Browser",
            "Arc/User Data",
        ] {
            assert!(has(b), "{b}");
        }
        assert!(!has("Vivaldi") && !has("Microsoft Edge Beta"), "{dirs:?}");
        let _ = std::fs::remove_dir_all(&base);
    }
}
