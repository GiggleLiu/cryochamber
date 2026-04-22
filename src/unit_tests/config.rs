use super::*;

#[test]
fn test_load_malformed_toml() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cryo.toml");
    std::fs::write(&path, "this is {{{{ not valid toml").unwrap();
    let result = load_config(&path);
    assert!(result.is_err(), "Should return error for malformed TOML");
}

#[test]
fn test_load_partial_toml() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cryo.toml");
    std::fs::write(&path, "agent = \"claude\"\n").unwrap();
    let config = load_config(&path).unwrap().unwrap();
    assert_eq!(config.agent, "claude");
    assert_eq!(config.max_retries, 5, "Should use default max_retries");
    assert_eq!(config.max_session_duration, 0, "Should use default timeout");
    assert!(config.watch_inbox, "Should use default watch_inbox");
}

#[test]
fn test_apply_overrides_all_fields() {
    let mut config = CryoConfig::default();
    let state = crate::state::CryoState {
        session_number: 1,
        pid: None,
        retry_count: 0,
        agent_override: Some("claude".to_string()),
        max_retries_override: Some(10),
        max_session_duration_override: Some(300),

        last_report_time: None,
        provider_index: None,
        instance_id: None,
        pending_fallback: None,
        in_flight_fallback: None,
        previous_session_crashed: false,
    };
    config.apply_overrides(&state);
    assert_eq!(config.agent, "claude");
    assert_eq!(config.max_retries, 10);
    assert_eq!(config.max_session_duration, 300);
}

#[test]
fn test_apply_overrides_none_fields() {
    let original = CryoConfig::default();
    let mut config = CryoConfig::default();
    let state = crate::state::CryoState {
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
        in_flight_fallback: None,
        previous_session_crashed: false,
    };
    config.apply_overrides(&state);
    assert_eq!(config.agent, original.agent);
    assert_eq!(config.max_retries, original.max_retries);
    assert_eq!(config.max_session_duration, original.max_session_duration);
}
