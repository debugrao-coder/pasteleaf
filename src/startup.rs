//! Current-user logon startup. The registry is the source of truth.
use std::path::Path;

pub fn command(exe: &Path) -> Result<String, String> {
    let path = exe.to_str().ok_or("程序路径无法编码")?;
    let value = format!("\"{path}\" --autostart");
    if !exe.is_absolute() || path.contains(['\0', '"']) || value.encode_utf16().count() > 260 {
        return Err("程序路径不适合开机启动，请移到较短的固定路径后重试".into());
    }
    Ok(value)
}

const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const NAME: &str = "PasteLeaf";

pub fn enabled() -> Result<bool, String> {
    let expected = command(&std::env::current_exe().map_err(|e| e.to_string())?)?;
    Ok(read(RUN)?.is_some_and(|value| value.eq_ignore_ascii_case(&expected)))
}

pub fn set_enabled(enabled: bool) -> Result<(), String> {
    let value = command(&std::env::current_exe().map_err(|e| e.to_string())?)?;
    write(RUN, enabled.then_some(value.as_str()))
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn read(key: &str) -> Result<Option<String>, String> {
    use windows_sys::Win32::{Foundation::*, System::Registry::*};
    let mut buffer = [0u16; 2048];
    let mut size = std::mem::size_of_val(&buffer) as u32;
    // SAFETY: bounded writable UTF-16 buffer and live nul-terminated inputs.
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            wide(key).as_ptr(),
            wide(NAME).as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if result == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if result != ERROR_SUCCESS {
        return Err(format!("无法读取开机启动状态（Windows {result}）"));
    }
    let end = buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len());
    String::from_utf16(&buffer[..end])
        .map(Some)
        .map_err(|_| "启动项内容无效".into())
}

fn write(key: &str, value: Option<&str>) -> Result<(), String> {
    use windows_sys::Win32::{Foundation::*, System::Registry::*};
    let mut handle = std::ptr::null_mut();
    // SAFETY: handle is closed on all paths after a successful open; buffers
    // outlive synchronous calls. Only our named value is modified.
    unsafe {
        let result = if value.is_some() {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                wide(key).as_ptr(),
                0,
                std::ptr::null(),
                0,
                KEY_SET_VALUE,
                std::ptr::null(),
                &mut handle,
                std::ptr::null_mut(),
            )
        } else {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                wide(key).as_ptr(),
                0,
                KEY_SET_VALUE,
                &mut handle,
            )
        };
        if result == ERROR_FILE_NOT_FOUND && value.is_none() {
            return Ok(());
        }
        if result != ERROR_SUCCESS {
            return Err(format!("无法更改开机启动（Windows {result}）"));
        }
        let result = if let Some(value) = value {
            let data = wide(value);
            RegSetValueExW(
                handle,
                wide(NAME).as_ptr(),
                0,
                REG_SZ,
                data.as_ptr().cast(),
                (data.len() * 2) as u32,
            )
        } else {
            RegDeleteValueW(handle, wide(NAME).as_ptr())
        };
        RegCloseKey(handle);
        if result != ERROR_SUCCESS && !(value.is_none() && result == ERROR_FILE_NOT_FOUND) {
            return Err(format!("无法保存开机启动（Windows {result}）"));
        }
    }
    if read(key)?.as_deref() != value {
        return Err("开机启动回读不一致，请重试".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quotes_unicode_paths_with_spaces() {
        assert_eq!(
            command(Path::new(r"C:\应用 程序\PasteLeaf.exe")).unwrap(),
            "\"C:\\应用 程序\\PasteLeaf.exe\" --autostart"
        );
    }
    #[test]
    fn rejects_relative_or_overlong_startup_commands() {
        assert!(command(Path::new("pasteleaf.exe")).is_err());
        assert!(command(Path::new(&format!(
            "C:\\{}\\pasteleaf.exe",
            "x".repeat(260)
        )))
        .is_err());
    }
    #[test]
    fn isolated_registry_roundtrip_and_idempotent_disable() {
        let key = format!(r"Software\PasteLeafTests\{}", std::process::id());
        assert_eq!(read(&key).unwrap(), None);
        write(&key, None).unwrap();
        let value = command(Path::new(r"C:\应用 程序\PasteLeaf.exe")).unwrap();
        write(&key, Some(&value)).unwrap();
        assert_eq!(read(&key).unwrap(), Some(value));
        write(&key, None).unwrap();
        write(&key, None).unwrap();
        assert_eq!(read(&key).unwrap(), None);
        // SAFETY: delete only this test's isolated, empty key, never Run.
        unsafe {
            windows_sys::Win32::System::Registry::RegDeleteKeyW(
                windows_sys::Win32::System::Registry::HKEY_CURRENT_USER,
                wide(&key).as_ptr(),
            );
        }
    }
    #[test]
    fn invalid_registry_location_returns_error() {
        assert!(write(&"x".repeat(300), Some("test")).is_err());
    }
}
