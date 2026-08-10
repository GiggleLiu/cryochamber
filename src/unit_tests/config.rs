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

#[cfg(unix)]
#[test]
fn test_save_config_is_owner_readable_only() {
    // cryo.toml can carry a provider API key, so it must never be world- or
    // group-readable on disk.
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cryo.toml");

    save_config(&path, &CryoConfig::default()).unwrap();

    let mode = std::fs::metadata(&path).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600, "cryo.toml must be mode 0600");

    // And the config still round-trips for the owner.
    assert!(load_config(&path).unwrap().is_some());
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

#[test]
fn wait_timeout_defaults_to_none_and_is_not_serialized() {
    let config = CryoConfig::default();
    assert_eq!(config.wait_timeout, None);
    let toml = toml::to_string(&config).unwrap();
    assert!(!toml.contains("wait_timeout"));
}

#[test]
fn wait_timeout_round_trips_when_set() {
    let toml_src = "agent = \"mock\"\nwait_timeout = 7200\n";
    let config: CryoConfig = toml::from_str(toml_src).unwrap();
    assert_eq!(config.wait_timeout, Some(7200));
    let out = toml::to_string(&config).unwrap();
    assert!(out.contains("wait_timeout = 7200"));
}

#[test]
fn reply_window_defaults_to_none_and_is_not_serialized() {
    let config = CryoConfig::default();
    assert_eq!(config.reply_window, None);
    let toml = toml::to_string(&config).unwrap();
    assert!(!toml.contains("reply_window"));
}

#[test]
fn reply_window_absent_key_parses_as_none() {
    let config: CryoConfig = toml::from_str("agent = \"mock\"\n").unwrap();
    assert_eq!(config.reply_window, None);
}

/// The struct keeps `None` for an absent key; the 300 s window is applied at
/// the use site (`config.reply_window.unwrap_or(DEFAULT_..)`), and an
/// explicit `0` disables the window rather than falling back to the default.
#[test]
fn reply_window_unset_resolves_to_the_default_window_and_zero_disables() {
    assert_eq!(crate::config::DEFAULT_REPLY_WINDOW_SECS, 300);

    let unset: CryoConfig = toml::from_str("agent = \"mock\"\n").unwrap();
    assert_eq!(
        unset
            .reply_window
            .unwrap_or(crate::config::DEFAULT_REPLY_WINDOW_SECS),
        300
    );

    let disabled: CryoConfig = toml::from_str("agent = \"mock\"\nreply_window = 0\n").unwrap();
    assert_eq!(disabled.reply_window, Some(0));
    assert_eq!(
        disabled
            .reply_window
            .unwrap_or(crate::config::DEFAULT_REPLY_WINDOW_SECS),
        0
    );
}

#[test]
fn reply_window_round_trips_when_set() {
    let toml_src = "agent = \"mock\"\nreply_window = 600\n";
    let config: CryoConfig = toml::from_str(toml_src).unwrap();
    assert_eq!(config.reply_window, Some(600));
    let out = toml::to_string(&config).unwrap();
    assert!(out.contains("reply_window = 600"));
}

#[test]
fn reply_window_survives_save_and_load() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cryo.toml");
    let config = CryoConfig {
        reply_window: Some(900),
        ..CryoConfig::default()
    };

    save_config(&path, &config).unwrap();
    let loaded = load_config(&path).unwrap().unwrap();

    assert_eq!(loaded.reply_window, Some(900));
}
