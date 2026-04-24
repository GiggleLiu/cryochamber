// tests/state_tests.rs
use cryochamber::state::{load_state, save_state, CryoState};

#[test]
fn test_save_and_load_state() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("timer.json");

    let state = CryoState {
        session_number: 3,
        pid: Some(std::process::id()),
        retry_count: 0,
        agent_override: Some("opencode test".to_string()),
        max_session_duration_override: None,

        last_report_time: None,
        provider_index: None,
        instance_id: None,
        session_active: false,
        previous_session_crashed: false,
        session_active: false,
    };

    save_state(&state_path, &state).unwrap();
    let loaded = load_state(&state_path).unwrap().unwrap();

    assert_eq!(loaded.session_number, 3);
    assert_eq!(loaded.agent_override, Some("opencode test".to_string()));
    assert_eq!(loaded.retry_count, 0);
}

#[test]
fn test_load_missing_state() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("nonexistent.json");
    let loaded = load_state(&state_path).unwrap();
    assert!(loaded.is_none());
}

#[test]
fn test_lock_mechanism() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("timer.json");

    let state = CryoState {
        session_number: 1,
        pid: Some(std::process::id()),
        retry_count: 0,
        agent_override: None,
        max_session_duration_override: None,

        last_report_time: None,
        provider_index: None,
        instance_id: None,
        session_active: false,
        previous_session_crashed: false,
        session_active: false,
    };
    save_state(&state_path, &state).unwrap();

    // Current process PID should be considered "running"
    let loaded = load_state(&state_path).unwrap().unwrap();
    assert_eq!(loaded.pid, Some(std::process::id()));
}

#[test]
fn test_is_locked_dead_process() {
    use cryochamber::state::is_locked;
    // PID 999999 is very unlikely to exist
    let state = CryoState {
        session_number: 1,
        pid: Some(999999),
        retry_count: 0,
        agent_override: None,
        max_session_duration_override: None,

        last_report_time: None,
        provider_index: None,
        instance_id: None,
        session_active: false,
        previous_session_crashed: false,
        session_active: false,
    };
    assert!(!is_locked(&state));
}

#[test]
fn test_is_locked_no_pid() {
    use cryochamber::state::is_locked;
    let state = CryoState {
        session_number: 1,
        pid: None,
        retry_count: 0,
        agent_override: None,
        max_session_duration_override: None,

        last_report_time: None,
        provider_index: None,
        instance_id: None,
        session_active: false,
        previous_session_crashed: false,
        session_active: false,
    };
    assert!(!is_locked(&state));
}

#[test]
fn test_load_empty_state_returns_none() {
    // Empty file should return None (handles truncate-then-write race)
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("timer.json");
    std::fs::write(&state_path, "").unwrap();
    let loaded = load_state(&state_path).unwrap();
    assert!(loaded.is_none());
}

#[test]
fn test_load_corrupted_state() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("timer.json");
    std::fs::write(&state_path, "not valid json {{{").unwrap();
    let result = load_state(&state_path);
    assert!(result.is_err());
}

#[test]
fn test_previous_session_crashed_default_false_and_skipped_when_false() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("timer.json");
    std::fs::write(&state_path, r#"{"session_number": 1}"#).unwrap();
    let loaded = load_state(&state_path).unwrap().unwrap();
    assert!(!loaded.previous_session_crashed, "default must be false");

    let state = CryoState {
        session_number: 1,
        pid: None,
        retry_count: 0,
        agent_override: None,
        max_session_duration_override: None,
        last_report_time: None,
        provider_index: None,
        instance_id: None,
        session_active: false,
        previous_session_crashed: false,
        session_active: false,
    };
    save_state(&state_path, &state).unwrap();
    let json = std::fs::read_to_string(&state_path).unwrap();
    assert!(
        !json.contains("previous_session_crashed"),
        "false should not be serialized"
    );
}

