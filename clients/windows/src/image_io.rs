//! Shared bounded PNG/JPEG decoding for capture, preview and paste.
use std::{
    io::{Cursor, Read},
    path::Path,
};
const MAX_BYTES: u64 = 100 * 1024 * 1024;
const MAX_PIXELS: u64 = 16 * 1024 * 1024;
pub fn read_bounded(path: &Path) -> Result<image::DynamicImage, String> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| "图片附件不可读")?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > MAX_BYTES {
        return Err("图片附件类型或大小无效".into());
    }
    let file = std::fs::File::open(path).map_err(|_| "图片附件不可读")?;
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "图片附件读取失败")?;
    decode_bounded(&bytes)
}
pub fn decode_bounded(bytes: &[u8]) -> Result<image::DynamicImage, String> {
    if bytes.len() as u64 > MAX_BYTES {
        return Err("图片超过 100 MiB 限制".into());
    }
    let reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| "无法识别图片格式")?;
    if !matches!(
        reader.format(),
        Some(image::ImageFormat::Png | image::ImageFormat::Jpeg)
    ) {
        return Err("仅支持 PNG / JPEG 图片附件".into());
    }
    let (w, h) = reader.into_dimensions().map_err(|_| "图片尺寸无效")?;
    if w == 0 || h == 0 || w > 16384 || h > 16384 || w as u64 * h as u64 > MAX_PIXELS {
        return Err("图片超过 16 MP / 16384 像素限制".into());
    }
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| "无法识别图片格式")?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    reader
        .decode()
        .map_err(|_| "图片损坏或超出 128 MiB 解码内存限制".into())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_png_and_jpeg() {
        for format in [image::ImageFormat::Png, image::ImageFormat::Jpeg] {
            let image = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
                3,
                2,
                image::Rgb([10, 80, 40]),
            ));
            let mut encoded = Cursor::new(Vec::new());
            image.write_to(&mut encoded, format).unwrap();
            assert_eq!(decode_bounded(encoded.get_ref()).unwrap().width(), 3);
        }
    }
    #[test]
    fn rejects_invalid_data() {
        assert!(decode_bounded(b"not an image").is_err());
    }
    #[test]
    fn rejects_oversized_dimensions_before_decode() {
        let image = image::DynamicImage::ImageRgb8(image::RgbImage::new(16385, 1));
        let mut encoded = Cursor::new(Vec::new());
        image
            .write_to(&mut encoded, image::ImageFormat::Png)
            .unwrap();
        assert!(decode_bounded(encoded.get_ref()).is_err());
    }
}
