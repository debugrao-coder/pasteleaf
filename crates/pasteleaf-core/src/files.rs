use crate::{
    error::{invalid, Error, Result},
    model::{id, valid_name, Attachment, FILE_LIMIT},
};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
/// Reject links/reparse placeholders before reads. We never follow input symlinks or recurse folders.
pub fn ordinary(path: &Path) -> Result<std::fs::Metadata> {
    let m = fs::symlink_metadata(path)?;
    if !m.is_file() || m.file_type().is_symlink() {
        return Err(invalid(
            "Only hydrated ordinary files are supported; no directories or symbolic links",
        ));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        let attributes = m.file_attributes();
        let tag = if attributes & 0x400 != 0 {
            reparse_tag(path)?
        } else {
            0
        };
        validate_windows_attributes(attributes, tag)?;
    }
    #[cfg(target_os = "macos")]
    {
        use std::os::macos::fs::MetadataExt;
        // Darwin SF_DATALESS (sys/stat.h): do not implicitly hydrate clipboard input.
        if m.st_flags() & 0x40000000 != 0 {
            return Err(Error::new(
                "pending_hydration",
                "Download this iCloud file locally before importing it",
            ));
        }
    }
    Ok(m)
}
pub fn safe_dir(path: &Path) -> Result<()> {
    if path.exists() && fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err(invalid("Managed directories cannot be symbolic links"));
    }
    fs::create_dir_all(path)?;
    if !path.is_dir() {
        return Err(invalid("Expected a directory"));
    }
    Ok(())
}
pub fn limited(path: &Path, limit: u64) -> Result<Vec<u8>> {
    if ordinary(path)?.len() > limit {
        return Err(invalid("File exceeds the supported size limit"));
    }
    let mut bytes = Vec::new();
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(invalid("Source changed to a non-file while opening it"));
    }
    file.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(invalid("File changed or exceeds the size limit"));
    }
    Ok(bytes)
}
pub fn size(path: &Path) -> Result<u64> {
    if !path.exists() {
        return Ok(0);
    }
    let m = fs::symlink_metadata(path)?;
    if m.file_type().is_symlink() {
        return Err(invalid("Managed storage contains a symbolic link"));
    }
    if m.is_file() {
        return Ok(m.len());
    }
    let mut n = 0u64;
    for e in fs::read_dir(path)? {
        n = n.saturating_add(size(&e?.path())?);
    }
    Ok(n)
}
/// Complete write + fsync before atomic same-filesystem publication. No visible partial data.
pub fn publish(path: &Path, bytes: &[u8]) -> Result<bool> {
    if path.exists() {
        ordinary(path)?;
        return Ok(false);
    }
    let parent = path
        .parent()
        .ok_or_else(|| invalid("Invalid destination"))?;
    safe_dir(parent)?;
    let temp = parent.join(format!("{}.partial", id()));
    let result = (|| {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            f.set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        if !rename_no_replace(&temp, path)? {
            return Ok(false);
        }
        #[cfg(unix)]
        fs::File::open(parent)?.sync_all()?;
        Ok(true)
    })();
    let _ = fs::remove_file(temp);
    result
}
pub fn blob_path(root: &Path, a: &Attachment) -> PathBuf {
    root.join("attachments").join(&a.hash).join(&a.name)
}
pub fn copy_attachment(
    root: &Path,
    source: &Path,
    limit: u64,
    max_file: u64,
) -> Result<Attachment> {
    let metadata = ordinary(source)?;
    if metadata.len() > FILE_LIMIT.min(max_file) {
        return Err(Error::new(
            "file_limit",
            "Ordinary file exceeds the configured size limit (maximum 100 MiB)",
        ));
    }
    let name = source
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| invalid("File name must be valid UTF-8"))?
        .to_string();
    if !valid_name(&name) {
        return Err(invalid("File name is unsupported or exceeds 240 bytes"));
    }
    let bytes = limited(source, FILE_LIMIT.min(max_file))?;
    let after = ordinary(source)?;
    if after.len() != metadata.len() || after.modified()? != metadata.modified()? {
        return Err(Error::new(
            "file_changed",
            "File changed while being copied; retry capture",
        ));
    }
    let a = Attachment {
        name,
        size: bytes.len() as u64,
        hash: blake3::hash(&bytes).to_hex().to_string(),
    };
    let target = blob_path(root, &a);
    if !target.exists() && size(root)?.saturating_add(a.size) > limit {
        return Err(Error::new(
            "local_quota",
            "Local storage limit reached; history was retained",
        ));
    }
    publish(&target, &bytes)?;
    verify(root, &a)?;
    Ok(a)
}
pub fn verify(root: &Path, a: &Attachment) -> Result<()> {
    a.validate()?;
    let bytes = limited(&blob_path(root, a), FILE_LIMIT)?;
    if bytes.len() as u64 != a.size || blake3::hash(&bytes).to_hex().as_str() != a.hash {
        return Err(Error::new(
            "attachment_corrupt",
            "Attachment checksum mismatch; paste and receipt withheld",
        ));
    }
    Ok(())
}
pub fn available(root: &Path, a: &Attachment) -> bool {
    ordinary(&blob_path(root, a))
        .map(|m| m.len() == a.size)
        .unwrap_or(false)
}