#[test]
fn test_previous_session_crashed_true_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("timer.json");
    let state = CryoState {
        session_number: 1,
        pid: None,
        retry_count: 0,
        agent_override: None,
        max_session_duration_override: None,
        last_report_time: None,
        provider_index: None,
        instance_id: None,
        session_active: false,
        previous_session_crashed: true,
        session_active: false,
    };
    save_state(&state_path, &state).unwrap();
    let loaded = load_state(&state_path).unwrap().unwrap();
    assert!(loaded.previous_session_crashed);
    let json = std::fs::read_to_string(&state_path).unwrap();
    assert!(json.contains("previous_session_crashed"));
}

#[test]
fn test_session_active_true_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("timer.json");
    std::fs::write(
        &state_path,
        r#"{
            "session_number": 1,
            "pid": null,
            "session_active": true
        }"#,
    )
    .unwrap();

    let loaded = load_state(&state_path).unwrap().unwrap();
    save_state(&state_path, &loaded).unwrap();

    let json = std::fs::read_to_string(&state_path).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["session_active"], true);
}

#[test]
fn test_load_minimal_state() {
    // Minimal JSON with only required fields — serde defaults should apply
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("timer.json");
    let minimal_json = r#"{
        "session_number": 5,
        "pid": null
    }"#;
    std::fs::write(&state_path, minimal_json).unwrap();
    let loaded = load_state(&state_path).unwrap().unwrap();
    assert_eq!(loaded.session_number, 5);
    assert_eq!(loaded.retry_count, 0); // default
    assert!(loaded.agent_override.is_none());
}

#[test]
fn test_override_fields_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("timer.json");
    let state = CryoState {
        session_number: 1,
        pid: None,
        retry_count: 2,
        agent_override: Some("claude".to_string()),
        max_session_duration_override: Some(1800),

        last_report_time: None,
        provider_index: None,
        instance_id: None,
        session_active: false,
        previous_session_crashed: false,
        session_active: false,
    };
    save_state(&state_path, &state).unwrap();
    let loaded = load_state(&state_path).unwrap().unwrap();
    assert_eq!(loaded.agent_override, Some("claude".to_string()));
    assert_eq!(loaded.max_session_duration_override, Some(1800));
}

#[test]
fn test_none_overrides_not_serialized() {
    // When overrides are None, they should not appear in the JSON output
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("timer.json");
    let state = CryoState {
        session_number: 1,
        pid: None,
        retry_count: 0,
        agent_override: None,
        max_session_duration_override: None,

        last_report_time: None,
        provider_index: None,
        instance_id: None,
        session_active: false,
        previous_session_crashed: false,
        session_active: false,
    };
    save_state(&state_path, &state).unwrap();
    let json = std::fs::read_to_string(&state_path).unwrap();
    assert!(!json.contains("agent_override"));
    assert!(!json.contains("max_session_duration_override"));
    assert!(!json.contains("last_report_time"));
    assert!(!json.contains("provider_index"));
}

#[test]
fn test_last_report_time_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("timer.json");
    let state = CryoState {
        session_number: 1,
        pid: None,
        retry_count: 0,
        agent_override: None,
        max_session_duration_override: None,

        last_report_time: Some("2026-02-28T09:00:00".to_string()),
        provider_index: None,
        instance_id: None,
        session_active: false,
        previous_session_crashed: false,
        session_active: false,
    };
    save_state(&state_path, &state).unwrap();
    let loaded = load_state(&state_path).unwrap().unwrap();
    assert_eq!(
        loaded.last_report_time,
        Some("2026-02-28T09:00:00".to_string())
    );

    // Verify it appears in JSON
    let json = std::fs::read_to_string(&state_path).unwrap();
    assert!(json.contains("last_report_time"));
}

#[test]
fn test_provider_index_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("timer.json");
    let state = CryoState {
        session_number: 1,
        pid: None,
        retry_count: 0,
        agent_override: None,
        max_session_duration_override: None,

        last_report_time: None,
        provider_index: Some(2),
        instance_id: None,
        session_active: false,
        previous_session_crashed: false,
        session_active: false,
    };
    save_state(&state_path, &state).unwrap();
    let loaded = load_state(&state_path).unwrap().unwrap();
    assert_eq!(loaded.provider_index, Some(2));

    // Verify it appears in JSON
    let json = std::fs::read_to_string(&state_path).unwrap();
    assert!(json.contains("provider_index"));
}
