use super::*;

fn chamber_entry(id: &str, path: std::path::PathBuf, archived: bool) -> ChamberEntry {
    ChamberEntry {
        id: id.to_string(),
        name: id.to_string(),
        path,
        path_hint: None,
        config_error: None,
        running: false,
        agent_running: false,
        session: None,
        next_wake: None,
        next_wake_display: None,
        wake_imminent: false,
        has_open_question: false,
        task: None,
        last_message_preview: None,
        completed: false,
        archived,
        sync: Vec::new(),
    }
}

#[test]
fn watcher_targets_skip_archived_chambers() {
    let active_path = std::path::PathBuf::from("/tmp/active");
    let archived_path = std::path::PathBuf::from("/tmp/archived");
    let mut idx = ChamberIndex::new();
    idx.insert(
        "active".to_string(),
        chamber_entry("active", active_path.clone(), false),
    );
    idx.insert(
        "archived".to_string(),
        chamber_entry("archived", archived_path, true),
    );

    let (paths, entries) = watcher_targets(&idx);

    assert_eq!(
        paths,
        std::collections::BTreeSet::from([active_path.clone()])
    );
    assert_eq!(entries, vec![("active".to_string(), active_path)]);
}

#[test]
fn resolve_finds_known_chamber() {
    let dir = tempfile::tempdir().unwrap();
    let alpha = dir.path().join("alpha");
    std::fs::create_dir_all(&alpha).unwrap();
    let cfg = crate::config::CryoConfig::default();
    crate::config::save_config(&alpha.join("cryo.toml"), &cfg).unwrap();

    let state = AppState::local_only(dir.path().to_path_buf());
    state.refresh();
    let id = crate::hub::discovery::encode_id(&alpha.canonicalize().unwrap());
    let resolved = state.resolve(&id);
    assert!(resolved.is_some());
    let (path, entry) = resolved.unwrap();
    assert_eq!(path, alpha.canonicalize().unwrap());
    assert_eq!(entry.name, "alpha");
}

#[test]
fn resolve_returns_none_for_unknown_id() {
    let dir = tempfile::tempdir().unwrap();
    let state = AppState::local_only(dir.path().to_path_buf());
    assert!(state.resolve("nonexistent").is_none());
}
