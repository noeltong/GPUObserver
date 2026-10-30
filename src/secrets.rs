//! 凭据保护：Windows DPAPI（CurrentUser），熵与 C# 版 SecretProtector 一致，
//! 因此 Rust 版可以直接解密 C# 版保存的密码/私钥口令，反之亦然。

use base64::Engine;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HLOCAL, LocalFree};
use windows::Win32::Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB};

const ENTROPY: &[u8] = b"GPUObserver.Secret.v1";

/// 加密明文并 Base64 编码；空输入返回 None（与 C# Protect 一致）。
pub fn protect(plaintext: &str) -> Option<String> {
    if plaintext.is_empty() {
        return None;
    }

    unsafe {
        let bytes = plaintext.as_bytes();
        let mut input = CRYPT_INTEGER_BLOB {
            cbData: bytes.len() as u32,
            pbData: bytes.as_ptr() as *mut u8,
        };
        let mut entropy = CRYPT_INTEGER_BLOB {
            cbData: ENTROPY.len() as u32,
            pbData: ENTROPY.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB::default();

        let result = CryptProtectData(
            &mut input,
            PCWSTR::null(),
            Some(&mut entropy),
            None,
            None,
            0,
            &mut output,
        );

        if result.is_err() {
            return None;
        }

        let encoded = std::slice::from_raw_parts(output.pbData, output.cbData as usize);
        let protected = base64::engine::general_purpose::STANDARD.encode(encoded);
        LocalFree(Some(HLOCAL(output.pbData as *mut core::ffi::c_void)));
        Some(protected)
    }
}

pub fn unprotect(protected_base64: &str) -> Result<String, ()> {
    if protected_base64.is_empty() {
        return Ok(String::new());
    }

    let Ok(protected_bytes) = base64::engine::general_purpose::STANDARD.decode(protected_base64) else {
        // 对应 C# 的 FormatException / CryptographicException。
        return Err(());
    };

    unsafe {
        let mut input = CRYPT_INTEGER_BLOB {
            cbData: protected_bytes.len() as u32,
            pbData: protected_bytes.as_ptr() as *mut u8,
        };
        let mut entropy = CRYPT_INTEGER_BLOB {
            cbData: ENTROPY.len() as u32,
            pbData: ENTROPY.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB::default();

        let result = CryptUnprotectData(
            &mut input,
            None,
            Some(&mut entropy),
            None,
            None,
            0,
            &mut output,
        );

        if result.is_err() {
            return Err(());
        }

        let decoded = std::slice::from_raw_parts(output.pbData, output.cbData as usize);
        let plaintext = String::from_utf8_lossy(decoded).into_owned();
        LocalFree(Some(HLOCAL(output.pbData as *mut core::ffi::c_void)));
        Ok(plaintext)
    }
}

pub fn try_unprotect(protected_base64: Option<&str>) -> Option<String> {
    unprotect(protected_base64.unwrap_or("")).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_rejects_empty() {
        let secret = "correct horse battery staple 密码";
        let protected = protect(secret).expect("protect should succeed");

        assert_ne!(protected, secret);
        assert_eq!(unprotect(&protected).ok().as_deref(), Some(secret));
        assert_eq!(protect(""), None);
    }

    #[test]
    fn tampered_value_fails_gracefully() {
        let protected = protect("secret").expect("protect should succeed");
        let tail = &protected[protected.len() - 2..];
        let tampered_tail = if tail.starts_with('A') { "BB" } else { "AA" };
        let tampered = format!("{}{}", &protected[..protected.len() - 2], tampered_tail);

        assert!(try_unprotect(Some(&tampered)).is_none());
    }
}
