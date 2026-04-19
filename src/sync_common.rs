//! Shared sync backend abstraction: summary types and lifecycle wrappers.
//! Two backends (gh, zulip) with near-identical verbs -- free functions are
//! enough; no trait needed.

use anyhow::{Context, Result};
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

fn resolve_cli(backend: SyncBackend) -> Result<std::path::PathBuf> {
    let (env_var, bin_name) = match backend {
        SyncBackend::Gh => ("CRYO_GH_CLI", "cryo-gh"),
        SyncBackend::Zulip => ("CRYO_ZULIP_CLI", "cryo-zulip"),
    };
    if let Ok(p) = std::env::var(env_var) {
        return Ok(std::path::PathBuf::from(p));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let sibling = parent.join(bin_name);
            if sibling.exists() {
                return Ok(sibling);
            }
        }
    }
    if let Ok(output) = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {bin_name}"))
        .output()
    {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return Ok(std::path::PathBuf::from(path));
            }
        }
    }
    anyhow::bail!("{bin_name} binary not found (tried ${env_var}, sibling of current exe, $PATH)");
}

fn run_subcommand(backend: SyncBackend, dir: &Path, sub: &str) -> Result<()> {
    let cli = resolve_cli(backend)?;
    let output = std::process::Command::new(&cli)
        .current_dir(dir)
        .arg(sub)
        .output()
        .with_context(|| format!("Failed to spawn {}", cli.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let truncated: String = stderr.chars().take(500).collect();
        anyhow::bail!(
            "{} {sub} exited with {}: {}",
            cli.display(),
            output.status,
            truncated.trim()
        );
    }
    Ok(())
}

pub fn start(backend: SyncBackend, dir: &Path) -> Result<()> {
    run_subcommand(backend, dir, "sync")
}
pub fn stop(backend: SyncBackend, dir: &Path) -> Result<()> {
    run_subcommand(backend, dir, "unsync")
}
pub fn pull(backend: SyncBackend, dir: &Path) -> Result<()> {
    run_subcommand(backend, dir, "pull")
}
pub fn push(backend: SyncBackend, dir: &Path) -> Result<()> {
    run_subcommand(backend, dir, "push")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn make_stub(dir: &Path, name: &str, exit_code: i32, stdout: &str) -> std::path::PathBuf {
        let p = dir.join(name);
        let mut f = std::fs::File::create(&p).unwrap();
        writeln!(f, "#!/bin/sh").unwrap();
        writeln!(f, "echo {stdout}").unwrap();
        writeln!(f, "exit {exit_code}").unwrap();
        let mut perms = std::fs::metadata(&p).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&p, perms).unwrap();
        p
    }

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

    #[test]
    fn start_invokes_sync_subcommand_via_env_override() {
        let _guard = ENV_LOCK.lock().unwrap();
        let bin = tempfile::tempdir().unwrap();
        let work = tempfile::tempdir().unwrap();
        let stub = make_stub(bin.path(), "cryo-gh-stub", 0, "ok");
        std::env::set_var("CRYO_GH_CLI", &stub);
        let res = start(SyncBackend::Gh, work.path());
        std::env::remove_var("CRYO_GH_CLI");
        assert!(res.is_ok(), "{res:?}");
    }

    #[test]
    fn stop_propagates_non_zero_exit_as_error() {
        let _guard = ENV_LOCK.lock().unwrap();
        let bin = tempfile::tempdir().unwrap();
        let work = tempfile::tempdir().unwrap();
        let stub = make_stub(bin.path(), "cryo-gh-stub", 7, "boom");
        std::env::set_var("CRYO_GH_CLI", &stub);
        let res = stop(SyncBackend::Gh, work.path());
        std::env::remove_var("CRYO_GH_CLI");
        assert!(res.is_err());
    }

    #[test]
    fn pull_and_push_use_zulip_env_override() {
        let _guard = ENV_LOCK.lock().unwrap();
        let bin = tempfile::tempdir().unwrap();
        let work = tempfile::tempdir().unwrap();
        let stub = make_stub(bin.path(), "cryo-zulip-stub", 0, "ok");
        std::env::set_var("CRYO_ZULIP_CLI", &stub);
        assert!(pull(SyncBackend::Zulip, work.path()).is_ok());
        assert!(push(SyncBackend::Zulip, work.path()).is_ok());
        std::env::remove_var("CRYO_ZULIP_CLI");
    }
}
