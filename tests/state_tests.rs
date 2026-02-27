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
        max_retries_override: None,
        max_session_duration_override: None,
        next_wake: None,
        last_report_time: None,
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
        max_retries_override: None,
        max_session_duration_override: None,
        next_wake: None,
        last_report_time: None,
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
        max_retries_override: None,
        max_session_duration_override: None,
        next_wake: None,
        last_report_time: None,
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
        max_retries_override: None,
        max_session_duration_override: None,
        next_wake: None,
        last_report_time: None,
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
        max_retries_override: Some(5),
        max_session_duration_override: Some(1800),
        next_wake: None,
        last_report_time: None,
    };
    save_state(&state_path, &state).unwrap();
    let loaded = load_state(&state_path).unwrap().unwrap();
    assert_eq!(loaded.agent_override, Some("claude".to_string()));
    assert_eq!(loaded.max_retries_override, Some(5));
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
        max_retries_override: None,
        max_session_duration_override: None,
        next_wake: None,
        last_report_time: None,
    };
    save_state(&state_path, &state).unwrap();
    let json = std::fs::read_to_string(&state_path).unwrap();
    assert!(!json.contains("agent_override"));
    assert!(!json.contains("max_retries_override"));
    assert!(!json.contains("max_session_duration_override"));
    assert!(!json.contains("next_wake"));
    assert!(!json.contains("last_report_time"));
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
        max_retries_override: None,
        max_session_duration_override: None,
        next_wake: None,
        last_report_time: Some("2026-02-28T09:00:00".to_string()),
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
fn test_next_wake_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("timer.json");
    let state = CryoState {
        session_number: 1,
        pid: None,
        retry_count: 0,
        agent_override: None,
        max_retries_override: None,
        max_session_duration_override: None,
        next_wake: Some("2026-03-01T09:00".to_string()),
        last_report_time: None,
    };
    save_state(&state_path, &state).unwrap();
    let loaded = load_state(&state_path).unwrap().unwrap();
    assert_eq!(loaded.next_wake, Some("2026-03-01T09:00".to_string()));

    // Verify it appears in JSON
    let json = std::fs::read_to_string(&state_path).unwrap();
    assert!(json.contains("next_wake"));
}
