use cryochamber::gh_sync::{load_sync_state, save_sync_state, GhSyncState};

#[test]
fn test_sync_state_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gh-sync.json");

    let state = GhSyncState {
        repo: "owner/repo".to_string(),
        discussion_number: 42,
        discussion_node_id: "D_kwDOtest".to_string(),
        last_read_cursor: Some("Y3Vyc29y".to_string()),
    };
    save_sync_state(&path, &state).unwrap();
    let loaded = load_sync_state(&path).unwrap().unwrap();

    assert_eq!(loaded.repo, "owner/repo");
    assert_eq!(loaded.discussion_number, 42);
    assert_eq!(loaded.discussion_node_id, "D_kwDOtest");
    assert_eq!(loaded.last_read_cursor, Some("Y3Vyc29y".to_string()));
}

#[test]
fn test_sync_state_missing_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gh-sync.json");
    let loaded = load_sync_state(&path).unwrap();
    assert!(loaded.is_none());
}

#[test]
fn test_sync_state_no_cursor() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gh-sync.json");

    let state = GhSyncState {
        repo: "owner/repo".to_string(),
        discussion_number: 1,
        discussion_node_id: "D_abc".to_string(),
        last_read_cursor: None,
    };
    save_sync_state(&path, &state).unwrap();
    let loaded = load_sync_state(&path).unwrap().unwrap();
    assert!(loaded.last_read_cursor.is_none());
}

#[test]
fn test_sync_state_owner_repo_split() {
    let state = GhSyncState {
        repo: "GiggleLiu/cryochamber".to_string(),
        discussion_number: 1,
        discussion_node_id: "D_abc".to_string(),
        last_read_cursor: None,
    };
    let (owner, repo) = state.owner_repo().unwrap();
    assert_eq!(owner, "GiggleLiu");
    assert_eq!(repo, "cryochamber");
}
