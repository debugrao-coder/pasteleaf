//! Store only derived 32-byte library keys. Passwords never enter SQLite or files.
#[cfg(windows)]
use crate::files;
use crate::{
    crypto::Key,
    error::{Error, Result},
};
use std::path::Path;
#[cfg(target_os = "macos")]
const SERVICE: &str = "io.pasteleaf.sync.v2";
#[cfg(target_os = "macos")]
fn platform_save(_root: &Path, account: &str, key: &Key) -> Result<()> {
    security_framework::passwords::set_generic_password(SERVICE, account, &key[..])
        .map_err(|_| Error::new("keychain", "Could not save sync key to macOS Keychain"))
}
#[cfg(target_os = "macos")]
fn platform_load(_root: &Path, account: &str) -> Result<Key> {
    let b = zeroize::Zeroizing::new(
        security_framework::passwords::get_generic_password(SERVICE, account).map_err(|_| {
            Error::new(
                "key_locked",
                "Unlock sync; no accessible remembered Keychain key",
            )
        })?,
    );
    Ok(zeroize::Zeroizing::new(b.as_slice().try_into().map_err(
        |_| Error::new("keychain", "Stored sync key has invalid length"),
    )?))
}
#[cfg(target_os = "macos")]
fn platform_forget(_root: &Path, account: &str) -> Result<()> {
    match security_framework::passwords::delete_generic_password(SERVICE, account) {
        Ok(()) => Ok(()),
        Err(e) if e.code() == -25300 => Ok(()),
        Err(_) => Err(Error::new(
            "keychain",
            "Could not remove remembered Keychain key",
        )),
    }
}
#[cfg(windows)]
mod dpapi {
    use super::*;
    #[repr(C)]
    struct Blob {
        len: u32,
        data: *mut u8,
    }
    #[link(name = "crypt32")]
    unsafe extern "system" {
        fn CryptProtectData(
            input: *const Blob,
            description: *const u16,
            entropy: *const Blob,
            reserved: *mut std::ffi::c_void,
            prompt: *const std::ffi::c_void,
            flags: u32,
            output: *mut Blob,
        ) -> i32;
        fn CryptUnprotectData(
            input: *const Blob,
            description: *mut *mut u16,
            entropy: *const Blob,
            reserved: *mut std::ffi::c_void,
            prompt: *const std::ffi::c_void,
            flags: u32,
            output: *mut Blob,
        ) -> i32;
    }
    pub fn transform(bytes: &[u8], encrypt: bool) -> Result<Vec<u8>> {
        use std::ptr::{null, null_mut};
        let input = Blob {
            len: bytes
                .len()
                .try_into()
                .map_err(|_| Error::new("dpapi", "Invalid key size"))?,
            data: bytes.as_ptr() as *mut u8,
        };
        let mut output = Blob {
            len: 0,
            data: null_mut(),
        };
        // SAFETY: live bounded input; DPAPI allocates output with LocalAlloc.
        unsafe {
            let ok = if encrypt {
                CryptProtectData(&input, null(), null(), null_mut(), null(), 1, &mut output)
            } else {
                CryptUnprotectData(
                    &input,
                    null_mut(),
                    null(),
                    null_mut(),
                    null(),
                    1,
                    &mut output,
                )
            };
            if ok == 0 {
                return Err(Error::new(
                    "dpapi",
                    "Windows user key protection failed; unlock sync again",
                ));
            }
            let data = std::slice::from_raw_parts(output.data, output.len as usize).to_vec();
            std::ptr::write_bytes(output.data, 0, output.len as usize);
            windows_sys::Win32::Foundation::LocalFree(output.data.cast());
            Ok(data)
        }
    }
}
#[cfg(windows)]
fn platform_save(root: &Path, account: &str, key: &Key) -> Result<()> {
    let path = root.join(format!("key-{account}.dpapi"));
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    files::publish(&path, &dpapi::transform(&key[..], true)?)?;
    Ok(())
}
#[cfg(windows)]
fn platform_load(root: &Path, account: &str) -> Result<Key> {
    let b = zeroize::Zeroizing::new(dpapi::transform(
        &files::limited(&root.join(format!("key-{account}.dpapi")), 8192)?,
        false,
    )?);
    Ok(zeroize::Zeroizing::new(b.as_slice().try_into().map_err(
        |_| Error::new("dpapi", "Invalid remembered key"),
    )?))
}
#[cfg(windows)]
fn platform_forget(root: &Path, account: &str) -> Result<()> {
    let p = root.join(format!("key-{account}.dpapi"));
    if p.exists() {
        std::fs::remove_file(p)?;
    }
    Ok(())
}
#[cfg(not(any(windows, target_os = "macos")))]
fn platform_save(_root: &Path, _account: &str, _key: &Key) -> Result<()> {
    Err(Error::new(
        "unsupported_vault",
        "Remembering sync keys is supported only on macOS and Windows",
    ))
}
#[cfg(not(any(windows, target_os = "macos")))]
fn platform_load(_root: &Path, _account: &str) -> Result<Key> {
    Err(Error::new("key_locked", "Unlock sync with its password"))
}
#[cfg(not(any(windows, target_os = "macos")))]
fn platform_forget(_root: &Path, _account: &str) -> Result<()> {
    Ok(())
}

fn disabled() -> bool {
    std::env::var_os("PASTELEAF_DISABLE_KEYCHAIN").is_some()
}
pub fn save(root: &Path, account: &str, key: &Key) -> Result<()> {
    if disabled() {
        return Err(Error::new(
            "key_storage_disabled",
            "Remembering keys is disabled in this isolated session",
        ));
    }
    platform_save(root, account, key)
}
pub fn load(root: &Path, account: &str) -> Result<Key> {
    if disabled() {
        return Err(Error::new(
            "key_locked",
            "Key storage is disabled; unlock with the test library password",
        ));
    }
    platform_load(root, account)
}
pub fn forget(root: &Path, account: &str) -> Result<()> {
    // Isolated test mode must not delete a real Keychain item from copied metadata.
    if disabled() {
        return Ok(());
    }
    platform_forget(root, account)
}
