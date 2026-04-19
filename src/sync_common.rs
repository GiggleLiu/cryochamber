//! Shared sync backend abstraction: summary types and lifecycle wrappers.
//! Two backends (gh, zulip) with near-identical verbs -- free functions are
//! enough; no trait needed.

use anyhow::Result;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SyncBackend {
    Gh,
    Zulip,
}

impl SyncBackend {
    pub fn as_str(&self) -> &'static str {
        match self {
            SyncBackend::Gh => "gh",
            SyncBackend::Zulip => "zulip",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "gh" => Some(SyncBackend::Gh),
            "zulip" => Some(SyncBackend::Zulip),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncSummary {
    pub backend: SyncBackend,
    pub configured: bool,
    pub installed: bool,
    pub running: bool,
    pub target: String,
    pub last_pushed_session: Option<u32>,
    pub log_tail_path: PathBuf,
}

pub fn summarize(backend: SyncBackend, dir: &Path) -> Option<SyncSummary> {
    match backend {
        SyncBackend::Gh => crate::gh_sync::summarize(dir),
        SyncBackend::Zulip => crate::zulip_sync::summarize(dir),
    }
}

pub fn summarize_all(dir: &Path) -> Vec<SyncSummary> {
    [SyncBackend::Gh, SyncBackend::Zulip]
        .into_iter()
        .filter_map(|b| summarize(b, dir))
        .collect()
}

// Lifecycle wrappers (implemented in Task B6).
pub fn start(_backend: SyncBackend, _dir: &Path) -> Result<()> {
    anyhow::bail!("not implemented")
}
pub fn stop(_backend: SyncBackend, _dir: &Path) -> Result<()> {
    anyhow::bail!("not implemented")
}
pub fn pull(_backend: SyncBackend, _dir: &Path) -> Result<()> {
    anyhow::bail!("not implemented")
}
pub fn push(_backend: SyncBackend, _dir: &Path) -> Result<()> {
    anyhow::bail!("not implemented")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_parse_roundtrip() {
        assert_eq!(SyncBackend::parse("gh"), Some(SyncBackend::Gh));
        assert_eq!(SyncBackend::parse("zulip"), Some(SyncBackend::Zulip));
        assert_eq!(SyncBackend::parse("nope"), None);
        assert_eq!(SyncBackend::Gh.as_str(), "gh");
        assert_eq!(SyncBackend::Zulip.as_str(), "zulip");
    }

    #[test]
    fn summarize_all_empty_for_unconfigured_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert!(summarize_all(dir.path()).is_empty());
    }

    #[test]
    fn summarize_all_returns_configured_backends() {
        let dir = tempfile::tempdir().unwrap();
        let state = crate::gh_sync::GhSyncState {
            repo: "alice/notes".into(),
            discussion_number: 7,
            discussion_node_id: "node".into(),
            last_read_cursor: None,
            self_login: None,
            last_pushed_session: Some(3),
        };
        crate::gh_sync::save_sync_state(&dir.path().join("gh-sync.json"), &state).unwrap();

        let summaries = summarize_all(dir.path());
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].backend, SyncBackend::Gh);
        assert_eq!(summaries[0].target, "alice/notes#7");
        assert_eq!(summaries[0].last_pushed_session, Some(3));
        assert!(!summaries[0].running);
    }
}
