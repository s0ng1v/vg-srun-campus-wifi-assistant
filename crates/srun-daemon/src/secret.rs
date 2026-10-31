use anyhow::{Context, Result};

#[cfg(windows)]
use std::{ptr, slice};

const ENTROPY: &[u8] = b"VGCampusWifiAssistant/credentials/v1";

#[cfg(windows)]
pub fn protect_local_machine(plaintext: &[u8]) -> Result<Vec<u8>> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CRYPTPROTECT_LOCAL_MACHINE, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    let input = CRYPT_INTEGER_BLOB {
        cbData: plaintext.len().try_into().context("凭据数据过大")?,
        pbData: plaintext.as_ptr().cast_mut(),
    };
    let entropy = CRYPT_INTEGER_BLOB {
        cbData: ENTROPY.len().try_into().context("DPAPI entropy 过大")?,
        pbData: ENTROPY.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();

    let succeeded = unsafe {
        CryptProtectData(
            &input,
            ptr::null(),
            &entropy,
            ptr::null(),
            ptr::null(),
            CRYPTPROTECT_LOCAL_MACHINE | CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if succeeded == 0 {
        return Err(std::io::Error::last_os_error()).context("DPAPI 加密凭据失败");
    }

    let encrypted =
        unsafe { slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
    unsafe {
        LocalFree(output.pbData.cast());
    }
    Ok(encrypted)
}

#[cfg(windows)]
pub fn unprotect_local_machine(encrypted: &[u8]) -> Result<Vec<u8>> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    let input = CRYPT_INTEGER_BLOB {
        cbData: encrypted.len().try_into().context("凭据文件过大")?,
        pbData: encrypted.as_ptr().cast_mut(),
    };
    let entropy = CRYPT_INTEGER_BLOB {
        cbData: ENTROPY.len().try_into().context("DPAPI entropy 过大")?,
        pbData: ENTROPY.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();

    let succeeded = unsafe {
        CryptUnprotectData(
            &input,
            ptr::null_mut(),
            &entropy,
            ptr::null(),
            ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if succeeded == 0 {
        return Err(std::io::Error::last_os_error()).context("DPAPI 解密凭据失败");
    }

    let plaintext =
        unsafe { slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
    unsafe {
        LocalFree(output.pbData.cast());
    }
    Ok(plaintext)
}

#[cfg(not(windows))]
pub fn protect_local_machine(_plaintext: &[u8]) -> Result<Vec<u8>> {
    anyhow::bail!("本项目的凭据加密只支持 Windows")
}

#[cfg(not(windows))]
pub fn unprotect_local_machine(_encrypted: &[u8]) -> Result<Vec<u8>> {
    anyhow::bail!("本项目的凭据加密只支持 Windows")
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn local_machine_dpapi_round_trip() {
        let plaintext = br#"{"account":"fixture","password":"not-a-real-secret"}"#;
        let encrypted = protect_local_machine(plaintext).unwrap();
        assert_ne!(encrypted, plaintext);
        assert_eq!(unprotect_local_machine(&encrypted).unwrap(), plaintext);
    }
}
