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
    assert_eq!(
        config.max_session_duration, 3600,
        "Should use default timeout"
    );
    assert_eq!(
        config.watch_dirs,
        default_watch_dirs(),
        "Should use default watch_dirs"
    );
}

#[test]
fn test_save_config_omits_removed_report_fields() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cryo.toml");

    save_config(&path, &CryoConfig::default()).unwrap();

    let toml = std::fs::read_to_string(path).unwrap();
    assert!(!toml.contains("report_time"), "got {toml}");
    assert!(!toml.contains("report_interval"), "got {toml}");
}

#[test]
fn apply_optional_override_replaces_value_when_present() {
    let mut value = "opencode".to_string();

    apply_optional_override(&mut value, &Some("claude".to_string()));

    assert_eq!(value, "claude");
}

#[test]
fn apply_optional_override_keeps_value_when_absent() {
    let mut value = 5;

    apply_optional_override(&mut value, &None);

    assert_eq!(value, 5);
}

#[test]
fn test_apply_overrides_all_fields() {
    let mut config = CryoConfig::default();
    let state = crate::state::CryoState {
        session_number: 1,
        pid: None,
        agent_override: Some("claude".to_string()),
        max_session_duration_override: Some(300),
        instance_id: None,
        session_active: false,
        previous_session_crashed: false,
    };
    config.apply_overrides(&state);
    assert_eq!(config.agent, "claude");
    assert_eq!(config.max_session_duration, 300);
}

#[test]
fn test_apply_overrides_none_fields() {
    let original = CryoConfig::default();
    let mut config = CryoConfig::default();
    let state = crate::state::CryoState {
        session_number: 1,
        pid: None,
        agent_override: None,
        max_session_duration_override: None,
        instance_id: None,
        session_active: false,
        previous_session_crashed: false,
    };
    config.apply_overrides(&state);
    assert_eq!(config.agent, original.agent);
    assert_eq!(config.max_session_duration, original.max_session_duration);
}
