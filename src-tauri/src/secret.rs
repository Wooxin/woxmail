use base64::Engine;

pub fn protect_password(password: &str) -> Result<String, String> {
    protect_bytes(password.as_bytes())
}

pub fn unprotect_password(value: &str) -> Result<String, String> {
    let bytes = unprotect_bytes(value)?;
    String::from_utf8(bytes).map_err(|e| e.to_string())
}

#[cfg(windows)]
fn protect_bytes(bytes: &[u8]) -> Result<String, String> {
    use std::{ptr, slice};
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB},
    };

    let mut input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: ptr::null_mut(),
    };

    let ok = unsafe {
        CryptProtectData(
            &mut input,
            ptr::null(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };

    if ok == 0 {
        return Err("Windows DPAPI 加密失败".to_string());
    }

    let encrypted = unsafe { slice::from_raw_parts(output.pbData, output.cbData as usize) };
    let encoded = base64::engine::general_purpose::STANDARD.encode(encrypted);
    unsafe {
        LocalFree(output.pbData as _);
    }
    Ok(encoded)
}

#[cfg(windows)]
fn unprotect_bytes(value: &str) -> Result<Vec<u8>, String> {
    use std::{ptr, slice};
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{
            CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
        },
    };

    let mut encrypted = base64::engine::general_purpose::STANDARD
        .decode(value)
        .map_err(|e| e.to_string())?;
    let mut input = CRYPT_INTEGER_BLOB {
        cbData: encrypted.len() as u32,
        pbData: encrypted.as_mut_ptr(),
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: ptr::null_mut(),
    };

    let ok = unsafe {
        CryptUnprotectData(
            &mut input,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };

    if ok == 0 {
        return Err("Windows DPAPI 解密失败".to_string());
    }

    let decrypted =
        unsafe { slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
    unsafe {
        LocalFree(output.pbData as _);
    }
    Ok(decrypted)
}

#[cfg(not(windows))]
fn protect_bytes(bytes: &[u8]) -> Result<String, String> {
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
}

#[cfg(not(windows))]
fn unprotect_bytes(value: &str) -> Result<Vec<u8>, String> {
    base64::engine::general_purpose::STANDARD
        .decode(value)
        .map_err(|e| e.to_string())
}
