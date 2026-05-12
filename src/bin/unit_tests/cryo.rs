use super::*;

#[test]
fn wake_notification_action_queues_when_daemon_is_not_running() {
    assert_eq!(
        wake_notification_action(false, false),
        WakeNotificationAction::QueueUntilStart
    );
    assert_eq!(
        wake_notification_action(false, true),
        WakeNotificationAction::QueueUntilStart
    );
}

#[test]
fn wake_notification_action_uses_watcher_for_running_daemon_with_inbox_watched() {
    assert_eq!(
        wake_notification_action(true, true),
        WakeNotificationAction::InboxWatcher
    );
}

#[test]
fn wake_notification_action_sends_signal_for_running_daemon_without_watcher() {
    assert_eq!(
        wake_notification_action(true, false),
        WakeNotificationAction::SendSignal
    );
}

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
