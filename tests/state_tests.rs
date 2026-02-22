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
        wake_timer_id: Some("com.cryochamber.abc.wake".to_string()),
        fallback_timer_id: Some("com.cryochamber.abc.fallback".to_string()),
        pid: Some(std::process::id()),
    };

    save_state(&state_path, &state).unwrap();
    let loaded = load_state(&state_path).unwrap().unwrap();

    assert_eq!(loaded.session_number, 3);
    assert_eq!(loaded.last_command, Some("opencode test".to_string()));
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
        wake_timer_id: None,
        fallback_timer_id: None,
        pid: Some(std::process::id()),
    };
    save_state(&state_path, &state).unwrap();

    // Current process PID should be considered "running"
    let loaded = load_state(&state_path).unwrap().unwrap();
    assert_eq!(loaded.pid, Some(std::process::id()));
}
