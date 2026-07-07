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

#[test]
fn stop_chamber_leaves_locked_but_unresponsive_pid_untouched() {
    // A real, live process with no cryo daemon answering on the socket is the
    // reboot / PID-reuse "stale" case. stop_chamber must NOT signal that PID
    // (it may now belong to an unrelated process) but must still null the pid.
    let dir = tempfile::tempdir().unwrap();
    let mut child = std::process::Command::new("sleep")
        .arg("60")
        .spawn()
        .unwrap();
    let pid = child.id();

    let st = CryoState {
        session_number: 2,
        pid: Some(pid),
        agent_override: None,
        max_session_duration_override: None,
        instance_id: Some("stale".into()),
        session_active: true,
        previous_session_crashed: false,
    };
    state::save_state(&state::state_path(dir.path()), &st).unwrap();

    stop_chamber(dir.path()).unwrap();

    // If the fix were wrong and stop_chamber terminated the PID, the child would
    // have exited. `try_wait` reaps and reports it regardless of zombie state.
    let exited = child.try_wait().unwrap().is_some();
    let _ = child.kill();
    let _ = child.wait();

    assert!(
        !exited,
        "stop_chamber must not terminate a locked-but-unresponsive (stale) PID"
    );

    let after = state::load_state(&state::state_path(dir.path()))
        .unwrap()
        .unwrap();
    assert!(
        after.pid.is_none(),
        "stop_chamber should still null the stale pid"
    );
}
