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
    let state = AppState::new(dir.path().to_path_buf());
    assert!(state.resolve("nonexistent").is_none());
}
