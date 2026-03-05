use cryochamber::zulip_sync::{load_sync_state, save_sync_state, ZulipSyncState};

#[test]
fn test_zulip_sync_state_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("zulip-sync.json");

    let state = ZulipSyncState {
        site: "https://zulip.example.com".to_string(),
        stream: "cryochamber".to_string(),
        stream_id: 42,
        self_email: "bot@example.com".to_string(),
        topic: Some("my-project".to_string()),
        last_message_id: Some(12345),
        last_pushed_session: Some(3),
    };
    save_sync_state(&path, &state).unwrap();
    let loaded = load_sync_state(&path).unwrap().unwrap();

    assert_eq!(loaded.site, "https://zulip.example.com");
    assert_eq!(loaded.stream, "cryochamber");
    assert_eq!(loaded.stream_id, 42);
    assert_eq!(loaded.self_email, "bot@example.com");
    assert_eq!(loaded.topic, Some("my-project".to_string()));
    assert_eq!(loaded.last_message_id, Some(12345));
    assert_eq!(loaded.last_pushed_session, Some(3));
}

#[test]
fn test_zulip_sync_state_missing_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("zulip-sync.json");
    let loaded = load_sync_state(&path).unwrap();
    assert!(loaded.is_none());
}

#[test]
fn test_zulip_sync_state_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("zulip-sync.json");

    let state = ZulipSyncState {
        site: "https://z.example.com".to_string(),
        stream: "test".to_string(),
        stream_id: 1,
        self_email: "bot@z.example.com".to_string(),
        topic: None,
        last_message_id: None,
        last_pushed_session: None,
    };
    save_sync_state(&path, &state).unwrap();
    let loaded = load_sync_state(&path).unwrap().unwrap();
    assert!(loaded.topic.is_none());
    assert!(loaded.last_message_id.is_none());
    assert!(loaded.last_pushed_session.is_none());
}

#[test]
fn test_zulip_sync_state_legacy_json_compat() {
    // Simulate a zulip-sync.json without optional fields
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("zulip-sync.json");
    std::fs::write(
        &path,
        r#"{"site":"https://z.example.com","stream":"test","stream_id":1,"self_email":"bot@z.example.com"}"#,
    )
    .unwrap();
    let loaded = load_sync_state(&path).unwrap().unwrap();
    assert!(loaded.topic.is_none());
    assert!(loaded.last_message_id.is_none());
    assert!(loaded.last_pushed_session.is_none());
}
