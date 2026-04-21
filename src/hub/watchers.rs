//! Lazy per-chamber file watchers. A `WatcherRegistry` keeps one watcher
//! thread per chamber path; `ensure_watching` is idempotent so the discovery
//! pass can just call it for every known chamber on every refresh.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use notify::{
    event::{ModifyKind, RenameMode},
    recommended_watcher, Event as NotifyEvent, EventKind, RecursiveMode, Watcher,
};

use crate::hub::state::SseEvent;

/// Stored handle per chamber: the watcher (kept alive by the thread) and the
/// stop flag for the background log/state poll thread.
struct Handle {
    _watcher: notify::RecommendedWatcher,
    _stop: Arc<AtomicBool>,
}

#[derive(Default, Clone)]
pub struct WatcherRegistry {
    inner: Arc<Mutex<HashMap<PathBuf, Handle>>>,
}

impl WatcherRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start a watcher for `dir` if we don't already have one.
    pub fn ensure_watching(
        &self,
        chamber_id: String,
        dir: &Path,
        tx: tokio::sync::broadcast::Sender<SseEvent>,
    ) {
        let mut map = self.inner.lock().unwrap();
        if map.contains_key(dir) {
            return;
        }
        if let Some(handle) = spawn_watcher(chamber_id, dir, tx) {
            map.insert(dir.to_path_buf(), handle);
        }
    }

    /// Drop watchers for any chamber whose path is not in `keep`.
    pub fn retain(&self, keep: &std::collections::BTreeSet<PathBuf>) {
        let mut map = self.inner.lock().unwrap();
        // Signal poll threads to stop before dropping their handles.
        for (p, handle) in map.iter() {
            if !keep.contains(p) {
                handle._stop.store(true, Ordering::Relaxed);
            }
        }
        map.retain(|p, _| keep.contains(p));
    }

    /// Drop the watcher for a single chamber path so the next `ensure_watching`
    /// rebuilds it. Needed after reset: `archive_runtime` renames `messages/`
    /// out from under the notify handle, leaving it tied to the archived dir
    /// instead of the freshly re-created one.
    pub fn drop_watcher(&self, dir: &Path) {
        let mut map = self.inner.lock().unwrap();
        if let Some(handle) = map.remove(dir) {
            handle._stop.store(true, Ordering::Relaxed);
        }
    }
}

fn spawn_watcher(
    chamber_id: String,
    dir: &Path,
    tx: tokio::sync::broadcast::Sender<SseEvent>,
) -> Option<Handle> {
    let inbox = dir.join("messages").join("inbox");
    let outbox = dir.join("messages").join("outbox");

    // File watcher: messages
    let tx_msg = tx.clone();
    // Canonicalize so that `starts_with` works on macOS where tempdir may
    // return a symlink (e.g. /var/folders/… → /private/var/folders/…).
    let inbox_for_cb = inbox.canonicalize().unwrap_or_else(|_| inbox.clone());
    let outbox_for_cb = outbox.canonicalize().unwrap_or_else(|_| outbox.clone());
    let id_for_cb = chamber_id.clone();
    let mut watcher = recommended_watcher(move |res: Result<NotifyEvent, _>| {
        if let Ok(event) = res {
            // Accept both Create events and Rename events (atomic writes via
            // rename show up as Modify(Name) on macOS FSEvents).
            let is_create_or_rename = matches!(event.kind, EventKind::Create(_))
                || matches!(
                    event.kind,
                    EventKind::Modify(ModifyKind::Name(RenameMode::Any))
                        | EventKind::Modify(ModifyKind::Name(RenameMode::To))
                );
            if is_create_or_rename {
                for path in &event.paths {
                    if path.extension().is_some_and(|e| e == "md") {
                        let direction = if path.starts_with(&inbox_for_cb) {
                            "inbox"
                        } else if path.starts_with(&outbox_for_cb) {
                            "outbox"
                        } else {
                            continue;
                        };
                        if let Ok(content) = std::fs::read_to_string(path) {
                            if let Ok(msg) = crate::message::parse_message(&content) {
                                let _ = tx_msg.send(SseEvent::NewMessage {
                                    chamber_id: id_for_cb.clone(),
                                    direction: direction.to_string(),
                                    from: msg.from,
                                    subject: msg.subject,
                                    body: msg.body,
                                    timestamp: msg
                                        .timestamp
                                        .format("%Y-%m-%dT%H:%M:%S")
                                        .to_string(),
                                });
                            }
                        }
                    }
                }
            }
        }
    })
    .ok()?;

    let _ = std::fs::create_dir_all(&inbox);
    let _ = std::fs::create_dir_all(&outbox);
    watcher.watch(&inbox, RecursiveMode::NonRecursive).ok()?;
    watcher.watch(&outbox, RecursiveMode::NonRecursive).ok()?;

    // Background poll: log tail + timer.json change.
    let stop = Arc::new(AtomicBool::new(false));
    let stop_clone = stop.clone();
    let tx_log = tx.clone();
    let tx_state = tx;
    let log_path = crate::log::log_path(dir);
    let state_path = crate::state::state_path(dir);
    let id_log = chamber_id.clone();
    let id_state = chamber_id;
    std::thread::spawn(move || {
        let mut last_size = log_path.metadata().map(|m| m.len()).unwrap_or(0);
        let mut last_state = std::fs::read_to_string(&state_path).unwrap_or_default();
        loop {
            if stop_clone.load(Ordering::Relaxed) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
            if let Ok(bytes) = std::fs::read(&log_path) {
                if (bytes.len() as u64) > last_size {
                    let new_bytes = &bytes[last_size as usize..];
                    for line in new_bytes.split(|b| *b == b'\n') {
                        if !line.is_empty() {
                            let text = String::from_utf8_lossy(line).into_owned();
                            if !text.trim().is_empty() {
                                let _ = tx_log.send(SseEvent::LogLine {
                                    chamber_id: id_log.clone(),
                                    line: text,
                                });
                            }
                        }
                    }
                    last_size = bytes.len() as u64;
                }
            }
            if let Ok(content) = std::fs::read_to_string(&state_path) {
                if content != last_state {
                    let _ = tx_state.send(SseEvent::StatusChange {
                        chamber_id: id_state.clone(),
                    });
                    last_state = content;
                }
            }
        }
    });

    Some(Handle {
        _watcher: watcher,
        _stop: stop,
    })
}

#[cfg(test)]
#[path = "../unit_tests/hub/watchers.rs"]
mod tests;
