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
fn discover_with_options_merges_registered_chambers() {
    let workspace = tempfile::tempdir().unwrap();
    let registry_root = tempfile::tempdir().unwrap();
    let local = workspace.path().join("local");
    let registered = registry_root.path().join("registered");
    std::fs::create_dir_all(&local).unwrap();
    std::fs::create_dir_all(&registered).unwrap();
    let cfg = crate::config::CryoConfig::default();
    crate::config::save_config(&local.join("cryo.toml"), &cfg).unwrap();
    crate::config::save_config(&registered.join("cryo.toml"), &cfg).unwrap();

    let registry_path = workspace.path().join("known-chambers.json");
    crate::chamber_registry::record_at(&registry_path, &registered).unwrap();

    let idx = discover_with_options(
        workspace.path(),
        DiscoveryOptions {
            include_chamber_registry: true,
            chamber_registry_path: Some(registry_path),
        },
    );

    assert_eq!(idx.len(), 2);
    let local_entry = idx.values().find(|e| e.name == "local").unwrap();
    let registered_entry = idx.values().find(|e| e.name == "registered").unwrap();
    assert!(local_entry.workspace_local);
    assert!(!registered_entry.workspace_local);
}

#[test]
fn discover_with_options_deduplicates_registered_workspace_chambers() {
    let workspace = tempfile::tempdir().unwrap();
    let local = workspace.path().join("local");
    std::fs::create_dir_all(&local).unwrap();
    let cfg = crate::config::CryoConfig::default();
    crate::config::save_config(&local.join("cryo.toml"), &cfg).unwrap();

    let registry_path = workspace.path().join("known-chambers.json");
    crate::chamber_registry::record_at(&registry_path, &local).unwrap();

    let idx = discover_with_options(
        workspace.path(),
        DiscoveryOptions {
            include_chamber_registry: true,
            chamber_registry_path: Some(registry_path),
        },
    );

    assert_eq!(idx.len(), 1);
    let entry = idx.values().next().unwrap();
    assert_eq!(entry.name, "local");
    assert!(entry.workspace_local);
}

#[test]
fn discover_with_options_local_only_ignores_registered_chambers() {
    let workspace = tempfile::tempdir().unwrap();
    let registry_root = tempfile::tempdir().unwrap();
    let local = workspace.path().join("local");
    let registered = registry_root.path().join("registered");
    std::fs::create_dir_all(&local).unwrap();
    std::fs::create_dir_all(&registered).unwrap();
    let cfg = crate::config::CryoConfig::default();
    crate::config::save_config(&local.join("cryo.toml"), &cfg).unwrap();
    crate::config::save_config(&registered.join("cryo.toml"), &cfg).unwrap();

    let registry_path = workspace.path().join("known-chambers.json");
    crate::chamber_registry::record_at(&registry_path, &registered).unwrap();

    let idx = discover_with_options(
        workspace.path(),
        DiscoveryOptions {
            include_chamber_registry: false,
            chamber_registry_path: Some(registry_path),
        },
    );

    assert_eq!(idx.len(), 1);
    assert_eq!(idx.values().next().unwrap().name, "local");
}

#[test]
fn scan_skips_subdirs_without_cryo_toml() {
    // Stray non-chamber directories (e.g. an accidental `messages/` from a
    // mis-targeted `cryo init`) must not pollute the chamber rail.
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("messages").join("inbox")).unwrap();
    std::fs::create_dir_all(dir.path().join("notes")).unwrap();
    let idx = scan_workspace(dir.path());
    assert!(idx.is_empty());
}

#[test]
fn scan_flags_malformed_cryo_toml_as_error() {
    // A directory that *attempted* to be a chamber (has cryo.toml) but
    // fails to parse is still surfaced so the operator can see the breakage.
    let dir = tempfile::tempdir().unwrap();
    let chamber = dir.path().join("broken");
    std::fs::create_dir_all(&chamber).unwrap();
    std::fs::write(chamber.join("cryo.toml"), "this is = not valid toml [[").unwrap();
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
        agent_override: None,
        max_session_duration_override: None,
        last_report_time: None,
        instance_id: None,
        session_active: false,
        previous_session_crashed: false,
    };
    crate::state::save_state(&crate::state::state_path(&alpha), &st).unwrap();

    // Outbox carries an open question; inbox is empty so the question is unanswered.
    crate::message::ensure_dirs(&alpha).unwrap();
    let msg = crate::message::Message {
        from: "agent".into(),
        subject: "hi".into(),
        body: "yo?".into(),
        timestamp: chrono::Local::now().naive_local(),
        metadata: Default::default(),
        is_question: true,
    };
    crate::message::write_message(&alpha, "outbox", &msg).unwrap();

    let mut idx = scan_workspace(dir.path());
    populate_runtime(&mut idx);
    let entry = idx.values().next().unwrap();
    assert_eq!(entry.session, Some(7));
    assert!(entry.has_open_question);
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
        is_question: false,
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

#[test]
fn populate_runtime_reports_agent_running_when_session_active() {
    let dir = tempfile::tempdir().unwrap();
    let chamber = dir.path().join("alpha");
    std::fs::create_dir_all(&chamber).unwrap();
    let cfg = crate::config::CryoConfig::default();
    crate::config::save_config(&chamber.join("cryo.toml"), &cfg).unwrap();

    let st = crate::state::CryoState {
        session_number: 1,
        pid: Some(std::process::id()),
        agent_override: None,
        max_session_duration_override: None,
        last_report_time: None,
        instance_id: None,
        previous_session_crashed: false,
        session_active: true,
    };
    crate::state::save_state(&crate::state::state_path(&chamber), &st).unwrap();

    let mut idx = scan_workspace(dir.path());
    populate_runtime(&mut idx);
    let entry = idx.values().next().expect("one chamber");
    assert!(entry.running);
    assert!(entry.agent_running);
    let value = serde_json::to_value(entry).unwrap();
    assert_eq!(value["agent_running"], true);
}

#[test]
fn populate_runtime_reports_agent_running_false_when_idle() {
    let dir = tempfile::tempdir().unwrap();
    let chamber = dir.path().join("beta");
    std::fs::create_dir_all(&chamber).unwrap();
    let cfg = crate::config::CryoConfig::default();
    crate::config::save_config(&chamber.join("cryo.toml"), &cfg).unwrap();

    let st = crate::state::CryoState {
        session_number: 1,
        pid: Some(std::process::id()),
        agent_override: None,
        max_session_duration_override: None,
        last_report_time: None,
        instance_id: None,
        previous_session_crashed: false,
        session_active: false,
    };
    crate::state::save_state(&crate::state::state_path(&chamber), &st).unwrap();

    let mut idx = scan_workspace(dir.path());
    populate_runtime(&mut idx);
    let entry = idx.values().next().expect("one chamber");
    assert!(entry.running);
    assert!(!entry.agent_running);
}
