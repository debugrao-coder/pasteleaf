//! Native sync settings and UI-thread ownership of background results.
use super::*;
use pasteleaf::sync::{
    self,
    crypto::{self, Key},
};
use std::{
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum SyncAction {
    Open,
    Browse,
    Create,
    Join,
    Unlock,
    Scope,
    Remember,
    Now,
    Pause,
    Disconnect,
    Conflicts,
    NextVersion,
    Keep,
    KeepBoth,
}
enum WorkerResult {
    Connect {
        before: Vec<u8>,
        result: Box<Result<(Model, Key), String>>,
    },
    Exchange(Result<Box<Pending>, String>),
    Unlock(Result<Key, String>),
}
struct Pending {
    exchange: sync::Exchange,
    model: Model,
    count: usize,
    revision: u64,
}
fn prepare(
    mut model: Model,
    exchange: sync::Exchange,
    revision: u64,
) -> Result<Box<Pending>, String> {
    let count = sync::merge(&mut model, exchange.clone())?;
    Ok(Box::new(Pending {
        exchange,
        model,
        count,
        revision,
    }))
}
pub(super) struct SyncRuntime {
    pub key: Option<Key>,
    worker: Option<Receiver<WorkerResult>>,
    pub status: String,
    pub all: bool,
    pub remember: bool,
    due: Instant,
    dirty_since: Option<Instant>,
    retry: u64,
    pending: Option<Box<Pending>>,
    revision: u64,
    pub last: Option<u64>,
}
impl SyncRuntime {
    pub fn new(model: &Model) -> Self {
        let key = model.sync.config.as_ref().and_then(|c| {
            if c.remembered_key.is_empty() {
                None
            } else {
                crypto::unprotect(&c.remembered_key).ok()
            }
        });
        Self {
            key,
            worker: None,
            status: String::new(),
            all: model.sync.all_records(),
            remember: model
                .sync
                .config
                .as_ref()
                .is_none_or(|c| !c.remembered_key.is_empty()),
            due: Instant::now(),
            dirty_since: None,
            retry: 1,
            pending: None,
            revision: 0,
            last: None,
        }
    }
    pub fn busy(&self) -> bool {
        self.worker.is_some() || self.pending.is_some()
    }
    pub fn changed(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        let now = Instant::now();
        let first = *self.dirty_since.get_or_insert(now);
        self.due = (now + Duration::from_secs(2)).min(first + Duration::from_secs(10));
    }
}
impl App {
    pub(super) fn layout_sync_controls(&self) {
        let width = (self.width - 48).min(720);
        let x = (self.width - width) / 2;
        // SAFETY: owned EDITs; all coordinates use the current DPI scale.
        unsafe {
            for (control, y, w) in [
                (SYNC_PATH, 124, width - 138),
                (SYNC_PASSWORD, 178, (width - 62) / 2),
                (SYNC_CONFIRM, 178, (width - 62) / 2),
            ] {
                let offset = if control == SYNC_CONFIRM {
                    width / 2 + 7
                } else {
                    24
                };
                MoveWindow(
                    self.control(control),
                    self.px(x + offset),
                    self.px(y),
                    self.px(w),
                    self.px(24),
                    1,
                );
            }
        }
    }
    pub(super) fn prepare_sync_dialog(&mut self) {
        let path = self
            .model
            .sync
            .config
            .as_ref()
            .map(|c| c.directory.clone())
            .or_else(sync::suggested_directory)
            .unwrap_or_default();
        self.set_text(SYNC_PATH, &path.to_string_lossy());
        self.set_text(SYNC_PASSWORD, "");
        self.set_text(SYNC_CONFIRM, "");
        self.sync_runtime.all = self.model.sync.all_records();
        // SAFETY: native password EDITs hide text and block clipboard copying.
        unsafe {
            for control in [SYNC_PATH, SYNC_PASSWORD, SYNC_CONFIRM] {
                ShowWindow(self.control(control), SW_SHOW);
            }
            EnableWindow(
                self.control(SYNC_PATH),
                i32::from(self.model.sync.config.is_none()),
            );
            SetFocus(self.control(SYNC_PASSWORD));
        }
    }
    pub(super) fn prepare_conflict(&self, entity: &str, index: usize) {
        let heads = self.model.sync.heads(entity);
        if let Some(op) = heads.get(index % heads.len().max(1)) {
            let value = match &op.value {
                sync::Value::Record(r) => format!(
                    "标签：{}\r\n分类：{}\r\n\r\n{}",
                    r.tags.join("，"),
                    r.category
                        .as_ref()
                        .and_then(|c| self
                            .model
                            .sync
                            .category_ids
                            .iter()
                            .find(|(_, v)| *v == c)
                            .map(|(n, _)| n.as_str()))
                        .unwrap_or("历史"),
                    match &r.content {
                        Content::Text(t) => t.clone(),
                        Content::Image { width, height, .. } =>
                            format!("图片 {width} × {height}（此处仅显示尺寸，选择保留后可预览）"),
                    }
                ),
                sync::Value::Category { name, color } => {
                    format!("分类：{name}\r\n颜色：#{color:06X}")
                }
                sync::Value::Scope(all) => {
                    format!("同步范围：{}", if *all { "全部记录" } else { "仅收藏" })
                }
                sync::Value::Deleted => "此版本已删除记录".into(),
                sync::Value::Retired => "历史版本已归档".into(),
            };
            self.set_text(
                PREVIEW,
                &format!(
                    "版本 {} / {} · 设备 {}\r\n{}",
                    index % heads.len() + 1,
                    heads.len(),
                    op.device,
                    value
                ),
            );
        } else {
            self.set_text(PREVIEW, "冲突已处理");
        }
    }
    pub(super) fn sync_action(&mut self, action: SyncAction) -> Result<(), String> {
        if action == SyncAction::Open {
            self.open_dialog(Dialog::Sync);
            return Ok(());
        }
        if self.sync_runtime.busy() {
            return Err("同步正在处理，请稍后重试".into());
        }
        match action {
            SyncAction::Open => {}
            SyncAction::Browse => {
                if self.model.sync.config.is_some() {
                    return Ok(());
                }
                // SAFETY: shell owns the modal picker; PIDL is freed with its allocator.
                unsafe {
                    let title = wide("选择 iCloud Drive 中的专用空文件夹，或已有 PasteLeaf 同步库");
                    let info = BROWSEINFOW {
                        hwndOwner: self.hwnd,
                        lpszTitle: title.as_ptr(),
                        ulFlags: BIF_RETURNONLYFSDIRS,
                        ..zeroed()
                    };
                    let pidl = SHBrowseForFolderW(&info);
                    if !pidl.is_null() {
                        let mut path = [0u16; 260];
                        if SHGetPathFromIDListW(pidl, path.as_mut_ptr()) != 0 {
                            let len = path.iter().position(|c| *c == 0).unwrap_or(path.len());
                            self.set_text(SYNC_PATH, &String::from_utf16_lossy(&path[..len]));
                        }
                        CoTaskMemFree(pidl.cast());
                    }
                }
            }
            SyncAction::Create | SyncAction::Join => {
                if self.model.sync.config.is_some() {
                    return Err("请先断开当前同步库".into());
                }
                let password = Zeroizing::new(self.text(SYNC_PASSWORD));
                let confirmation = Zeroizing::new(self.text(SYNC_CONFIRM));
                let create = action == SyncAction::Create;
                if create && *password != *confirmation {
                    return Err("两次同步密码不一致".into());
                }
                if password.chars().count() < 12 {
                    return Err("同步密码至少 12 个字符".into());
                }
                let directory = PathBuf::from(self.text(SYNC_PATH));
                if !directory.is_absolute() {
                    return Err("请选择 iCloud Drive 中的绝对文件夹路径".into());
                }
                let all = self.sync_runtime.all;
                let remember = self.sync_runtime.remember;
                let model = self.model.clone();
                let local = self.store.root.clone();
                let before = serde_json::to_vec(&model).map_err(|e| e.to_string())?;
                let (tx, rx) = mpsc::channel();
                self.sync_runtime.worker = Some(rx);
                self.sync_runtime.status = "正在处理：验证密码并合并记录".into();
                std::thread::spawn(move || {
                    let result =
                        sync::connect(&model, &local, &directory, &password, create, all, remember);
                    let _ = tx.send(WorkerResult::Connect {
                        before,
                        result: Box::new(result),
                    });
                });
            }
            SyncAction::Unlock => {
                let descriptor = self
                    .model
                    .sync
                    .config
                    .as_ref()
                    .ok_or("请先创建或加入同步库")?
                    .descriptor
                    .clone();
                let password = Zeroizing::new(self.text(SYNC_PASSWORD));
                let (tx, rx) = mpsc::channel();
                self.sync_runtime.worker = Some(rx);
                std::thread::spawn(move || {
                    let _ = tx.send(WorkerResult::Unlock(descriptor.unlock(&password)));
                });
            }
            SyncAction::Scope => {
                let all = !self.sync_runtime.all;
                if self.model.sync.config.is_some() {
                    if !self.confirm(if all {
                        "改为同步全部记录？现有及后续剪贴板历史也会上传。"
                    } else {
                        "改为仅收藏？后续历史停止同步，各电脑已有历史及云端旧文件不会自动删除。"
                    }) {
                        return Ok(());
                    }
                    let mut next = self.model.clone();
                    sync::set_scope(&mut next, all)?;
                    self.store.save(&next)?;
                    self.model = next;
                    self.sync_runtime.changed();
                }
                self.sync_runtime.all = all;
            }
            SyncAction::Remember => {
                let remember = !self.sync_runtime.remember;
                let mut next = self.model.clone();
                if let Some(c) = next.sync.config.as_mut() {
                    c.remembered_key = if remember {
                        crypto::protect(&**self.sync_runtime.key.as_ref().ok_or("请先解锁")?)?
                    } else {
                        vec![]
                    };
                    self.store.save(&next)?;
                    self.model = next;
                }
                self.sync_runtime.remember = remember;
            }
            SyncAction::Now => {
                self.sync_runtime.due = Instant::now();
            }
            SyncAction::Pause => {
                let mut next = self.model.clone();
                let c = next.sync.config.as_mut().ok_or("同步未启用")?;
                c.paused = !c.paused;
                self.store.save(&next)?;
                self.model = next;
                self.sync_runtime.due = Instant::now();
            }
            SyncAction::Disconnect => {
                if !self
                    .confirm("断开同步并移除本机保存的密钥？本地记录和 iCloud Drive 文件都会保留。")
                {
                    return Ok(());
                }
                let mut next = self.model.clone();
                sync::disconnect(&mut next);
                self.store.save(&next)?;
                self.model = next;
                self.sync_runtime = SyncRuntime::new(&self.model);
                self.prepare_sync_dialog();
            }
            SyncAction::Conflicts => {
                if let Some(entity) = self.model.sync.conflicts().first() {
                    self.open_dialog(Dialog::SyncConflict(entity.clone(), 0));
                } else {
                    return Err("当前没有同步冲突".into());
                }
            }
            SyncAction::NextVersion => {
                if let Dialog::SyncConflict(entity, index) = self.dialog.clone() {
                    self.open_dialog(Dialog::SyncConflict(entity, index + 1));
                }
            }
            SyncAction::Keep | SyncAction::KeepBoth => {
                if let Dialog::SyncConflict(entity, index) = self.dialog.clone() {
                    let heads = self.model.sync.heads(&entity);
                    let selected = heads
                        .get(index % heads.len().max(1))
                        .ok_or("冲突已变化")?
                        .id
                        .clone();
                    let mut next = self.model.clone();
                    sync::resolve(
                        &mut next,
                        &entity,
                        &selected,
                        action == SyncAction::KeepBoth,
                    )?;
                    self.store.save(&next)?;
                    self.model = next;
                    self.sync_runtime.changed();
                    self.open_dialog(Dialog::Sync);
                    self.refresh();
                }
            }
        }
        self.invalidate();
        Ok(())
    }
    pub(super) fn sync_tick(&mut self) {
        if let Some(receiver) = &self.sync_runtime.worker {
            let message = match receiver.try_recv() {
                Ok(message) => Some(message),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(_) => {
                    self.sync_runtime.worker = None;
                    self.sync_runtime.status = "同步任务中断，可点击立即同步重试".into();
                    None
                }
            };
            if let Some(message) = message {
                self.sync_runtime.worker = None;
                let result: Result<(), String> = (|| {
                    match message {
                        WorkerResult::Connect { before, result } => {
                            let (next, key) = (*result)?;
                            if serde_json::to_vec(&self.model).map_err(|e| e.to_string())? != before
                            {
                                return Err(
                                    "连接期间本地记录发生变化，原数据已保留；请重试。尚未发送记录的新库可再次选择创建"
                                        .into(),
                                );
                            }
                            let count = next
                                .sync
                                .outbox
                                .iter()
                                .filter_map(|id| next.sync.operations.get(id))
                                .filter(|o| matches!(o.value, sync::Value::Record(_)))
                                .map(|o| &o.entity)
                                .collect::<std::collections::BTreeSet<_>>()
                                .len();
                            if self.dialog != Dialog::Sync || !self.confirm(&format!("启用加密同步？\n实际范围：{}\n本机待上传 {count} 条记录。\n请确认所选目录位于 iCloud Drive 中，由客户端上传；本地原始数据仍未加密。", if next.sync.all_records() {"全部记录"} else {"仅收藏"})) {
                                self.sync_runtime.status = "已取消，尚未启用同步".into();
                                return Ok(());
                            }
                            self.store.save(&next)?;
                            self.model = next;
                            self.sync_runtime.key = Some(key);
                            self.sync_runtime.all = self.model.sync.all_records();
                            self.set_text(SYNC_PASSWORD, "");
                            self.set_text(SYNC_CONFIRM, "");
                            self.prepare_sync_dialog();
                            self.sync_runtime.due = Instant::now();
                            self.layout_controls();
                            self.refresh();
                        }
                        WorkerResult::Unlock(result) => {
                            let key = result?;
                            let mut next = self.model.clone();
                            if self.sync_runtime.remember {
                                if let Some(c) = next.sync.config.as_mut() {
                                    c.remembered_key = crypto::protect(&*key)?;
                                }
                            }
                            self.store.save(&next)?;
                            self.model = next;
                            self.sync_runtime.key = Some(key);
                            self.set_text(SYNC_PASSWORD, "");
                            self.sync_runtime.due = Instant::now();
                        }
                        WorkerResult::Exchange(result) => {
                            self.sync_runtime.pending = Some(result?);
                        }
                    }
                    Ok(())
                })();
                if let Err(error) = result {
                    self.sync_failed(error);
                }
                self.invalidate();
            }
        }
        if self.sync_runtime.pending.is_some() && Instant::now() >= self.sync_runtime.due {
            let pending = self.sync_runtime.pending.as_ref().expect("checked pending");
            if pending.revision != self.sync_runtime.revision {
                let exchange = pending.exchange.clone();
                let model = self.model.clone();
                let revision = self.sync_runtime.revision;
                let (tx, rx) = mpsc::channel();
                self.sync_runtime.worker = Some(rx);
                self.sync_runtime.pending = None;
                std::thread::spawn(move || {
                    let _ = tx.send(WorkerResult::Exchange(prepare(model, exchange, revision)));
                });
                return;
            }
            let next = pending.model.clone();
            let waiting = pending.exchange.waiting;
            let notice = pending.exchange.notice.clone();
            let result = self.store.save(&next).map(|_| pending.count);
            match result {
                Ok(pending) => {
                    self.model = next;
                    self.sync_runtime.pending = None;
                    self.sync_runtime.retry = 1;
                    self.sync_runtime.last = Some(timestamp());
                    self.sync_runtime.status = format!(
                        "已写入同步文件夹 · {} 个冲突 · {} 项等待导入",
                        self.model.sync.conflicts().len(),
                        pending + waiting
                    );
                    if let Some(notice) = notice {
                        self.sync_runtime.status = notice;
                    }
                    if pending + waiting > 0 || !self.model.sync.conflicts().is_empty() {
                        self.status =
                            "同步有冲突或待导入内容，请打开设置 → iCloud Drive 同步查看".into();
                    }
                    self.sync_runtime.all = self.model.sync.all_records();
                    self.sync_runtime.due = Instant::now() + Duration::from_secs(10);
                    self.sync_runtime.dirty_since = None;
                    self.thumbnails.clear();
                    self.refresh();
                }
                Err(error) => self.sync_failed(error),
            }
        }
        if self.sync_runtime.busy()
            || self.sync_runtime.pending.is_some()
            || Instant::now() < self.sync_runtime.due
        {
            return;
        }
        let Some(config) = &self.model.sync.config else {
            return;
        };
        if config.paused {
            return;
        }
        let Some(key) = self.sync_runtime.key.clone() else {
            return;
        };
        let model = self.model.clone();
        let revision = self.sync_runtime.revision;
        let local = self.store.root.clone();
        let (tx, rx) = mpsc::channel();
        self.sync_runtime.worker = Some(rx);
        self.sync_runtime.status = "正在处理同步文件".into();
        self.invalidate();
        std::thread::spawn(move || {
            let result = sync::exchange(&model.sync, &local, &key, true)
                .and_then(|exchange| prepare(model, exchange, revision));
            let _ = tx.send(WorkerResult::Exchange(result));
        });
    }
    fn sync_failed(&mut self, error: String) {
        self.sync_runtime.status = format!("失败待重试：{error}");
        self.status = self.sync_runtime.status.clone();
        self.sync_runtime.due = Instant::now() + Duration::from_secs(self.sync_runtime.retry);
        self.sync_runtime.retry = (self.sync_runtime.retry * 2).min(60);
        self.invalidate();
    }
}
#[link(name = "ole32")]
unsafe extern "system" {
    fn CoTaskMemFree(pointer: *const std::ffi::c_void);
}
