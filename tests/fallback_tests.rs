// tests/fallback_tests.rs
use cryochamber::fallback::FallbackAction;

#[test]
fn test_fallback_action_display() {
    let action = FallbackAction {
        action: "email".to_string(),
        target: "user@example.com".to_string(),
        message: "task failed".to_string(),
    };
    let display = format!("{action}");
    assert!(display.contains("email"));
    assert!(display.contains("user@example.com"));
}

#[test]
fn test_fallback_action_is_email() {
    let action = FallbackAction {
        action: "email".to_string(),
        target: "user@example.com".to_string(),
        message: "failed".to_string(),
    };
    assert!(action.is_email());
    assert!(!action.is_webhook());
}

#[test]
fn test_fallback_action_is_webhook() {
    let action = FallbackAction {
        action: "webhook".to_string(),
        target: "https://hooks.slack.com/xxx".to_string(),
        message: "failed".to_string(),
    };
    assert!(!action.is_email());
    assert!(action.is_webhook());
}

#[test]
fn test_execute_writes_to_outbox() {
    let dir = tempfile::tempdir().unwrap();
    let action = FallbackAction {
        action: "email".to_string(),
        target: "user@example.com".to_string(),
        message: "session did not run".to_string(),
    };
    action.execute(dir.path(), "outbox").unwrap();

    // Verify outbox file was created
    let outbox = dir.path().join("messages/outbox");
    let entries: Vec<_> = std::fs::read_dir(&outbox)
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    assert_eq!(entries.len(), 1);

    let content = std::fs::read_to_string(entries[0].path()).unwrap();
    assert!(content.contains("fallback_action: email"));
    assert!(content.contains("fallback_target: user@example.com"));
    assert!(content.contains("session did not run"));
}

#[test]
fn test_execute_webhook_writes_to_outbox() {
    let dir = tempfile::tempdir().unwrap();
    let action = FallbackAction {
        action: "webhook".to_string(),
        target: "https://hooks.slack.com/xxx".to_string(),
        message: "alert".to_string(),
    };
    action.execute(dir.path(), "outbox").unwrap();

    let outbox = dir.path().join("messages/outbox");
    let entries: Vec<_> = std::fs::read_dir(&outbox)
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    assert_eq!(entries.len(), 1);

    let content = std::fs::read_to_string(entries[0].path()).unwrap();
    assert!(content.contains("fallback_action: webhook"));
}
