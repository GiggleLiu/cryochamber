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
        self_login: None,
        last_pushed_session: None,
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
        self_login: None,
        last_pushed_session: None,
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
        self_login: None,
        last_pushed_session: None,
    };
    let (owner, repo) = state.owner_repo().unwrap();
    assert_eq!(owner, "GiggleLiu");
    assert_eq!(repo, "cryochamber");
}

#[test]
fn test_sync_state_new_fields_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gh-sync.json");

    let state = GhSyncState {
        repo: "owner/repo".to_string(),
        discussion_number: 5,
        discussion_node_id: "D_abc".to_string(),
        last_read_cursor: None,
        self_login: Some("mybot".to_string()),
        last_pushed_session: Some(3),
    };
    save_sync_state(&path, &state).unwrap();
    let loaded = load_sync_state(&path).unwrap().unwrap();
    assert_eq!(loaded.self_login, Some("mybot".to_string()));
    assert_eq!(loaded.last_pushed_session, Some(3));
}

#[test]
fn test_sync_state_legacy_json_compat() {
    // Simulate a gh-sync.json from before the new fields were added
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gh-sync.json");
    std::fs::write(
        &path,
        r#"{"repo":"owner/repo","discussion_number":1,"discussion_node_id":"D_x"}"#,
    )
    .unwrap();
    let loaded = load_sync_state(&path).unwrap().unwrap();
    assert_eq!(loaded.self_login, None);
    assert_eq!(loaded.last_pushed_session, None);
}

#[test]
fn test_ensure_self_login_fills_legacy_state() {
    let mut state = GhSyncState {
        repo: "owner/repo".to_string(),
        discussion_number: 1,
        discussion_node_id: "D_abc".to_string(),
        last_read_cursor: None,
        self_login: None,
        last_pushed_session: None,
    };

    let changed = state
        .ensure_self_login_with(|| Ok("mybot".to_string()))
        .unwrap();

    assert!(changed);
    assert_eq!(state.self_login, Some("mybot".to_string()));
}

#[test]
fn test_ensure_self_login_keeps_existing_value() {
    let mut state = GhSyncState {
        repo: "owner/repo".to_string(),
        discussion_number: 1,
        discussion_node_id: "D_abc".to_string(),
        last_read_cursor: None,
        self_login: Some("existing".to_string()),
        last_pushed_session: None,
    };

    let changed = state
        .ensure_self_login_with(|| Ok("different".to_string()))
        .unwrap();

    assert!(!changed);
    assert_eq!(state.self_login, Some("existing".to_string()));
}

#[test]
fn test_status_lines_include_user_and_last_pushed_session() {
    let state = GhSyncState {
        repo: "owner/repo".to_string(),
        discussion_number: 9,
        discussion_node_id: "D_abc".to_string(),
        last_read_cursor: Some("read-marker".to_string()),
        self_login: Some("mybot".to_string()),
        last_pushed_session: Some(12),
    };

    assert_eq!(
        state.status_lines(),
        vec![
            "Repo: owner/repo".to_string(),
            "Discussion: #9".to_string(),
            "GitHub user: mybot".to_string(),
            "Last read position: read-marker".to_string(),
            "Last pushed session: 12".to_string(),
        ]
    );
}

#[test]
fn test_status_lines_show_missing_values_plainly() {
    let state = GhSyncState {
        repo: "owner/repo".to_string(),
        discussion_number: 9,
        discussion_node_id: "D_abc".to_string(),
        last_read_cursor: None,
        self_login: None,
        last_pushed_session: None,
    };

    assert_eq!(
        state.status_lines(),
        vec![
            "Repo: owner/repo".to_string(),
            "Discussion: #9".to_string(),
            "GitHub user: (unknown)".to_string(),
            "Last read position: (none - will read all)".to_string(),
            "Last pushed session: (none)".to_string(),
        ]
    );
}
