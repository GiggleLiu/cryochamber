// tests/state_tests.rs
use cryochamber::state::{load_state, save_state, CryoState};

#[test]
fn test_save_and_load_state() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("timer.json");

    let state = CryoState {
        plan_path: "plan.md".to_string(),
        session_number: 3,
        last_command: Some("opencode test".to_string()),
        pid: Some(std::process::id()),
        max_retries: 1,
        retry_count: 0,
        max_session_duration: 1800,
        watch_inbox: true,
        daemon_mode: false,
    };

    save_state(&state_path, &state).unwrap();
    let loaded = load_state(&state_path).unwrap().unwrap();

    assert_eq!(loaded.session_number, 3);
    assert_eq!(loaded.last_command, Some("opencode test".to_string()));
    assert_eq!(loaded.max_retries, 1);
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
        plan_path: "plan.md".to_string(),
        session_number: 1,
        last_command: None,
        pid: Some(std::process::id()),
        max_retries: 1,
        retry_count: 0,
        max_session_duration: 1800,
        watch_inbox: true,
        daemon_mode: false,
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
        plan_path: "plan.md".to_string(),
        session_number: 1,
        last_command: None,
        pid: Some(999999),
        max_retries: 1,
        retry_count: 0,
        max_session_duration: 1800,
        watch_inbox: true,
        daemon_mode: false,
    };
    assert!(!is_locked(&state));
}

#[test]
fn test_is_locked_no_pid() {
    use cryochamber::state::is_locked;
    let state = CryoState {
        plan_path: "plan.md".to_string(),
        session_number: 1,
        last_command: None,
        pid: None,
        max_retries: 1,
        retry_count: 0,
        max_session_duration: 1800,
        watch_inbox: true,
        daemon_mode: false,
    };
    assert!(!is_locked(&state));
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
fn test_load_legacy_state_without_retry_fields() {
    // Old timer.json files won't have max_retries/retry_count — serde defaults should apply
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("timer.json");
    let legacy_json = r#"{
        "plan_path": "plan.md",
        "session_number": 5,
        "last_command": "opencode",
        "pid": null
    }"#;
    std::fs::write(&state_path, legacy_json).unwrap();
    let loaded = load_state(&state_path).unwrap().unwrap();
    assert_eq!(loaded.session_number, 5);
    assert_eq!(loaded.max_retries, 1); // default
    assert_eq!(loaded.retry_count, 0); // default
}

#[test]
fn test_retry_fields_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("timer.json");
    let state = CryoState {
        plan_path: "plan.md".to_string(),
        session_number: 1,
        last_command: None,
        pid: None,
        max_retries: 5,
        retry_count: 2,
        max_session_duration: 1800,
        watch_inbox: true,
        daemon_mode: false,
    };
    save_state(&state_path, &state).unwrap();
    let loaded = load_state(&state_path).unwrap().unwrap();
    assert_eq!(loaded.max_retries, 5);
    assert_eq!(loaded.retry_count, 2);
}
