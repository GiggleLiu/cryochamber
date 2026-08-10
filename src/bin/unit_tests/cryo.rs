use super::*;

#[test]
fn daemon_termination_action_skips_unlocked_state() {
    assert_eq!(
        daemon_termination_action(false, true, Some(123)),
        DaemonTerminationAction::Skip
    );
}

#[test]
fn daemon_termination_action_skips_unresponsive_daemon() {
    assert_eq!(
        daemon_termination_action(true, false, Some(123)),
        DaemonTerminationAction::Skip
    );
}

#[test]
fn daemon_termination_action_skips_missing_pid() {
    assert_eq!(
        daemon_termination_action(true, true, None),
        DaemonTerminationAction::Skip
    );
}

#[test]
fn daemon_termination_action_terminates_locked_responding_pid() {
    assert_eq!(
        daemon_termination_action(true, true, Some(123)),
        DaemonTerminationAction::Terminate(123)
    );
}
