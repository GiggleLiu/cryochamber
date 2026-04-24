use super::*;

#[test]
fn encode_decode_round_trip() {
    let path = PathBuf::from("/Users/alice/work space/chambers/my chamber");
    let id = encode_id(&path);
    assert!(!id.contains(' '), "id must be URL-safe");
    assert!(!id.contains('/'), "id must not contain raw slashes");
    let back = decode_id(&id).unwrap();
    assert_eq!(back, path);
}

#[test]
fn decode_rejects_invalid() {
    // %ZZ is not valid percent-encoding
    assert!(decode_id("%ZZ").is_none());
}

#[test]
fn scan_empty_workspace_returns_empty_index() {
    let dir = tempfile::tempdir().unwrap();
    let idx = scan_workspace(dir.path());
    assert!(idx.is_empty());
}

#[test]
fn scan_finds_chambers_with_valid_config() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("alpha")).unwrap();
    std::fs::create_dir_all(dir.path().join("beta")).unwrap();
    let cfg = crate::config::CryoConfig::default();
    crate::config::save_config(&dir.path().join("alpha").join("cryo.toml"), &cfg).unwrap();
    crate::config::save_config(&dir.path().join("beta").join("cryo.toml"), &cfg).unwrap();

    let idx = scan_workspace(dir.path());
    assert_eq!(idx.len(), 2);
    let names: Vec<_> = idx.values().map(|e| e.name.clone()).collect();
    assert!(names.contains(&"alpha".to_string()));
    assert!(names.contains(&"beta".to_string()));
    for entry in idx.values() {
        assert!(entry.config_error.is_none());
    }
}

#[test]
fn scan_flags_missing_cryo_toml_as_error() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("broken")).unwrap();
    let idx = scan_workspace(dir.path());
    assert_eq!(idx.len(), 1);
    let entry = idx.values().next().unwrap();
    assert!(entry.config_error.is_some());
}

#[test]
fn populate_reads_session_and_unread() {
    let dir = tempfile::tempdir().unwrap();
    let alpha = dir.path().join("alpha");
    std::fs::create_dir_all(&alpha).unwrap();
    let cfg = crate::config::CryoConfig::default();
    crate::config::save_config(&alpha.join("cryo.toml"), &cfg).unwrap();

    // Fake runtime state: session 7, not locked (no live PID)
    let st = crate::state::CryoState {
        session_number: 7,
        pid: None,
        retry_count: 0,
        agent_override: None,
        max_session_duration_override: None,
        last_report_time: None,
        provider_index: None,
        instance_id: None,
        session_active: false,
        previous_session_crashed: false,
    };
    crate::state::save_state(&crate::state::state_path(&alpha), &st).unwrap();

    // Fake inbox with one message
    crate::message::ensure_dirs(&alpha).unwrap();
    let msg = crate::message::Message {
        from: "tester".into(),
        subject: "hi".into(),
        body: "yo".into(),
        timestamp: chrono::Local::now().naive_local(),
        metadata: Default::default(),
    };
    crate::message::write_message(&alpha, "inbox", &msg).unwrap();

    let mut idx = scan_workspace(dir.path());
    populate_runtime(&mut idx);
    let entry = idx.values().next().unwrap();
    assert_eq!(entry.session, Some(7));
    assert_eq!(entry.unread, 1);
    assert!(!entry.running, "no live pid -> not running");
}

#[test]
fn populate_reports_configured_gh_sync() {
    let dir = tempfile::tempdir().unwrap();
    let alpha = dir.path().join("alpha");
    std::fs::create_dir_all(&alpha).unwrap();
    let cfg = crate::config::CryoConfig::default();
    crate::config::save_config(&alpha.join("cryo.toml"), &cfg).unwrap();
    let state = crate::gh_sync::GhSyncState {
        repo: "a/b".into(),
        discussion_number: 1,
        discussion_node_id: "n".into(),
        last_read_cursor: None,
        self_login: None,
        last_pushed_session: None,
    };
    crate::gh_sync::save_sync_state(&alpha.join("gh-sync.json"), &state).unwrap();

    let mut idx = scan_workspace(dir.path());
    populate_runtime(&mut idx);
    let entry = idx.values().next().unwrap();
    assert_eq!(entry.sync.len(), 1);
    assert_eq!(entry.sync[0].backend, "gh");
    assert!(!entry.sync[0].running);
}

#[test]
fn populate_runtime_exposes_rail_display_fields() {
    let dir = tempfile::tempdir().unwrap();
    let alpha = dir.path().join("alpha");
    std::fs::create_dir_all(&alpha).unwrap();
    let cfg = crate::config::CryoConfig::default();
    crate::config::save_config(&alpha.join("cryo.toml"), &cfg).unwrap();
    std::fs::write(
        alpha.join("cryo.log"),
        "--- CRYO SESSION 1 | 2026-01-01T00:00:00Z ---\n\
         task: Review inbox\n\
         agent: true\n",
    )
    .unwrap();

    crate::todo::TodoFile::new(alpha.join("todo.json"))
        .add("next step".into(), "2099-05-01T10:00".into())
        .unwrap();

    crate::message::ensure_dirs(&alpha).unwrap();
    let msg = crate::message::Message {
        from: "tester".into(),
        subject: "preview".into(),
        body: "hello preview\nsecond line".into(),
        timestamp: chrono::NaiveDate::from_ymd_opt(2026, 1, 1)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap(),
        metadata: Default::default(),
    };
    crate::message::write_message(&alpha, "inbox", &msg).unwrap();

    let mut idx = scan_workspace(dir.path());
    populate_runtime(&mut idx);
    let entry = idx.values().next().unwrap();
    let value = serde_json::to_value(entry).unwrap();

    assert_eq!(value["task"], "Review inbox");
    assert_eq!(value["next_wake_display"], "2099-05-01T10:00");
    assert_eq!(value["wake_imminent"], false);
    assert_eq!(value["last_message_preview"], "hello preview");
}
