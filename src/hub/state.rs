//! Shared application state for the web server.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use crate::hub::discovery::{ChamberEntry, ChamberIndex, DiscoveryOptions};

/// SSE event broadcast to all connected clients. Every event carries
/// `chamber_id` so the sidebar (which listens to all events) and the detail
/// pane (which filters to one id) can route them.
#[derive(Clone, Debug)]
pub enum SseEvent {
    NewMessage {
        /// Mailbox id of the message, identical to the `id` the messages list
        /// (`crate::chamber_status::messages`) reports for the same file — see
        /// [`crate::chamber_status::message_id`]. Clients dedupe on it.
        id: String,
        chamber_id: String,
        direction: String,
        from: String,
        subject: String,
        body: String,
        timestamp: String,
        is_question: bool,
        thread_id: Option<String>,
        shared_from: Option<String>,
    },
    StatusChange {
        chamber_id: String,
    },
    LogLine {
        chamber_id: String,
        line: String,
    },
    /// Index-level refresh — chambers list changed (added/removed).
    IndexChanged,
}

pub struct AppState {
    pub workspace_dir: PathBuf,
    pub default_agent: RwLock<String>,
    pub discovery_options: DiscoveryOptions,
    pub chambers: Arc<RwLock<ChamberIndex>>,
    pub tx: tokio::sync::broadcast::Sender<SseEvent>,
    pub watchers: crate::hub::watchers::WatcherRegistry,
    /// Per-credential throttle for the two routes a guest may write through.
    pub write_limiter: crate::hub::ratelimit::RateLimiter,
}

fn watcher_targets(idx: &ChamberIndex) -> (BTreeSet<PathBuf>, Vec<(String, PathBuf)>) {
    let entries: Vec<(String, PathBuf)> = idx
        .values()
        .filter(|entry| !entry.archived)
        .map(|entry| (entry.id.clone(), entry.path.clone()))
        .collect();
    let paths = entries.iter().map(|(_, path)| path.clone()).collect();
    (paths, entries)
}

impl AppState {
    pub fn global() -> Self {
        let config = crate::hub::config::load_config().unwrap_or_default();
        Self::with_discovery_options_and_agent(
            config.chamber_root,
            DiscoveryOptions::all_chambers(),
            config.default_agent,
        )
    }

    pub fn new(workspace_dir: PathBuf) -> Self {
        Self::with_discovery_options(workspace_dir, DiscoveryOptions::all_chambers())
    }

    pub fn local_only(workspace_dir: PathBuf) -> Self {
        Self::with_discovery_options(workspace_dir, DiscoveryOptions::local_only())
    }

    pub fn with_discovery_options(
        workspace_dir: PathBuf,
        discovery_options: DiscoveryOptions,
    ) -> Self {
        Self::with_discovery_options_and_agent(
            workspace_dir,
            discovery_options,
            crate::config::default_agent(),
        )
    }

    pub fn with_discovery_options_and_agent(
        workspace_dir: PathBuf,
        discovery_options: DiscoveryOptions,
        default_agent: String,
    ) -> Self {
        let (tx, _rx) = tokio::sync::broadcast::channel::<SseEvent>(256);
        Self {
            workspace_dir,
            default_agent: RwLock::new(default_agent),
            discovery_options,
            chambers: Arc::new(RwLock::new(ChamberIndex::new())),
            tx,
            watchers: crate::hub::watchers::WatcherRegistry::new(),
            write_limiter: crate::hub::ratelimit::RateLimiter::new(
                crate::hub::ratelimit::WRITE_BURST,
                crate::hub::ratelimit::WRITE_REFILL_PER_MIN,
            ),
        }
    }

    /// Resolve an id to `(path, ChamberEntry)` if the id refers to a known
    /// chamber in the current index.
    ///
    /// The `id` may arrive either as the raw percent-encoded form (the key
    /// stored in the index) or as the decoded absolute path string (because
    /// axum's `Path` extractor percent-decodes path parameters before calling
    /// the handler). Both forms are tried.
    pub fn resolve(&self, id: &str) -> Option<(PathBuf, ChamberEntry)> {
        let idx = self.chambers.read().ok()?;
        // Fast path: id is already in encoded form (direct key lookup).
        if let Some(e) = idx.get(id) {
            return Some((e.path.clone(), e.clone()));
        }
        // Slow path: axum decoded the percent-encoding, so re-encode and retry.
        let re_encoded = crate::hub::discovery::encode_id(std::path::Path::new(id));
        idx.get(&re_encoded).map(|e| (e.path.clone(), e.clone()))
    }

    /// Overwrite the chamber index with a fresh discovery pass.
    pub fn refresh(&self) {
        let fresh = crate::hub::discovery::discover_with_options(
            &self.workspace_dir,
            self.discovery_options,
        );
        if let Ok(mut idx) = self.chambers.write() {
            *idx = fresh;
        }
        let _ = self.tx.send(SseEvent::IndexChanged);
        self.wire_watchers();
    }

    /// One pass of the registry staleness check: refresh the index when the
    /// registry's content fingerprint moved since the previous pass. Split
    /// from the watch thread so tests can drive it directly.
    pub fn registry_poll_once(&self, last: &mut u64) {
        let fingerprint = crate::registry::fingerprint();
        if fingerprint != *last {
            *last = fingerprint;
            self.refresh();
        }
    }

    /// Follow registry changes made by other processes (a terminal `cryo
    /// start` writes its own registry entry) so the console learns about new
    /// chambers on its own instead of needing Settings → Refresh chambers.
    /// A 1s content-fingerprint poll, not a notify watcher: `refresh()` itself
    /// rewrites entries (pid repair in `registry::list`), and a fingerprint
    /// makes those self-writes compare equal instead of looping forever.
    #[cfg(not(test))]
    pub fn spawn_registry_watch(self: &Arc<Self>) {
        if !self.discovery_options.include_registry {
            return;
        }
        let app = Arc::downgrade(self);
        std::thread::spawn(move || {
            let mut last = crate::registry::fingerprint();
            loop {
                std::thread::sleep(std::time::Duration::from_secs(1));
                let Some(app) = app.upgrade() else {
                    return;
                };
                app.registry_poll_once(&mut last);
            }
        });
    }

    /// Tests drive `registry_poll_once` directly; no background threads from
    /// route/state tests.
    #[cfg(test)]
    pub fn spawn_registry_watch(self: &Arc<Self>) {}

    /// Synchronise the watcher registry with the current chamber index:
    /// start watchers for any new chambers and stop watchers for removed ones.
    /// Tests that populate the index directly should call this after writing.
    #[cfg(test)]
    pub fn wire_watchers(&self) {
        // Unit tests exercise WatcherRegistry directly. Avoid starting real OS
        // watchers from unrelated route/state tests, since they are
        // process-global resources and make the parallel test harness flaky.
    }

    /// Synchronise the watcher registry with the current chamber index:
    /// start watchers for any new chambers and stop watchers for removed ones.
    /// Tests that populate the index directly should call this after writing.
    #[cfg(not(test))]
    pub fn wire_watchers(&self) {
        let (paths, entries): (BTreeSet<PathBuf>, Vec<(String, PathBuf)>) = {
            let idx = self.chambers.read().unwrap();
            watcher_targets(&idx)
        };
        for (id, path) in entries {
            self.watchers.ensure_watching(id, &path, self.tx.clone());
        }
        self.watchers.retain(&paths);
    }
}

#[cfg(test)]
#[path = "../unit_tests/hub/state.rs"]
mod tests;
