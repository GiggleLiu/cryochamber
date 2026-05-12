//! Shared application state for the web server.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

#[cfg(not(test))]
use std::collections::BTreeSet;

use crate::hub::discovery::{ChamberEntry, ChamberIndex, DiscoveryOptions};

/// SSE event broadcast to all connected clients. Every event carries
/// `chamber_id` so the sidebar (which listens to all events) and the detail
/// pane (which filters to one id) can route them.
#[derive(Clone, Debug)]
pub enum SseEvent {
    NewMessage {
        chamber_id: String,
        direction: String,
        from: String,
        subject: String,
        body: String,
        timestamp: String,
        is_question: bool,
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
    pub discovery_options: DiscoveryOptions,
    pub chambers: Arc<RwLock<ChamberIndex>>,
    pub tx: tokio::sync::broadcast::Sender<SseEvent>,
    pub watchers: crate::hub::watchers::WatcherRegistry,
}

impl AppState {
    pub fn global() -> Self {
        let chamber_root = crate::hub::config::load_config()
            .map(|config| config.chamber_root)
            .unwrap_or_else(|_| crate::hub::paths::global_chambers_dir());
        Self::with_discovery_options(chamber_root, DiscoveryOptions::all_chambers())
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
        let (tx, _rx) = tokio::sync::broadcast::channel::<SseEvent>(256);
        Self {
            workspace_dir,
            discovery_options,
            chambers: Arc::new(RwLock::new(ChamberIndex::new())),
            tx,
            watchers: crate::hub::watchers::WatcherRegistry::new(),
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
            let paths: BTreeSet<PathBuf> = idx.values().map(|e| e.path.clone()).collect();
            let entries: Vec<(String, PathBuf)> = idx
                .values()
                .map(|e| (e.id.clone(), e.path.clone()))
                .collect();
            (paths, entries)
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
