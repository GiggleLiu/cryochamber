use super::*;

#[test]
fn test_load_empty_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("timer.json");
    std::fs::write(&path, "").unwrap();
    let result = load_state(&path).unwrap();
    assert!(result.is_none(), "Empty file should return None");
}

#[test]
fn test_load_corrupted_json() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("timer.json");
    std::fs::write(&path, "{broken").unwrap();
    let result = load_state(&path);
    assert!(result.is_err(), "Corrupted JSON should return error");
}

#[test]
fn test_load_minimal_json() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("timer.json");
    std::fs::write(&path, r#"{"session_number": 5}"#).unwrap();
    let state = load_state(&path).unwrap().unwrap();
    assert_eq!(state.session_number, 5);
    assert!(state.pid.is_none(), "pid should default to None");
    assert!(state.agent_override.is_none());
}

#[test]
fn test_legacy_retry_count_is_not_reserialized() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("timer.json");
    std::fs::write(
        &path,
        r#"{"session_number": 5, "pid": null, "retry_count": 9}"#,
    )
    .unwrap();

    let state = load_state(&path).unwrap().unwrap();
    save_state(&path, &state).unwrap();

    let json = std::fs::read_to_string(&path).unwrap();
    assert!(
        !json.contains("retry_count"),
        "retry_count is legacy retry-loop state and should not be reserialized: {json}"
    );
}

#[test]
fn test_legacy_last_report_time_is_not_reserialized() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("timer.json");
    std::fs::write(
        &path,
        r#"{"session_number": 5, "last_report_time": "2026-03-01T09:00:00"}"#,
    )
    .unwrap();

    let state = load_state(&path).unwrap().unwrap();
    save_state(&path, &state).unwrap();

    let json = std::fs::read_to_string(&path).unwrap();
    assert!(
        !json.contains("last_report_time"),
        "last_report_time is legacy report-interval state and should not be reserialized: {json}"
    );
}

#[test]
fn test_is_locked_stale_pid() {
    // Spawn a child, wait for it to exit, use its PID
    let mut child = std::process::Command::new("true").spawn().unwrap();
    let pid = child.id();
    child.wait().unwrap();
    // Small delay to ensure the process is fully reaped
    std::thread::sleep(std::time::Duration::from_millis(100));

    let state = CryoState {
        session_number: 1,
        pid: Some(pid),

        agent_override: None,
        max_session_duration_override: None,
        instance_id: None,
        session_active: false,
        previous_session_crashed: false,
    };
    assert!(!is_locked(&state), "Dead PID should not be locked");
}

#[test]
fn test_is_locked_no_pid() {
    let state = CryoState {
        session_number: 1,
        pid: None,

        agent_override: None,
        max_session_duration_override: None,
        instance_id: None,
        session_active: false,
        previous_session_crashed: false,
    };
    assert!(!is_locked(&state), "No PID should not be locked");
}

#[test]
fn test_is_locked_own_pid() {
    let state = CryoState {
        session_number: 1,
        pid: Some(std::process::id()),

        agent_override: None,
        max_session_duration_override: None,
        instance_id: None,
        session_active: false,
        previous_session_crashed: false,
    };
    assert!(is_locked(&state), "Own PID should be locked");
}

#[test]
fn test_session_active_defaults_false_when_missing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("timer.json");
    std::fs::write(&path, r#"{"session_number": 1}"#).unwrap();
    let state = load_state(&path).unwrap().unwrap();
    assert!(
        !state.session_active,
        "session_active should default to false for pre-existing timer.json"
    );
}

#[test]
fn test_session_active_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("timer.json");
    let mut state = CryoState {
        session_number: 2,
        pid: None,
        agent_override: None,
        max_session_duration_override: None,
        instance_id: None,
        previous_session_crashed: false,
        session_active: true,
    };
    save_state(&path, &state).unwrap();
    let loaded = load_state(&path).unwrap().unwrap();
    assert!(loaded.session_active);
    state.session_active = false;
    save_state(&path, &state).unwrap();
    let loaded = load_state(&path).unwrap().unwrap();
    assert!(!loaded.session_active);
}
