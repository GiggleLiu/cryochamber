//! Shared application state for the web server.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use crate::web::discovery::{ChamberEntry, ChamberIndex};

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
    },
    StatusChange {
        chamber_id: String,
    },
    LogLine {
        chamber_id: String,
        line: String,
    },
    /// Workspace-level refresh — chambers list changed (added/removed).
    IndexChanged,
}

pub struct AppState {
    pub workspace_dir: PathBuf,
    pub chambers: Arc<RwLock<ChamberIndex>>,
    pub tx: tokio::sync::broadcast::Sender<SseEvent>,
}

impl AppState {
    pub fn new(workspace_dir: PathBuf) -> Self {
        let (tx, _rx) = tokio::sync::broadcast::channel::<SseEvent>(256);
        Self {
            workspace_dir,
            chambers: Arc::new(RwLock::new(ChamberIndex::new())),
            tx,
        }
    }

    /// Resolve an id to `(path, ChamberEntry)` if the id refers to a known
    /// chamber in the current index.
    pub fn resolve(&self, id: &str) -> Option<(PathBuf, ChamberEntry)> {
        let idx = self.chambers.read().ok()?;
        idx.get(id).map(|e| (e.path.clone(), e.clone()))
    }

    /// Overwrite the chamber index with a fresh discovery pass.
    pub fn refresh(&self) {
        let fresh = crate::web::discovery::discover(&self.workspace_dir);
        if let Ok(mut idx) = self.chambers.write() {
            *idx = fresh;
        }
        let _ = self.tx.send(SseEvent::IndexChanged);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_finds_known_chamber() {
        let dir = tempfile::tempdir().unwrap();
        let chambers = dir.path().join("chambers");
        let alpha = chambers.join("alpha");
        std::fs::create_dir_all(&alpha).unwrap();
        let cfg = crate::config::CryoConfig::default();
        crate::config::save_config(&alpha.join("cryo.toml"), &cfg).unwrap();

        let state = AppState::new(dir.path().to_path_buf());
        state.refresh();
        let id = crate::web::discovery::encode_id(&alpha.canonicalize().unwrap());
        let resolved = state.resolve(&id);
        assert!(resolved.is_some());
        let (path, entry) = resolved.unwrap();
        assert_eq!(path, alpha.canonicalize().unwrap());
        assert_eq!(entry.name, "alpha");
    }

    #[test]
    fn resolve_returns_none_for_unknown_id() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::new(dir.path().to_path_buf());
        assert!(state.resolve("nonexistent").is_none());
    }
}
