use pasteleaf::{
    model::{Content, Model},
    store::Store,
};
use std::path::PathBuf;

fn main() -> Result<(), String> {
    let dir = std::env::args_os()
        .nth(1)
        .ok_or("usage: seed_preview <empty-preview-directory>")?;
    let root = PathBuf::from(dir);
    if root.join("library.json").exists() {
        return Err("Preview directory already contains data".into());
    }
    let store = Store::new(root)?;
    let mut model = Model {
        paused: true,
        ..Model::default()
    };
    model.add_category("提示词")?;
    model.add_category("工作")?;
    model.capture(Content::Text(
        "https://www.rust-lang.org/\n\nRust 官方网站".into(),
    ))?;
    let id = model.capture(Content::Text(
        "请用清晰、简洁的中文解释这个问题，并给出一个具体例子。".into(),
    ))?;
    model.assign(id, Some("提示词".into()))?;
    let mut rgba = Vec::with_capacity(320 * 180 * 4);
    for y in 0..180u32 {
        for x in 0..320u32 {
            rgba.extend_from_slice(&[(30 + x / 4) as u8, (110 + y / 3) as u8, 110, 255]);
        }
    }
    let image = store.write_image(320, 180, &rgba)?;
    model.capture(image)?;
    let id = model.capture(Content::Text(
        "把值得留下的内容，放在手边。\n\nCtrl+Q 呼出\n回车或双击直接粘贴".into(),
    ))?;
    model.assign(id, Some("收藏".into()))?;
    model.capture(Content::Text(
        "const ideas = clipboard\n  .filter(item => item.pinned)\n  .map(item => item.content);"
            .into(),
    ))?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs();
    for (i, item) in model.items.iter_mut().enumerate() {
        item.source = ["Code", "Notes", "Photos", "ChatGPT", "Browser"]
            .get(i)
            .unwrap_or(&"Clipboard")
            .to_string();
        item.copied_at = now.saturating_sub(i as u64 * 240);
    }
    store.save(&model)?;
    println!("Preview seeded (capture paused).");
    Ok(())
}
