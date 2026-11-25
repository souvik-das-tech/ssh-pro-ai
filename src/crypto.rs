//! Secrets at rest are encrypted with Windows DPAPI (bound to the current Windows user), so the
//! database file is useless if copied to another account or machine. No master password needed.
//! Same format as the earlier Python version ("dpapi:" + base64), so its saved secrets still open.

use base64::Engine;

const PREFIX: &str = "dpapi:";

pub fn protect(value: Option<&str>) -> anyhow::Result<Option<String>> {
    match value {
        None | Some("") => Ok(None),
        Some(v) => {
            let blob = imp::protect(v.as_bytes())?;
            Ok(Some(format!("{PREFIX}{}", base64::engine::general_purpose::STANDARD.encode(blob))))
        }
    }
}

pub fn unprotect(value: Option<&str>) -> anyhow::Result<Option<String>> {
    let Some(value) = value.filter(|v| !v.is_empty()) else {
        return Ok(None);
    };
    let Some(encoded) = value.strip_prefix(PREFIX) else {
        anyhow::bail!("secret is not DPAPI-protected");
    };
    let blob = base64::engine::general_purpose::STANDARD.decode(encoded)?;
    let plain = imp::unprotect(&blob)?;
    Ok(Some(String::from_utf8_lossy(&plain).into_owned()))
}

#[cfg(windows)]
mod imp {
    use std::ptr::{null, null_mut};

    use windows_sys::Win32::Foundation::{LocalFree, HLOCAL};
    use windows_sys::Win32::Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB};

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    fn take(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
        let data = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize) }.to_vec();
        unsafe { LocalFree(out.pbData as HLOCAL) };
        data
    }

    pub fn protect(data: &[u8]) -> anyhow::Result<Vec<u8>> {
        let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
        let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: null_mut() };
        let description = wide("SSH-Pro-AI");
        let ok = unsafe { CryptProtectData(&input, description.as_ptr(), null(), null(), null(), 0, &mut out) };
        if ok == 0 {
            anyhow::bail!("DPAPI encrypt failed: {}", std::io::Error::last_os_error());
        }
        Ok(take(out))
    }

    pub fn unprotect(data: &[u8]) -> anyhow::Result<Vec<u8>> {
        let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
        let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: null_mut() };
        let ok = unsafe { CryptUnprotectData(&input, null_mut(), null(), null(), null(), 0, &mut out) };
        if ok == 0 {
            anyhow::bail!("DPAPI decrypt failed (secret saved by another Windows user?): {}",
                          std::io::Error::last_os_error());
        }
        Ok(take(out))
    }
}

#[cfg(not(windows))]
mod imp {
    // Development fallback only: the desktop app targets Windows.
    pub fn protect(data: &[u8]) -> anyhow::Result<Vec<u8>> {
        Ok(data.to_vec())
    }
    pub fn unprotect(data: &[u8]) -> anyhow::Result<Vec<u8>> {
        Ok(data.to_vec())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn round_trip() {
        let sealed = super::protect(Some("hunter2")).unwrap().unwrap();
        assert!(sealed.starts_with("dpapi:") && !sealed.contains("hunter2"));
        assert_eq!(super::unprotect(Some(&sealed)).unwrap().as_deref(), Some("hunter2"));
        assert_eq!(super::protect(Some("")).unwrap(), None);
        assert!(super::unprotect(Some("plain")).is_err());
    }
}
