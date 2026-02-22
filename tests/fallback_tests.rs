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