/// Validate dimensions before decode; images are passive data, not a preview URL.
pub fn validate_image(bytes: &[u8]) -> Result<()> {
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()?;
    let (w, h) = reader
        .into_dimensions()
        .map_err(|_| invalid("Unsupported or damaged PNG/JPEG image"))?;
    if w == 0 || h == 0 || w > 16384 || h > 16384 || u64::from(w) * u64::from(h) > 16 * 1024 * 1024
    {
        return Err(invalid(
            "Image exceeds 16 megapixels or 16384 pixels per side",
        ));
    }
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    reader
        .decode()
        .map_err(|_| invalid("Image decoding failed within safe memory limits"))?;
    Ok(())
}

/// Pure classifier kept portable for tests of Windows cloud-file attributes.
#[cfg(any(windows, test))]
pub(crate) fn validate_windows_attributes(attributes: u32, tag: u32) -> Result<()> {
    if attributes & (0x1000 | 0x40000 | 0x400000) != 0 {
        return Err(Error::new(
            "pending_hydration",
            "Download this cloud file locally before importing it",
        ));
    }
    if attributes & 0x400 != 0 {
        // Only the CLOUD and CLOUD_1..F tags are non-name-surrogate file data.
        // Other reparse tags are deliberately not followed.
        if tag & 0x20000000 != 0 || tag & 0xFFFF0FFF != 0x9000001A {
            return Err(invalid(
                "Unsupported reparse point; symbolic links are not clipboard attachments",
            ));
        }
    }
    Ok(())
}
#[cfg(windows)]
fn reparse_tag(path: &Path) -> Result<u32> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        Storage::FileSystem::*,
    };
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: live NUL-terminated path; handle opened to inspect, not traverse the tag.
    unsafe {
        let h = CreateFileW(
            path.as_ptr(),
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
            std::ptr::null_mut(),
        );
        if h == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error().into());
        }
        let mut info = FILE_ATTRIBUTE_TAG_INFO::default();
        let ok = GetFileInformationByHandleEx(
            h,
            FileAttributeTagInfo,
            (&mut info as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
            std::mem::size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
        );
        let error = std::io::Error::last_os_error();
        CloseHandle(h);
        if ok == 0 {
            return Err(error.into());
        }
        Ok(info.ReparseTag)
    }
}
fn rename_no_replace(from: &Path, to: &Path) -> Result<bool> {
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::ffi::OsStrExt;
        let from = std::ffi::CString::new(from.as_os_str().as_bytes())
            .map_err(|_| invalid("Invalid path"))?;
        let to = std::ffi::CString::new(to.as_os_str().as_bytes())
            .map_err(|_| invalid("Invalid path"))?;
        // SAFETY: two live NUL-terminated paths. RENAME_EXCL forbids overwrite atomically.
        let code = unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) };
        if code == 0 {
            return Ok(true);
        }
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            return Ok(false);
        }
        Err(error.into())
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_WRITE_THROUGH};
        let from: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
        let to: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
        // No REPLACE_EXISTING flag: existing immutable objects must never be replaced.
        if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_WRITE_THROUGH) } != 0 {
            return Ok(true);
        }
        let error = std::io::Error::last_os_error();
        if matches!(error.raw_os_error(), Some(80 | 183)) {
            return Ok(false);
        }
        Err(error.into())
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        // Hard-link create is atomic/no-replace. Unsupported filesystems fail closed;
        // a check-then-rename fallback could overwrite another device's library.
        match fs::hard_link(from, to) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
            Err(e) => Err(e.into()),
        }
    }
}
