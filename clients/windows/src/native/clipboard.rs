//! Clipboard handles are copied while locked and released before disk I/O.
//! No clipboard data is logged. Only regular existing files are accepted by the core.
use crate::model::{build_cf_html, extract_cf_html, PastePayload};
use base64::Engine as _;
use serde_json::{json, Value};
use std::{path::Path, time::Duration};
use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::*,
        System::{DataExchange::*, Memory::*},
        UI::Shell::*,
    },
};
const TEXT: u32 = 13;
const HDROP: u32 = 15;
const DIB: u32 = 8;
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
struct Clipboard;
impl Clipboard {
    fn open(owner: HWND) -> Result<Self, String> {
        for _ in 0..8 {
            if unsafe { OpenClipboard(owner) }.is_ok() {
                return Ok(Self);
            }
            std::thread::sleep(Duration::from_millis(15));
        }
        Err("剪贴板正被其他应用使用，请重试".into())
    }
}
impl Drop for Clipboard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseClipboard();
        }
    }
}
unsafe fn read_format(format: u32) -> Option<Vec<u8>> {
    let handle = GetClipboardData(format).ok()?;
    let global = HGLOBAL(handle.0);
    let size = GlobalSize(global);
    if size == 0 || size > 100 * 1024 * 1024 {
        return None;
    }
    let p = GlobalLock(global);
    if p.is_null() {
        return None;
    }
    let bytes = std::slice::from_raw_parts(p as *const u8, size).to_vec();
    let _ = GlobalUnlock(global);
    Some(bytes)
}
fn cstring(bytes: Vec<u8>) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}
fn read_text() -> String {
    unsafe { read_format(TEXT) }
        .map(|b| {
            let v: Vec<u16> = b
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .take_while(|&c| c != 0)
                .collect();
            String::from_utf16_lossy(&v)
        })
        .unwrap_or_default()
}
pub fn capture(root: &Path, source: &str, sequence: u32) -> Result<Option<Value>, String> {
    let guard = Clipboard::open(HWND::default())?;
    unsafe {
        if GetClipboardSequenceNumber() != sequence {
            return Ok(None);
        }
        if IsClipboardFormatAvailable(HDROP).is_ok() {
            let handle = GetClipboardData(HDROP).map_err(|_| "无法读取文件列表")?;
            let drop = windows::Win32::UI::Shell::HDROP(handle.0);
            let count = DragQueryFileW(drop, u32::MAX, None);
            if count > 128 {
                return Err("文件数量超过 128，未记录".into());
            }
            let mut files = Vec::new();
            for i in 0..count {
                let len = DragQueryFileW(drop, i, None);
                let mut path = vec![0u16; len as usize + 1];
                DragQueryFileW(drop, i, Some(&mut path));
                files.push(String::from_utf16_lossy(&path[..len as usize]));
            }
            return Ok(Some(
                json!({"op":"capture","kind":"files","paths":files,"source":source}),
            ));
        }
        let png_id = RegisterClipboardFormatW(PCWSTR(wide("PNG").as_ptr()));
        if let Some(png) = read_format(png_id) {
            drop(guard);
            let staging = root.join("capture-staging");
            std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
            let file = staging.join("clipboard.png");
            // Dimensions are checked before decode and memory allocation is bounded.
            let _ = crate::image_io::decode_bounded(&png)?;
            std::fs::write(&file, png).map_err(|e| e.to_string())?;
            return Ok(Some(
                json!({"op":"capture","kind":"image","image_path":file,"source":source}),
            ));
        }
        // CF_DIB is emitted by common screenshot tools even when PNG is absent.
        if let Some(bytes) = read_format(17).or_else(|| read_format(DIB)) {
            if let Ok((w, h, rgba)) = dib_rgba(&bytes) {
                drop(guard);
                let staging = root.join("capture-staging");
                std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
                let file = staging.join("clipboard.png");
                image::save_buffer(&file, &rgba, w, h, image::ColorType::Rgba8)
                    .map_err(|e| e.to_string())?;
                return Ok(Some(
                    json!({"op":"capture","kind":"image","image_path":file,"source":source}),
                ));
            }
        }
        let text = read_text();
        let html_id = RegisterClipboardFormatW(PCWSTR(wide("HTML Format").as_ptr()));
        let rtf_id = RegisterClipboardFormatW(PCWSTR(wide("Rich Text Format").as_ptr()));
        let html = read_format(html_id).and_then(|b| extract_cf_html(&cstring(b)));
        let rtf = read_format(rtf_id).map(|mut b| {
            while b.last() == Some(&0) {
                b.pop();
            }
            base64::engine::general_purpose::STANDARD.encode(b)
        });
        if text.is_empty() && html.is_none() && rtf.is_none() {
            return Ok(None);
        }
        Ok(Some(
            json!({"op":"capture","kind":if html.is_some()||rtf.is_some(){"rich_text"}else{"text"},"text":text,"html":html,"rtf":rtf,"source":source}),
        ))
    }
}
unsafe fn put(format: u32, bytes: &[u8]) -> Result<(), String> {
    let handle = GlobalAlloc(GMEM_MOVEABLE, bytes.len()).map_err(|_| "无法分配剪贴板内存")?;
    let data = GlobalLock(handle);
    if data.is_null() {
        let _ = GlobalFree(handle);
        return Err("无法锁定剪贴板内存".into());
    }
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), data as *mut u8, bytes.len());
    let _ = GlobalUnlock(handle);
    if SetClipboardData(format, HANDLE(handle.0)).is_err() {
        let _ = GlobalFree(handle);
        return Err("无法写入剪贴板".into());
    }
    Ok(())
}
fn utf16_bytes(s: &str) -> Vec<u8> {
    wide(s).iter().flat_map(|v| v.to_le_bytes()).collect()
}
fn bytes0(s: &str) -> Vec<u8> {
    let mut b = s.as_bytes().to_vec();
    b.push(0);
    b
}
pub fn write(payload: PastePayload, window: usize) -> Result<u32, String> {
    // Prepare all content before emptying the user's current clipboard.
    let mut formats: Vec<(u32, Vec<u8>)> = Vec::new();
    unsafe {
        match payload {
            PastePayload::Text(s) => formats.push((TEXT, utf16_bytes(&s))),
            PastePayload::Files(paths) => formats.push((HDROP, file_drop(&paths)?)),
            PastePayload::Item(item) => {
                if !item.available {
                    return Err("附件未完整到达".into());
                }
                if item.kind == "image" {
                    let path = item.image_path.ok_or("图片路径缺失")?;
                    let decoded = crate::image_io::read_bounded(Path::new(&path))?;
                    let im = decoded.to_rgba8();
                    let mut output = std::io::Cursor::new(Vec::new());
                    decoded
                        .write_to(&mut output, image::ImageFormat::Png)
                        .map_err(|_| "无法编码图片")?;
                    formats.push((
                        RegisterClipboardFormatW(PCWSTR(wide("PNG").as_ptr())),
                        output.into_inner(),
                    ));
                    formats.push((DIB, rgba_dib(im.width(), im.height(), im.as_raw())));
                } else if item.kind == "files" {
                    formats.push((
                        HDROP,
                        file_drop(
                            &item
                                .files
                                .iter()
                                .map(|f| f.path.clone())
                                .collect::<Vec<_>>(),
                        )?,
                    ));
                } else {
                    formats.push((TEXT, utf16_bytes(&item.text)));
                    if let Some(html) = item.html {
                        formats.push((
                            RegisterClipboardFormatW(PCWSTR(wide("HTML Format").as_ptr())),
                            bytes0(&build_cf_html(&html)),
                        ));
                    }
                    if let Some(rtf) = item.rtf {
                        formats.push((
                            RegisterClipboardFormatW(PCWSTR(wide("Rich Text Format").as_ptr())),
                            {
                                let mut b = base64::engine::general_purpose::STANDARD
                                    .decode(rtf)
                                    .map_err(|_| "RTF 附件编码无效")?;
                                b.push(0);
                                b
                            },
                        ));
                    }
                }
            }
        }
        let _guard = Clipboard::open(HWND(window as *mut _))?;
        EmptyClipboard().map_err(|_| "无法准备剪贴板")?;
        for (id, bytes) in formats {
            put(id, &bytes)?;
        }
        Ok(GetClipboardSequenceNumber())
    }
}
fn file_drop(paths: &[String]) -> Result<Vec<u8>, String> {
    if paths.is_empty() {
        return Err("文件列表为空".into());
    }
    let mut bytes = vec![0u8; 20];
    bytes[..4].copy_from_slice(&20u32.to_le_bytes());
    bytes[16..20].copy_from_slice(&1u32.to_le_bytes());
    for path in paths {
        let metadata = std::fs::symlink_metadata(path).map_err(|_| "文件附件不可读")?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err("仅支持普通文件附件".into());
        }
        if path.contains('\0') {
            return Err("文件路径无效".into());
        }
        bytes.extend(utf16_bytes(path));
    }
    bytes.extend([0, 0]);
    Ok(bytes)
}
fn dib_rgba(b: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    if b.len() < 40 {
        return Err("DIB 标头缺失".into());
    }
    let u32at = |n| u32::from_le_bytes(b[n..n + 4].try_into().unwrap());
    let mut header = u32at(0) as usize;
    let width = u32at(4) as i32;
    let height = u32at(8) as i32;
    let bits = u16::from_le_bytes([b[14], b[15]]);
    let compression = u32at(16);
    if header < 40
        || header > b.len()
        || width <= 0
        || height == 0
        || height == i32::MIN
        || !matches!(bits, 24 | 32)
        || !matches!(compression, 0 | 3)
    {
        return Err("不支持的 DIB 格式".into());
    }
    if compression == 3 {
        if bits != 32 || b.len() < 52 {
            return Err("不支持的 DIB 位掩码".into());
        }
        if u32at(40) != 0xff0000 || u32at(44) != 0xff00 || u32at(48) != 0xff {
            return Err("不支持的 DIB 色彩掩码".into());
        }
        if header == 40 {
            header = 52;
        }
    }
    let w = width as usize;
    let h = height.unsigned_abs() as usize;
    if w * h > 16 * 1024 * 1024 {
        return Err("图片尺寸超出安全限制".into());
    }
    let stride = (w * bits as usize).div_ceil(32) * 4;
    if header
        .checked_add(stride * h)
        .filter(|n| *n <= b.len())
        .is_none()
    {
        return Err("DIB 数据不完整".into());
    }
    let mut out = vec![255; w * h * 4];
    for y in 0..h {
        let sy = if height > 0 { h - 1 - y } else { y };
        for x in 0..w {
            let p = header + sy * stride + x * (bits as usize / 8);
            let q = (y * w + x) * 4;
            out[q] = b[p + 2];
            out[q + 1] = b[p + 1];
            out[q + 2] = b[p];
        }
    }
    Ok((w as u32, h as u32, out))
}
fn rgba_dib(w: u32, h: u32, pixels: &[u8]) -> Vec<u8> {
    let mut b = vec![0; 40];
    b[..4].copy_from_slice(&40u32.to_le_bytes());
    b[4..8].copy_from_slice(&w.to_le_bytes());
    b[8..12].copy_from_slice(&(-(h as i32)).to_le_bytes());
    b[12..14].copy_from_slice(&1u16.to_le_bytes());
    b[14..16].copy_from_slice(&32u16.to_le_bytes());
    for p in pixels.as_chunks::<4>().0 {
        b.extend([p[2], p[1], p[0], 0]);
    }
    b
}
