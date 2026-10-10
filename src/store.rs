use crate::model::{valid_image_file, Content, Model};
use std::{
    collections::HashSet,
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
};

pub struct Store {
    pub root: PathBuf,
}
impl Store {
    pub fn new(root: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(root.join("images")).map_err(|e| format!("无法创建数据目录：{e}"))?;
        let root = root.canonicalize().map_err(|e| e.to_string())?;
        Ok(Self { root })
    }
    pub fn load(&self) -> Result<Model, String> {
        let path = self.root.join("library.json");
        match fs::metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Model::default()),
            Err(e) => return Err(e.to_string()),
            Ok(m) if m.len() > 256 * 1024 * 1024 => return Err("数据文件过大；原文件未改动".into()),
            _ => {}
        }
        let bytes = fs::read(path).map_err(|e| e.to_string())?;
        let model: Model = serde_json::from_slice(&bytes)
            .map_err(|_| "数据文件损坏或格式不受支持；原文件未改动，请备份后检查 library.json")?;
        model.validate()?;
        Ok(model)
    }
    pub fn save(&self, model: &Model) -> Result<(), String> {
        model.validate()?;
        let bytes = serde_json::to_vec(model).map_err(|e| e.to_string())?;
        let limit = if model.sync.operations.is_empty() {
            70
        } else {
            256
        };
        if bytes.len() > limit * 1024 * 1024 {
            return Err(format!("序列化后数据超过 {limit} MiB；原文件未改动"));
        }
        let temp = self.root.join("library.json.tmp");
        let mut file = File::create(&temp).map_err(|e| format!("无法保存数据：{e}"))?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| format!("写入失败：{e}"))?;
        drop(file);
        let current = self.root.join("library.json");
        // Keep the last validated snapshot before replacing the current library.
        if let Ok(previous) = fs::read(&current) {
            if let Ok(mut snapshot) = serde_json::from_slice::<Model>(&previous) {
                if snapshot.validate().is_err() {
                    return Err("当前数据校验失败，原文件未改动".into());
                }
                // A rollback must not restore a key the user chose to forget.
                if let Some(config) = &mut snapshot.sync.config {
                    config.remembered_key.clear();
                }
                let previous = serde_json::to_vec(&snapshot).map_err(|e| e.to_string())?;
                let backup = self.root.join("library.previous.tmp");
                let mut file =
                    File::create(&backup).map_err(|e| format!("无法创建数据备份：{e}"))?;
                file.write_all(&previous)
                    .and_then(|_| file.sync_all())
                    .map_err(|e| e.to_string())?;
                drop(file);
                atomic_replace(&backup, &self.root.join("library.previous.json"))?;
            }
        }
        atomic_replace(&temp, &current)
    }
    pub fn write_image(&self, width: u32, height: u32, rgba: &[u8]) -> Result<Content, String> {
        let expected = (width as u64 * height as u64)
            .checked_mul(4)
            .ok_or("图片尺寸超限")?;
        if expected == 0 || expected > 16 * 1024 * 1024 || expected != rgba.len() as u64 {
            return Err("图片超过 16 MiB 或像素数据不完整".into());
        }
        let mut hash = blake3::Hasher::new();
        hash.update(&width.to_le_bytes());
        hash.update(&height.to_le_bytes());
        hash.update(rgba);
        let file = format!("{}.png", hash.finalize().to_hex());
        let path = self.root.join("images").join(&file);
        if !path.exists() {
            let temp = path.with_extension("tmp");
            image::save_buffer_with_format(
                &temp,
                rgba,
                width,
                height,
                image::ColorType::Rgba8,
                image::ImageFormat::Png,
            )
            .map_err(|e| e.to_string())?;
            fs::OpenOptions::new()
                .write(true)
                .open(&temp)
                .and_then(|f| f.sync_all())
                .map_err(|e| e.to_string())?;
            atomic_replace(&temp, &path)?;
        }
        Ok(Content::Image {
            file,
            width,
            height,
        })
    }
    pub fn read_image(&self, file: &str) -> Result<image::RgbaImage, String> {
        if !valid_image_file(file) {
            return Err("无效图片文件名".into());
        }
        let path = self.root.join("images").join(file);
        if fs::metadata(&path).map_err(|e| e.to_string())?.len() > 20 * 1024 * 1024 {
            return Err("图片文件过大".into());
        }
        let reader = image::ImageReader::open(&path).map_err(|e| e.to_string())?;
        let (w, h) = reader.into_dimensions().map_err(|e| e.to_string())?;
        if w as u64 * h as u64 > 4 * 1024 * 1024 {
            return Err("图片尺寸超限".into());
        }
        let mut reader = image::ImageReader::open(&path).map_err(|e| e.to_string())?;
        let mut limits = image::Limits::default();
        limits.max_alloc = Some(32 * 1024 * 1024);
        reader.limits(limits);
        reader
            .decode()
            .map(|x| x.to_rgba8())
            .map_err(|e| e.to_string())
    }
    pub fn cleanup(&self, model: &Model) -> Result<(), String> {
        let mut used: HashSet<String> = model
            .items
            .iter()
            .filter_map(|x| {
                if let Content::Image { file, .. } = &x.content {
                    Some(file.clone())
                } else {
                    None
                }
            })
            .collect();
        used.extend(model.sync.image_files());
        if let Ok(bytes) = fs::read(self.root.join("library.previous.json")) {
            if let Ok(previous) = serde_json::from_slice::<Model>(&bytes) {
                if previous.validate().is_ok() {
                    used.extend(previous.sync.image_files());
                    used.extend(
                        previous
                            .items
                            .into_iter()
                            .filter_map(|item| match item.content {
                                Content::Image { file, .. } => Some(file),
                                _ => None,
                            }),
                    );
                }
            }
        }
        for entry in fs::read_dir(self.root.join("images")).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if valid_image_file(&name)
                && !used.contains(name.as_ref())
                && entry.file_type().map_err(|e| e.to_string())?.is_file()
            {
                fs::remove_file(entry.path()).map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
}

fn atomic_replace(source: &Path, destination: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };
    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    // SAFETY: both paths are live NUL-terminated buffers on the same local volume.
    let success = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if success == 0 {
        Err(format!(
            "原数据未替换，保存失败：{}",
            std::io::Error::last_os_error()
        ))
    } else {
        Ok(())
    }
}
