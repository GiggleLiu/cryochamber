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
    assert_eq!(state.retry_count, 0, "retry_count should default to 0");
    assert!(state.agent_override.is_none());
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
        retry_count: 0,

        agent_override: None,
        max_retries_override: None,
        max_session_duration_override: None,
        last_report_time: None,
        provider_index: None,
        instance_id: None,
        pending_fallback: None,
    };
    assert!(!is_locked(&state), "Dead PID should not be locked");
}

#[test]
fn test_is_locked_no_pid() {
    let state = CryoState {
        session_number: 1,
        pid: None,
        retry_count: 0,

        agent_override: None,
        max_retries_override: None,
        max_session_duration_override: None,
        last_report_time: None,
        provider_index: None,
        instance_id: None,
        pending_fallback: None,
    };
    assert!(!is_locked(&state), "No PID should not be locked");
}

#[test]
fn test_is_locked_own_pid() {
    let state = CryoState {
        session_number: 1,
        pid: Some(std::process::id()),
        retry_count: 0,

        agent_override: None,
        max_retries_override: None,
        max_session_duration_override: None,
        last_report_time: None,
        provider_index: None,
        instance_id: None,
        pending_fallback: None,
    };
    assert!(is_locked(&state), "Own PID should be locked");
}
