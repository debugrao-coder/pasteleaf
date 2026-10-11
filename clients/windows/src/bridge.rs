//! Core ownership is confined to one worker; no database, crypto or file I/O in WndProc.
use crate::model::PastePayload;
use serde_json::{json, Value};
use std::{
    ffi::{CStr, CString},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc::{self, Receiver, Sender},
        Arc,
    },
    thread,
};
#[derive(Clone, Debug)]
pub enum Tag {
    List(u64),
    Status,
    Action,
    Conflicts,
    PasteReady,
    Thumbnail(String),
}
pub struct Reply {
    pub tag: Tag,
    pub result: Result<Value, String>,
}
enum Work {
    Request(Tag, Value),
    Capture(String, u32),
    Paste(PastePayload, Vec<String>),
    Stop,
}
pub struct Bridge {
    tx: Sender<Work>,
    pub rx: Receiver<Reply>,
    pub writing: Arc<AtomicBool>,
    pub own_sequence: Arc<AtomicU32>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Bridge {
    pub fn start(root: PathBuf, demo: bool, window: usize) -> Self {
        #[cfg(not(windows))]
        let _ = window;
        let (tx, work) = mpsc::channel();
        let (reply, rx) = mpsc::channel();
        let writing = Arc::new(AtomicBool::new(false));
        let own_sequence = Arc::new(AtomicU32::new(0));
        let w = writing.clone();
        let sequence = own_sequence.clone();
        let thread = thread::spawn(move || {
            let path = match CString::new(root.to_string_lossy().as_bytes()) {
                Ok(s) => s,
                Err(_) => {
                    let _ = reply.send(Reply {
                        tag: Tag::Status,
                        result: Err("数据目录包含无效字符".into()),
                    });
                    return;
                }
            };
            // SAFETY: handle is private to this thread and released after all queued operations.
            let handle = unsafe { pasteleaf_core::pl_open(path.as_ptr()) };
            if handle.is_null() {
                let _ = reply.send(Reply {
                    tag: Tag::Status,
                    result: Err("无法打开历史库，请检查目录权限和可用空间".into()),
                });
                return;
            }
            let request = |value: Value| -> Result<Value, String> {
                let input =
                    CString::new(value.to_string()).map_err(|_| "请求包含无效字符".to_string())?;
                let result = unsafe { pasteleaf_core::pl_request(handle, input.as_ptr()) };
                if result.is_null() {
                    return Err("共享核心未返回有效响应".into());
                }
                let parsed =
                    unsafe { serde_json::from_slice::<Value>(CStr::from_ptr(result).to_bytes()) };
                unsafe { pasteleaf_core::pl_free(result) };
                let v = parsed.map_err(|_| "共享核心响应格式错误".to_string())?;
                if v["ok"].as_bool() != Some(true) {
                    return Err(v["error"]["message"]
                        .as_str()
                        .unwrap_or("操作失败；请查看同步状态后重试")
                        .into());
                }
                Ok(v["data"].clone())
            };
            if demo {
                let _ = request(json!({"op":"settings","values":{"paused":false}}));
                for (kind, text, html, source) in [
                    (
                        "text",
                        "把灵感留在手边。\n\nPasteLeaf 的每一次复制，都能成为下一次创作的起点。",
                        None,
                        "Notes",
                    ),
                    (
                        "text",
                        "https://pasteleaf.app/design",
                        None,
                        "Microsoft Edge",
                    ),
                    (
                        "rich_text",
                        "周末计划\n沿着海岸，走一段慢路。",
                        Some("<h2>周末计划</h2><p>沿着海岸，走一段慢路。</p>"),
                        "Word",
                    ),
                    (
                        "text",
                        "const idea = await clipboard.recall();\n\n// Keep your next idea close.",
                        None,
                        "Visual Studio Code",
                    ),
                    (
                        "text",
                        "新想法 · 新连接\n\n设计是让复杂的事情变得轻盈。",
                        None,
                        "Notes",
                    ),
                ] {
                    let _ = request(
                        json!({"op":"capture","kind":kind,"text":text,"html":html,"source":source}),
                    );
                }
            }
            if demo {
                #[cfg(windows)]
                {
                    let asset = root.join("demo-art.png");
                    let pixels = image::RgbaImage::from_fn(640, 400, |x, y| {
                        let wave = ((x as f32 / 60.).sin() * 30. + y as f32 / 3.) as u8;
                        image::Rgba([82 + wave / 2, 125 + wave / 3, 112 + wave / 4, 255])
                    });
                    if pixels.save(&asset).is_ok() {
                        let _ = request(
                            json!({"op":"capture","kind":"image","image_path":asset,"source":"Photos"}),
                        );
                    }
                    let file = root.join("春日灵感设计说明.txt");
                    if std::fs::write(&file, "PasteLeaf synthetic attachment. No user data.")
                        .is_ok()
                    {
                        let _ = request(
                            json!({"op":"capture","kind":"files","paths":[file],"source":"Explorer"}),
                        );
                    }
                }
                let _ = request(json!({"op":"settings","values":{"paused":true}}));
            }
            for job in work {
                let response = match job {
                    Work::Stop => break,
                    Work::Request(tag, value) => {
                        #[cfg(windows)]
                        let result = if value.get("_thumbnail").is_some() {
                            use base64::Engine;
                            let id = if let Tag::Thumbnail(id) = &tag {
                                id.as_str()
                            } else {
                                ""
                            };
                            request(json!({"op":"get","id":id})).and_then(|item|{
                                let path=item["image_path"].as_str().ok_or_else(||"图片附件不可用".to_string())?;
                                crate::image_io::read_bounded(std::path::Path::new(path)).map(|im|{let im=im.thumbnail(460,340).into_rgba8();json!({"width":im.width(),"height":im.height(),"rgba":base64::engine::general_purpose::STANDARD.encode(im.as_raw())})})
                            })
                        } else {
                            request(value)
                        };
                        #[cfg(not(windows))]
                        let result = request(value);
                        Reply { tag, result }
                    }
                    Work::Capture(source, seq) => {
                        #[cfg(windows)]
                        let result = crate::native::clipboard::capture(&root, &source, seq)
                            .and_then(|v| match v {
                                Some(v) => request(v),
                                None => Ok(Value::Null),
                            });
                        #[cfg(not(windows))]
                        let result = {
                            let _ = (&source, seq);
                            Ok(Value::Null)
                        };
                        Reply {
                            tag: Tag::Action,
                            result,
                        }
                    }
                    Work::Paste(payload, ids) => {
                        w.store(true, Ordering::SeqCst);
                        #[cfg(windows)]
                        let result = revalidate_paste(payload, &ids, &request)
                            .and_then(|payload| crate::native::clipboard::write(payload, window))
                            .map(|seq| {
                                sequence.store(seq, Ordering::SeqCst);
                                json!({"sequence":seq})
                            });
                        #[cfg(not(windows))]
                        let result = {
                            let _ = (&payload, &ids);
                            sequence.store(0, Ordering::SeqCst);
                            Err("仅支持 Windows".into())
                        };
                        w.store(false, Ordering::SeqCst);
                        Reply {
                            tag: Tag::PasteReady,
                            result,
                        }
                    }
                };
                if reply.send(response).is_err() {
                    break;
                }
            }
            unsafe { pasteleaf_core::pl_close(handle) };
        });
        Self {
            tx,
            rx,
            writing,
            own_sequence,
            thread: Some(thread),
        }
    }
    pub fn request(&self, tag: Tag, value: Value) {
        let _ = self.tx.send(Work::Request(tag, value));
    }
    pub fn capture(&self, source: String, sequence: u32) {
        let _ = self.tx.send(Work::Capture(source, sequence));
    }
    pub fn paste(&self, value: PastePayload, ids: Vec<String>) {
        let _ = self.tx.send(Work::Paste(value, ids));
    }
}
impl Drop for Bridge {
    fn drop(&mut self) {
        let _ = self.tx.send(Work::Stop);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn revalidate_paste(
    payload: PastePayload,
    ids: &[String],
    request: &impl Fn(Value) -> Result<Value, String>,
) -> Result<PastePayload, String> {
    let mut items = Vec::new();
    let ids = if let PastePayload::Item(ref item) = payload {
        vec![item.id.clone()]
    } else {
        ids.to_vec()
    };
    if ids.is_empty() {
        return Err("记录标识缺失，未修改系统剪贴板".into());
    }
    for id in ids {
        let item: crate::model::Item =
            serde_json::from_value(request(json!({"op":"get","id":id}))?)
                .map_err(|_| "记录响应无效".to_string())?;
        if !item.available {
            return Err("附件已变更或尚未完整到达".into());
        }
        items.push(item);
    }
    match payload {
        PastePayload::Item(_) => Ok(PastePayload::Item(Box::new(items.remove(0)))),
        PastePayload::Text(_) => {
            if items.iter().any(|i| i.text.is_empty()) {
                return Err("记录已变更，不再包含可粘贴的纯文本".into());
            }
            Ok(PastePayload::Text(
                items
                    .iter()
                    .map(|i| i.text.clone())
                    .collect::<Vec<_>>()
                    .join("\n"),
            ))
        }
        PastePayload::Files(_) => {
            if items.iter().any(|i| i.kind != "files") {
                return Err("记录类型已变更，请重新选择".into());
            }
            Ok(PastePayload::Files(
                items
                    .iter()
                    .flat_map(|i| i.files.iter().map(|f| f.path.clone()))
                    .collect(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn root() -> PathBuf {
        // Wall-clock nanoseconds can repeat; concurrent tests must not share SQLite files.
        static NEXT_ROOT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        std::env::temp_dir().join(format!(
            "pasteleaf-windows-bridge-test-{}-{}-{}",
            std::process::id(),
            NEXT_ROOT.fetch_add(1, Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }
    #[test]
    fn temporary_roots_are_unique() {
        let roots: std::collections::HashSet<_> = (0..10_000).map(|_| root()).collect();
        assert_eq!(roots.len(), 10_000);
    }
    #[test]
    fn grouped_paste_revalidates_every_id_and_uses_current_content() {
        let seen = std::cell::RefCell::new(Vec::new());
        let payload=revalidate_paste(PastePayload::Files(vec!["old-untrusted-path".into()]),&["a".into(),"b".into()],&|request|{let id=request["id"].as_str().unwrap();seen.borrow_mut().push(id.to_owned());Ok(json!({"id":id,"kind":"files","available":true,"files":[{"path":format!("verified-{id}")}]}))}).unwrap();
        assert_eq!(*seen.borrow(), vec!["a", "b"]);
        match payload {
            PastePayload::Files(paths) => assert_eq!(paths, vec!["verified-a", "verified-b"]),
            _ => panic!(),
        }
    }
    #[test]
    fn failed_verification_blocks_paste() {
        let result = revalidate_paste(PastePayload::Text("stale".into()), &["a".into()], &|_| {
            Err("attachment hash mismatch".into())
        });
        assert!(result.is_err());
    }
    #[test]
    fn core_worker_starts_paused_and_processes_requests_in_order() {
        let root = root();
        let bridge = Bridge::start(root.clone(), false, 0);
        bridge.request(Tag::Status, json!({"op":"status"}));
        let initial = bridge
            .rx
            .recv_timeout(std::time::Duration::from_secs(20))
            .unwrap()
            .result
            .unwrap();
        assert_eq!(initial["paused"], true);
        bridge.request(
            Tag::Action,
            json!({"op":"settings","values":{"paused":false,"onboarding_complete":true}}),
        );
        bridge.request(
            Tag::Action,
            json!({"op":"capture","kind":"text","text":"合成内容 · 中文 🙂","source":"unit-test"}),
        );
        bridge.request(Tag::List(7), json!({"op":"list","limit":64}));
        for _ in 0..2 {
            assert!(bridge
                .rx
                .recv_timeout(std::time::Duration::from_secs(20))
                .unwrap()
                .result
                .is_ok());
        }
        let list = bridge
            .rx
            .recv_timeout(std::time::Duration::from_secs(20))
            .unwrap()
            .result
            .unwrap();
        assert_eq!(list["items"][0]["text"], "合成内容 · 中文 🙂");
        drop(bridge);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn demo_has_synthetic_content_and_remains_paused() {
        let root = root();
        let bridge = Bridge::start(root.clone(), true, 0);
        bridge.request(Tag::List(1), json!({"op":"list","limit":64}));
        let list = bridge
            .rx
            .recv_timeout(std::time::Duration::from_secs(20))
            .unwrap()
            .result
            .unwrap();
        assert!(list["total"].as_u64().unwrap() >= 5);
        bridge.request(Tag::Status, json!({"op":"status"}));
        let status = bridge
            .rx
            .recv_timeout(std::time::Duration::from_secs(20))
            .unwrap()
            .result
            .unwrap();
        assert_eq!(status["paused"], true);
        assert_eq!(status["sync_state"], "disconnected");
        drop(bridge);
        std::fs::remove_dir_all(root).unwrap();
    }
}
