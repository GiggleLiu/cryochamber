use super::*;

fn state(pid: Option<u32>, instance_id: Option<&str>) -> CryoState {
    CryoState {
        session_number: 1,
        pid,
        agent_override: None,
        max_session_duration_override: None,
        instance_id: instance_id.map(str::to_string),
        session_active: false,
        previous_session_crashed: false,
    }
}

#[test]
fn restarted_state_requires_pid_or_instance_change() {
    let before = state(Some(10), Some("old"));

    assert!(!restarted_state(
        Some(&before),
        &state(Some(10), Some("old"))
    ));
    assert!(restarted_state(
        Some(&before),
        &state(Some(11), Some("old"))
    ));
    assert!(restarted_state(
        Some(&before),
        &state(Some(10), Some("new"))
    ));
}

#[test]
fn restarted_state_accepts_any_live_state_when_no_baseline_exists() {
    assert!(restarted_state(None, &state(Some(10), Some("new"))));
}
