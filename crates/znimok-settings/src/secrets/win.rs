//! Windows Credential Manager: generic credentials, persisted on this machine only.

use super::{Result, SecretError};
use windows::Win32::Foundation::ERROR_NOT_FOUND;
use windows::Win32::Security::Credentials::{
    CRED_MAX_CREDENTIAL_BLOB_SIZE, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW,
    CredDeleteW, CredFree, CredReadW, CredWriteW,
};
use windows::core::{HSTRING, PWSTR};

fn target(service: &str, name: &str) -> HSTRING {
    HSTRING::from(format!("{service}:{name}"))
}

fn os(e: windows::core::Error) -> SecretError {
    SecretError::Os(e.code().0, e.message())
}

fn not_found(e: &windows::core::Error) -> bool {
    e.code() == ERROR_NOT_FOUND.to_hresult()
}

pub fn get(service: &str, name: &str) -> Result<Option<String>> {
    let mut p: *mut CREDENTIALW = std::ptr::null_mut();
    match unsafe { CredReadW(&target(service, name), CRED_TYPE_GENERIC, None, &mut p) } {
        Err(e) if not_found(&e) => return Ok(None),
        Err(e) => return Err(os(e)),
        Ok(()) => {}
    }
    // SAFETY: CredReadW succeeded, so `p` points to a credential we own until CredFree.
    let bytes = unsafe {
        let c = &*p;
        let v = if c.CredentialBlob.is_null() {
            Vec::new()
        } else {
            std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize).to_vec()
        };
        CredFree(p as *const _);
        v
    };
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| SecretError::NotText)
}

pub fn set(service: &str, name: &str, value: &str) -> Result<()> {
    if value.len() > CRED_MAX_CREDENTIAL_BLOB_SIZE as usize {
        return Err(SecretError::TooLong);
    }
    let mut target: Vec<u16> = format!("{service}:{name}")
        .encode_utf16()
        .chain([0])
        .collect();
    let mut user: Vec<u16> = name.encode_utf16().chain([0]).collect();
    let mut blob = value.as_bytes().to_vec();
    let cred = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: PWSTR(target.as_mut_ptr()),
        UserName: PWSTR(user.as_mut_ptr()),
        CredentialBlobSize: blob.len() as u32,
        CredentialBlob: blob.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        ..Default::default()
    };
    // SAFETY: every pointer in `cred` lives until the call returns; CredWriteW copies them.
    let r = unsafe { CredWriteW(&cred, 0) };
    blob.fill(0);
    r.map_err(os)
}

pub fn delete(service: &str, name: &str) -> Result<bool> {
    match unsafe { CredDeleteW(&target(service, name), CRED_TYPE_GENERIC, None) } {
        Ok(()) => Ok(true),
        Err(e) if not_found(&e) => Ok(false),
        Err(e) => Err(os(e)),
    }
}
