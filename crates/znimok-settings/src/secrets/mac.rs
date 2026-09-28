//! macOS login Keychain: generic passwords (service + account).

use super::{Result, SecretError};
use objc2_core_foundation::{CFBoolean, CFData, CFDictionary, CFRetained, CFString, CFType};
use objc2_security::{
    SecItemAdd, SecItemCopyMatching, SecItemDelete, errSecItemNotFound, errSecSuccess,
    kSecAttrAccount, kSecAttrService, kSecClass, kSecClassGenericPassword, kSecMatchLimit,
    kSecMatchLimitOne, kSecReturnData, kSecValueData,
};

fn os(status: i32) -> SecretError {
    SecretError::Os(status, format!("OSStatus {status}"))
}

/// class + service + account, plus extra pairs.
fn query(service: &str, name: &str, extra: &[(&CFString, &CFType)]) -> CFRetained<CFDictionary> {
    let service = CFString::from_str(service);
    let account = CFString::from_str(name);
    // SAFETY: the Security constants are immutable CFStrings exported by the framework.
    let (class, generic, k_service, k_account) = unsafe {
        (
            kSecClass,
            kSecClassGenericPassword,
            kSecAttrService,
            kSecAttrAccount,
        )
    };
    let mut keys: Vec<&CFString> = vec![class, k_service, k_account];
    let mut values: Vec<&CFType> = vec![generic.as_ref(), service.as_ref(), account.as_ref()];
    for (k, v) in extra {
        keys.push(k);
        values.push(v);
    }
    let d: CFRetained<CFDictionary<CFString, CFType>> = CFDictionary::from_slices(&keys, &values);
    // SAFETY: CFDictionary<K, V> is the untyped CFDictionary with type information on top.
    unsafe { CFRetained::cast_unchecked(d) }
}

pub fn get(service: &str, name: &str) -> Result<Option<String>> {
    // SAFETY: framework constants.
    let (ret, limit, one) = unsafe { (kSecReturnData, kSecMatchLimit, kSecMatchLimitOne) };
    let q = query(
        service,
        name,
        &[(ret, CFBoolean::new(true).as_ref()), (limit, one.as_ref())],
    );
    let mut out: *const CFType = std::ptr::null();
    // SAFETY: `q` is a valid dictionary; on success `out` is a +1 reference we take over.
    let status = unsafe { SecItemCopyMatching(&q, &mut out) };
    if status == errSecItemNotFound {
        return Ok(None);
    }
    if status != errSecSuccess || out.is_null() {
        return Err(os(status));
    }
    // SAFETY: with kSecReturnData and one match the result is a CFData (+1).
    let data: CFRetained<CFData> =
        unsafe { CFRetained::from_raw(std::ptr::NonNull::new_unchecked(out as *mut CFData)) };
    String::from_utf8(data.to_vec())
        .map(Some)
        .map_err(|_| SecretError::NotText)
}

pub fn set(service: &str, name: &str, value: &str) -> Result<()> {
    delete(service, name)?;
    let data = CFData::from_bytes(value.as_bytes());
    // SAFETY: framework constant.
    let k_value = unsafe { kSecValueData };
    let q = query(service, name, &[(k_value, data.as_ref())]);
    // SAFETY: valid dictionary, no result wanted.
    let status = unsafe { SecItemAdd(&q, std::ptr::null_mut()) };
    if status == errSecSuccess {
        Ok(())
    } else {
        Err(os(status))
    }
}

pub fn delete(service: &str, name: &str) -> Result<bool> {
    let q = query(service, name, &[]);
    // SAFETY: valid dictionary.
    match unsafe { SecItemDelete(&q) } {
        s if s == errSecSuccess => Ok(true),
        s if s == errSecItemNotFound => Ok(false),
        s => Err(os(s)),
    }
}
