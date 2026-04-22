use super::*;

#[test]
fn fallback_alert_mode_suppresses_none() {
    assert_eq!(fallback_alert_mode("none"), FallbackAlertMode::Suppress);
}

#[test]
fn fallback_alert_mode_writes_outbox_for_non_none_values() {
    for method in ["outbox", "notify", "custom"] {
        assert_eq!(fallback_alert_mode(method), FallbackAlertMode::Outbox);
    }
}
