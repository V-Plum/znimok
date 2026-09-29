//! ECDSA P-256 over SHA-256: the release key (PEM, SubjectPublicKeyInfo) and the signature as
//! `openssl dgst -sha256 -sign` writes it (DER). Parsing is ours and strict; the verification
//! itself is the OS's (BCrypt on Windows, Security.framework on macOS) — no hand-written curve
//! arithmetic.

use base64::Engine;

/// The fixed DER prefix of a P-256 SubjectPublicKeyInfo (id-ecPublicKey, prime256v1), followed by
/// the uncompressed point `04 ‖ X ‖ Y`.
const SPKI_P256_PREFIX: [u8; 26] = [
    0x30, 0x59, 0x30, 0x13, 0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01, 0x06, 0x08, 0x2a,
    0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07, 0x03, 0x42, 0x00,
];

/// The public point `04 ‖ X ‖ Y` (65 bytes) of a PEM P-256 public key.
pub fn public_point(pem: &str) -> Option<[u8; 65]> {
    let body: String = pem
        .lines()
        .map(str::trim)
        .skip_while(|l| *l != "-----BEGIN PUBLIC KEY-----")
        .skip(1)
        .take_while(|l| *l != "-----END PUBLIC KEY-----")
        .collect();
    let der = base64::engine::general_purpose::STANDARD
        .decode(body)
        .ok()?;
    if der.len() != 91 || der[..26] != SPKI_P256_PREFIX || der[26] != 0x04 {
        return None;
    }
    der[26..].try_into().ok()
}

/// `r ‖ s` (64 bytes) of a DER `SEQUENCE { INTEGER r, INTEGER s }`; strict: exact lengths, no
/// trailing bytes, values that fit 32 bytes.
pub fn raw_signature(der: &[u8]) -> Option<[u8; 64]> {
    let (&tag, rest) = der.split_first()?;
    let (&len, body) = rest.split_first()?;
    if tag != 0x30 || len as usize != body.len() || len >= 0x80 {
        return None;
    }
    let mut out = [0u8; 64];
    let mut p = body;
    for half in 0..2 {
        let (&t, rest) = p.split_first()?;
        let (&n, rest) = rest.split_first()?;
        let n = n as usize;
        if t != 0x02 || n == 0 || n > 33 || rest.len() < n {
            return None;
        }
        let (int, rest) = rest.split_at(n);
        // DER INTEGER: a set top bit means negative (never valid here); a leading zero only when
        // the next byte has its top bit set (minimal encoding) — then it is dropped.
        if int[0] & 0x80 != 0 {
            return None;
        }
        let int = match int {
            [0, next, ..] if next & 0x80 != 0 => &int[1..],
            [0, _, ..] => return None,
            _ => int,
        };
        if int.len() > 32 {
            return None;
        }
        out[half * 32 + 32 - int.len()..half * 32 + 32].copy_from_slice(int);
        p = rest;
    }
    p.is_empty().then_some(out)
}

/// Whether `sig` (DER) is `key`'s signature of `msg`.
pub fn verify(key_pem: &str, msg: &[u8], sig_der: &[u8]) -> Result<bool, String> {
    let point = public_point(key_pem).ok_or("the release key is not a P-256 public key")?;
    let Some(raw) = raw_signature(sig_der) else {
        return Ok(false);
    };
    let digest = crate::sha256::digest(msg);
    os::verify(&point, &digest, &raw, sig_der)
}

#[cfg(windows)]
mod os {
    use windows::Win32::Foundation::STATUS_INVALID_SIGNATURE;
    use windows::Win32::Security::Cryptography::{
        BCRYPT_ALG_HANDLE, BCRYPT_ECCKEY_BLOB, BCRYPT_ECCPUBLIC_BLOB, BCRYPT_ECDSA_P256_ALGORITHM,
        BCRYPT_ECDSA_PUBLIC_P256_MAGIC, BCRYPT_KEY_HANDLE, BCRYPT_OPEN_ALGORITHM_PROVIDER_FLAGS,
        BCryptCloseAlgorithmProvider, BCryptDestroyKey, BCryptImportKeyPair,
        BCryptOpenAlgorithmProvider, BCryptVerifySignature,
    };

