use crate::{envelope, Engine, Error, Result};
use notify::{RecursiveMode, Watcher};
use std::{
    path::{Path, PathBuf},
    sync::{mpsc, Arc, Mutex, TryLockError},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
enum Event {
    Changed,
    Files,
    Stop,
}
/// Serialized native host runtime. UI adapters call on their own background queue.
pub struct Runtime {
    engine: Arc<Mutex<Engine>>,
    root: PathBuf,
    cached_status: Arc<Mutex<serde_json::Value>>,
    sender: mpsc::Sender<Event>,
    worker: Option<JoinHandle<()>>,
}
impl Runtime {
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let initial = Engine::open(root)?;
        let root = initial.root.clone();
        let cached_status = Arc::new(Mutex::new(initial.status()?));
        let engine = Arc::new(Mutex::new(initial));
        let (sender, receiver) = mpsc::channel();
        let worker = if std::env::var_os("PASTELEAF_DISABLE_SYNC_WORKER").is_some() {
            None
        } else {
            let weak = Arc::downgrade(&engine);
            let cache = cached_status.clone();
            let tx = sender.clone();
            Some(
                thread::Builder::new()
                    .name("pasteleaf-sync".into())
                    .spawn(move || {
                        let mut watcher = notify::recommended_watcher(
                            move |result: notify::Result<notify::Event>| {
                                if result.is_ok() {
                                    let _ = tx.send(Event::Files);
                                }
                            },
                        )
                        .ok();
                        let mut watched: Option<PathBuf> = None;
                        let mut next = Instant::now() + Duration::from_secs(2);
                        loop {
                            match receiver.recv_timeout(Duration::from_millis(250)) {
                                Ok(Event::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                                    break
                                }
                                Ok(Event::Changed) | Ok(Event::Files) => {
                                    let candidate = Instant::now() + Duration::from_secs(2);
                                    if candidate < next {
                                        next = candidate;
                                    }
                                }
                                Err(mpsc::RecvTimeoutError::Timeout) => {}
                            }
                            let Some(engine) = weak.upgrade() else { break };
                            if let Ok(mut e) = engine.try_lock() {
                                let dir = e.sync_directory();
                                if watched != dir {
                                    if let (Some(w), Some(old)) = (&mut watcher, &watched) {
                                        let _ = w.unwatch(old);
                                    }
                                    if let (Some(w), Some(new)) = (&mut watcher, &dir) {
                                        let _ = w.watch(new, RecursiveMode::Recursive);
                                    }
                                    watched = dir;
                                }
                                if Instant::now() >= next {
                                    let _ = e.exchange();
                                    refresh_cache(&e, &cache);
                                    next = Instant::now() + Duration::from_secs(10);
                                }
                            };
                        }
                    })
                    .map_err(Error::from)?,
            )
        };
        Ok(Self {
            engine,
            root,
            cached_status,
            sender,
            worker,
        })
    }
    pub fn request_json(&self, input: &str) -> String {
        if input.len() > crate::model::OP_LIMIT as usize {
            return envelope(Err(crate::error::invalid(
                "Request exceeds the supported limit",
            )));
        }
        let request = serde_json::from_str::<serde_json::Value>(input).ok();
        let op = request.as_ref().and_then(|v| v["op"].as_str());
        if matches!(op, Some("list" | "status")) {
            match self.engine.try_lock() {
                Ok(mut e) => {
                    let response = e.request_json(input);
                    if op == Some("status") {
                        refresh_cache(&e, &self.cached_status);
                    }
                    return response;
                }
                Err(TryLockError::WouldBlock) => {
                    if op == Some("list") {
                        return envelope(crate::readonly_list(
                            &self.root,
                            request.as_ref().expect("parsed list request"),
                        ));
                    }
                    return self.busy_status();
                }
                Err(TryLockError::Poisoned(_)) => return poisoned(),
            }
        }
        let response = match self.engine.lock() {
            Ok(mut e) => {
                let result = e.request_json(input);
                if !matches!(op, Some("get" | "conflicts" | "sync_inspect")) {
                    refresh_cache(&e, &self.cached_status);
                }
                result
            }
            Err(_) => poisoned(),
        };
        if op.is_some_and(|s| {
            !matches!(
                s,
                "list" | "get" | "status" | "conflicts" | "sync_now" | "sync_inspect"
            )
        }) {
            let _ = self.sender.send(Event::Changed);
        }
        response
    }
    fn busy_status(&self) -> String {
        let Ok(cache) = self.cached_status.lock() else {
            return poisoned();
        };
        let mut value = cache.clone();
        if matches!(
            value["sync_state"].as_str(),
            Some("published" | "peer_imported" | "pending")
        ) {
            value["sync_state"] = serde_json::json!("pending");
        }
        value["busy"] = serde_json::json!(true);
        value["detail"]=serde_json::json!("Core is processing background work; status fields are the last confirmed snapshot. Delivery is not confirmed by this refresh.");
        envelope(Ok(value))
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        let _ = self.sender.send(Event::Stop);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn refresh_cache(engine: &Engine, cache: &Mutex<serde_json::Value>) {
    if let Ok(status) = engine.status() {
        if let Ok(mut cached) = cache.lock() {
            *cached = status;
        }
    }
}
fn poisoned() -> String {
    envelope(Err(Error::new(
        "engine_poisoned",
        "Core stopped after an internal error; restart without deleting history",
    )))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sync_inspection_does_not_signal_exchange_or_refresh_status_cache() {
        let tmp = tempfile::tempdir().unwrap();
        let selected = tmp.path().join("empty-selected-folder");
        std::fs::create_dir(&selected).unwrap();
        let engine = Engine::open(tmp.path().join("local")).unwrap();
        let root = engine.root.clone();
        let (sender, receiver) = mpsc::channel();
        let marker = serde_json::json!({"synthetic_cache_marker":true});
        let runtime = Runtime {
            engine: Arc::new(Mutex::new(engine)),
            root,
            cached_status: Arc::new(Mutex::new(marker.clone())),
            sender,
            worker: None,
        };
        let request = serde_json::json!({"op":"sync_inspect","directory":selected});
        let response: serde_json::Value =
            serde_json::from_str(&runtime.request_json(&request.to_string())).unwrap();
        assert_eq!(response["ok"], true);
        assert_eq!(response["data"]["state"], "missing");
        assert!(matches!(
            receiver.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
        assert_eq!(*runtime.cached_status.lock().unwrap(), marker);
        assert_eq!(std::fs::read_dir(selected).unwrap().count(), 0);
    }
    #[test]
    fn busy_sync_writer_does_not_block_read_only_history_or_claim_delivery() {
        let t = tempfile::tempdir().unwrap();
        let runtime = Arc::new(Runtime::open(t.path()).unwrap());
        runtime.request_json(r#"{"op":"settings","values":{"paused":false}}"#);
        runtime.request_json(r#"{"op":"capture","kind":"text","text":"snapshot fixture"}"#);
        let writer = runtime.clone();
        let (ready_tx, ready_rx) = mpsc::channel();
        let hold = std::thread::spawn(move || {
            let _lock = writer.engine.lock().unwrap();
            ready_tx.send(()).unwrap();
            std::thread::sleep(Duration::from_millis(700));
        });
        ready_rx.recv().unwrap();
        let start = Instant::now();
        let reply: serde_json::Value =
            serde_json::from_str(&runtime.request_json(r#"{"op":"list"}"#)).unwrap();
        assert_eq!(reply["data"]["total"], 1);
        assert!(start.elapsed() < Duration::from_millis(200));
        let status: serde_json::Value =
            serde_json::from_str(&runtime.request_json(r#"{"op":"status"}"#)).unwrap();
        assert_eq!(status["data"]["busy"], true);
        assert_ne!(status["data"]["sync_state"], "peer_imported");
        hold.join().unwrap();
    }
}
