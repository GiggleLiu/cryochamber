use super::*;
use crate::test_support::EnvVarGuard;

fn scaffold_chamber(parent: &std::path::Path, name: &str) -> std::path::PathBuf {
    let dir = parent.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let cfg = crate::config::CryoConfig::default();
    crate::config::save_config(&dir.join("cryo.toml"), &cfg).unwrap();
    dir.canonicalize().unwrap()
}

#[test]
fn test_daemon_entry_has_socket_path() {
    let entry = DaemonEntry {
        pid: Some(1234),
        dir: "/tmp/test".to_string(),
        socket_path: Some("/tmp/test/.cryo/cryo.sock".to_string()),
    };
    let json = serde_json::to_string(&entry).unwrap();
    assert!(json.contains("cryo.sock"));
}

#[test]
fn registry_dir_uses_user_state_home() {
    let state_home = tempfile::tempdir().unwrap();
    let _guard = EnvVarGuard::set_path("XDG_STATE_HOME", state_home.path());

    let dir = registry_dir().unwrap();

    assert!(dir.starts_with(state_home.path()));
    assert!(dir.ends_with("cryo/chambers"));
}

#[test]
fn unregister_keeps_stopped_chamber_in_registry() {
    let state_home = tempfile::tempdir().unwrap();
    let _guard = EnvVarGuard::set_path("XDG_STATE_HOME", state_home.path());
    let workspace = tempfile::tempdir().unwrap();
    let chamber = scaffold_chamber(workspace.path(), "alpha");
    let socket = chamber.join(".cryo/cryo.sock");

    register(&chamber, Some(&socket)).unwrap();
    let running = list().unwrap();
    let entry = running
        .iter()
        .find(|entry| entry.dir == chamber.display().to_string())
        .unwrap();
    assert_eq!(entry.pid, Some(std::process::id()));
    assert_eq!(entry.socket_path, Some(socket.display().to_string()));

    unregister(&chamber);

    let stopped = list().unwrap();
    let entry = stopped
        .iter()
        .find(|entry| entry.dir == chamber.display().to_string())
        .unwrap();
    assert_eq!(entry.pid, None);
    assert_eq!(entry.socket_path, None);
}

#[test]
fn list_marks_dead_pid_as_stopped_without_forgetting_chamber() {
    let state_home = tempfile::tempdir().unwrap();
    let _guard = EnvVarGuard::set_path("XDG_STATE_HOME", state_home.path());
    let workspace = tempfile::tempdir().unwrap();
    let chamber = scaffold_chamber(workspace.path(), "alpha");
    let entry_path = registry_dir().unwrap().join(entry_filename(&chamber));
    let entry = DaemonEntry {
        pid: Some(999_999_999),
        dir: chamber.display().to_string(),
        socket_path: Some(chamber.join(".cryo/cryo.sock").display().to_string()),
    };
    std::fs::write(&entry_path, serde_json::to_string(&entry).unwrap()).unwrap();

    let entries = list().unwrap();

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].dir, chamber.display().to_string());
    assert_eq!(entries[0].pid, None);
    assert_eq!(entries[0].socket_path, None);
}

#[test]
fn list_prunes_registry_entries_for_missing_chambers() {
    let state_home = tempfile::tempdir().unwrap();
    let _guard = EnvVarGuard::set_path("XDG_STATE_HOME", state_home.path());
    let missing = state_home.path().join("gone");
    let entry_path = registry_dir().unwrap().join(entry_filename(&missing));
    let entry = DaemonEntry {
        pid: None,
        dir: missing.display().to_string(),
        socket_path: None,
    };
    std::fs::write(&entry_path, serde_json::to_string(&entry).unwrap()).unwrap();

    let entries = list().unwrap();

    assert!(entries.is_empty());
    assert!(!entry_path.exists());
}