    pub fn verify(
        point: &[u8; 65],
        digest: &[u8; 32],
        raw: &[u8; 64],
        _der: &[u8],
    ) -> Result<bool, String> {
        let mut blob = Vec::with_capacity(8 + 64);
        let head = BCRYPT_ECCKEY_BLOB {
            dwMagic: BCRYPT_ECDSA_PUBLIC_P256_MAGIC,
            cbKey: 32,
        };
        blob.extend_from_slice(&head.dwMagic.to_le_bytes());
        blob.extend_from_slice(&head.cbKey.to_le_bytes());
        blob.extend_from_slice(&point[1..]);
        let mut alg = BCRYPT_ALG_HANDLE::default();
        // SAFETY: out-pointer to a handle; the algorithm id is a static wide string.
        let st = unsafe {
            BCryptOpenAlgorithmProvider(
                &mut alg,
                BCRYPT_ECDSA_P256_ALGORITHM,
                None,
                BCRYPT_OPEN_ALGORITHM_PROVIDER_FLAGS(0),
            )
        };
        st.ok().map_err(|e| format!("BCrypt: {e}"))?;
        let mut key = BCRYPT_KEY_HANDLE::default();
        // SAFETY: a public-key blob of the documented layout (header, X, Y).
        let st =
            unsafe { BCryptImportKeyPair(alg, None, BCRYPT_ECCPUBLIC_BLOB, &mut key, &blob, 0) };
        let result = st.ok().map_err(|e| format!("BCrypt key: {e}")).map(|()| {
            // SAFETY: valid key handle, 32-byte digest, 64-byte r‖s signature.
            let st = unsafe { BCryptVerifySignature(key, None, digest, raw, Default::default()) };
            if st.is_ok() {
                Ok(true)
            } else if st == STATUS_INVALID_SIGNATURE {
                Ok(false)
            } else {
                Err(format!("BCrypt verify: {:#x}", st.0))
            }
        });
        // SAFETY: handles we opened.
        unsafe {
            if !key.is_invalid() {
                let _ = BCryptDestroyKey(key);
            }
            let _ = BCryptCloseAlgorithmProvider(alg, 0);
        }
        result?
    }
}

#[cfg(target_os = "macos")]
mod os {
    //! Security.framework: SecKeyCreateWithData + SecKeyVerifySignature (digest, X9.62 DER).
    use std::ffi::c_void;

