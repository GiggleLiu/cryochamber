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
fn wake_notification_action_uses_watcher_for_running_daemon_with_watch_inbox() {
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
