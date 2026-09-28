//! Small registry helpers (HKCU writes, HKCU/HKLM reads) shared by autostart and file
//! associations.

use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegDeleteTreeW,
    RegGetValueW, RegSetKeyValueW,
};
use windows::core::HSTRING;
use znimok_platform::{PlatformError, Result};

pub(crate) fn not_found(e: WIN32_ERROR) -> bool {
    e == ERROR_FILE_NOT_FOUND
}

pub(crate) fn os(e: WIN32_ERROR) -> PlatformError {
    PlatformError::Os {
        code: e.0 as i64,
        message: windows::core::Error::from(e.to_hresult()).message(),
    }
}

/// A string value of HKCU; `name` "" is the key's default value.
pub(crate) fn read_sz(key: &str, name: &str) -> Result<Option<String>> {
    read_sz_in(HKEY_CURRENT_USER, key, name)
}

pub(crate) fn read_sz_in(root: HKEY, key: &str, name: &str) -> Result<Option<String>> {
    let (key, name) = (HSTRING::from(key), HSTRING::from(name));
    let mut len = 0u32;
    // SAFETY: size query without a buffer.
    let r = unsafe { RegGetValueW(root, &key, &name, RRF_RT_REG_SZ, None, None, Some(&mut len)) };
    if not_found(r) {
        return Ok(None);
    }
    if r != ERROR_SUCCESS {
        return Err(os(r));
    }
    let mut buf = vec![0u16; (len as usize).div_ceil(2) + 1];
    let mut len = (buf.len() * 2) as u32;
    // SAFETY: buffer of `len` bytes.
    let r = unsafe {
        RegGetValueW(
            root,
            &key,
            &name,
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut len),
        )
    };
    if not_found(r) {
        return Ok(None);
    }
    if r != ERROR_SUCCESS {
        return Err(os(r));
    }
    let chars = (len as usize / 2).min(buf.len());
    let s = String::from_utf16_lossy(&buf[..chars]);
    Ok(Some(s.trim_end_matches('\0').to_string()))
}

/// Writes a string value under HKCU, creating the key; `name` "" is the default value.
pub(crate) fn write_sz(key: &str, name: &str, value: &str) -> Result<()> {
    let data: Vec<u16> = value.encode_utf16().chain([0]).collect();
    // SAFETY: `data` is a NUL-terminated UTF-16 string of the given byte size.
    let r = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            &HSTRING::from(key),
            &HSTRING::from(name),
            REG_SZ.0,
            Some(data.as_ptr().cast()),
            (data.len() * 2) as u32,
        )
    };
    if r == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(os(r))
    }
}

pub(crate) fn delete_value(key: &str, name: &str) -> Result<()> {
    // SAFETY: plain call with valid strings.
    let r =
        unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, &HSTRING::from(key), &HSTRING::from(name)) };
    if r == ERROR_SUCCESS || not_found(r) {
        Ok(())
    } else {
        Err(os(r))
    }
}

/// Deletes a HKCU key with everything under it; a missing key is fine.
pub(crate) fn delete_tree(key: &str) -> Result<()> {
    // SAFETY: plain call with a valid string.
    let r = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(key)) };
    if r == ERROR_SUCCESS || not_found(r) {
        Ok(())
    } else {
        Err(os(r))
    }
}