    type CFTypeRef = *const c_void;
    type CFIndex = isize;

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFDataCreate(alloc: CFTypeRef, bytes: *const u8, len: CFIndex) -> CFTypeRef;
        fn CFDictionaryCreate(
            alloc: CFTypeRef,
            keys: *const CFTypeRef,
            values: *const CFTypeRef,
            n: CFIndex,
            key_cb: *const c_void,
            value_cb: *const c_void,
        ) -> CFTypeRef;
        fn CFRelease(cf: CFTypeRef);
        static kCFTypeDictionaryKeyCallBacks: c_void;
        static kCFTypeDictionaryValueCallBacks: c_void;
    }

    #[link(name = "Security", kind = "framework")]
    unsafe extern "C" {
        fn SecKeyCreateWithData(
            data: CFTypeRef,
            attrs: CFTypeRef,
            err: *mut CFTypeRef,
        ) -> CFTypeRef;
        fn SecKeyVerifySignature(
            key: CFTypeRef,
            alg: CFTypeRef,
            signed: CFTypeRef,
            sig: CFTypeRef,
            err: *mut CFTypeRef,
        ) -> u8;
        static kSecAttrKeyType: CFTypeRef;
        static kSecAttrKeyTypeECSECPrimeRandom: CFTypeRef;
        static kSecAttrKeyClass: CFTypeRef;
        static kSecAttrKeyClassPublic: CFTypeRef;
        static kSecKeyAlgorithmECDSASignatureDigestX962SHA256: CFTypeRef;
    }

    pub fn verify(
        point: &[u8; 65],
        digest: &[u8; 32],
        _raw: &[u8; 64],
        der: &[u8],
    ) -> Result<bool, String> {
        // SAFETY: CoreFoundation objects created and released here; the constants are the
        // framework's own.
        unsafe {
            let keys = [kSecAttrKeyType, kSecAttrKeyClass];
            let values = [kSecAttrKeyTypeECSECPrimeRandom, kSecAttrKeyClassPublic];
            let attrs = CFDictionaryCreate(
                std::ptr::null(),
                keys.as_ptr(),
                values.as_ptr(),
                2,
                &kCFTypeDictionaryKeyCallBacks,
                &kCFTypeDictionaryValueCallBacks,
            );
            let data = CFDataCreate(std::ptr::null(), point.as_ptr(), 65);
            let mut err: CFTypeRef = std::ptr::null();
            let key = SecKeyCreateWithData(data, attrs, &mut err);
            CFRelease(data);
            CFRelease(attrs);
            if key.is_null() {
                if !err.is_null() {
                    CFRelease(err);
                }
                return Err("Security.framework: cannot import the release key".into());
            }
            let signed = CFDataCreate(std::ptr::null(), digest.as_ptr(), 32);
            let sig = CFDataCreate(std::ptr::null(), der.as_ptr(), der.len() as CFIndex);
            let mut err: CFTypeRef = std::ptr::null();
            let ok = SecKeyVerifySignature(
                key,
                kSecKeyAlgorithmECDSASignatureDigestX962SHA256,
                signed,
                sig,
                &mut err,
            );
            if !err.is_null() {
                CFRelease(err);
            }
            CFRelease(signed);
            CFRelease(sig);
            CFRelease(key);
            Ok(ok != 0)
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod os {
    pub fn verify(_: &[u8; 65], _: &[u8; 32], _: &[u8; 64], _: &[u8]) -> Result<bool, String> {
        Err("signature checks need Windows or macOS".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = include_str!("../tests/fixtures/test-key.pub");
    const SUMS: &[u8] = include_bytes!("../tests/fixtures/SHA256SUMS");
    const SIG: &[u8] = include_bytes!("../tests/fixtures/SHA256SUMS.sig");

    #[test]
    fn key_and_signature_parse() {
        let p = public_point(KEY).unwrap();
        assert_eq!(p[0], 4);
        assert!(raw_signature(SIG).is_some());
        assert!(
            public_point("-----BEGIN PUBLIC KEY-----\nAAAA\n-----END PUBLIC KEY-----").is_none()
        );
    }

    /// Half of real signatures have an integer with its top bit set, written with a leading zero;
    /// some have a short one. Both must parse (the first fixture happened to have neither).
    #[test]
    fn der_integers_with_leading_zero_and_short() {
        let mut der = vec![0x30, 0x44, 0x02, 0x21, 0x00, 0xdf];
        der.extend([0x11; 31]);
        der.extend([0x02, 0x1f, 0x7f]);
        der.extend([0x22; 30]);
        let raw = raw_signature(&der).unwrap();
        assert_eq!(raw[0], 0xdf);
        assert_eq!(&raw[1..32], &[0x11; 31]);
        assert_eq!(raw[32], 0, "a 31-byte s is left-padded");
        assert_eq!(raw[33], 0x7f);
    }

    #[test]
    fn hostile_signatures_are_refused_without_panic() {
        let mut cases: Vec<Vec<u8>> = vec![
            vec![],
            vec![0x30],
            vec![0x30, 0x00],
            SIG[..SIG.len() - 1].to_vec(),
        ];
        let mut trailing = SIG.to_vec();
        trailing.push(0);
        cases.push(trailing);
        let mut wrong_len = SIG.to_vec();
        wrong_len[1] = wrong_len[1].wrapping_add(1);
        cases.push(wrong_len);
        cases.push(vec![0x30, 0x06, 0x02, 0x01, 0x80, 0x02, 0x01, 0x01]); // negative r
        cases.push(vec![0x30, 0x07, 0x02, 0x02, 0x00, 0x01, 0x02, 0x01, 0x01]); // non-minimal r
        for c in cases {
            assert!(raw_signature(&c).is_none(), "{c:02x?}");
        }
    }

    /// openssl's signature verifies through the OS; a changed byte of the message or of the
    /// signature does not.
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn os_verification() {
        assert_eq!(verify(KEY, SUMS, SIG), Ok(true));
        let mut msg = SUMS.to_vec();
        msg[0] ^= 1;
        assert_eq!(verify(KEY, &msg, SIG), Ok(false));
        let mut sig = SIG.to_vec();
        let last = sig.len() - 1;
        sig[last] ^= 1;
        assert_eq!(verify(KEY, SUMS, &sig), Ok(false));
    }
}
