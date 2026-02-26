// tests/config_tests.rs
use cryochamber::config::{config_path, load_config, save_config, CryoConfig};
use cryochamber::state::CryoState;

#[test]
fn test_config_defaults() {
    let config = CryoConfig::default();
    assert_eq!(config.agent, "opencode");
    assert_eq!(config.max_retries, 1);
    assert_eq!(config.max_session_duration, 0);
    assert!(config.watch_inbox);
}

#[test]
fn test_config_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let path = config_path(dir.path());

    let config = CryoConfig {
        agent: "claude".to_string(),
        max_retries: 5,
        max_session_duration: 3600,
        watch_inbox: false,
        ..Default::default()
    };

    save_config(&path, &config).unwrap();
    let loaded = load_config(&path).unwrap().unwrap();

    assert_eq!(loaded.agent, "claude");
    assert_eq!(loaded.max_retries, 5);
    assert_eq!(loaded.max_session_duration, 3600);
    assert!(!loaded.watch_inbox);
}

#[test]
fn test_config_load_missing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nonexistent.toml");
    let loaded = load_config(&path).unwrap();
    assert!(loaded.is_none());
}

#[test]
fn test_config_partial_toml_uses_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let path = config_path(dir.path());
    // Only set agent — other fields should use defaults
    std::fs::write(&path, "agent = \"codex\"\n").unwrap();

    let loaded = load_config(&path).unwrap().unwrap();
    assert_eq!(loaded.agent, "codex");
    assert_eq!(loaded.max_retries, 1); // default
    assert_eq!(loaded.max_session_duration, 0); // default
    assert!(loaded.watch_inbox); // default
}

#[test]
fn test_apply_overrides_all() {
    let mut config = CryoConfig::default();
    let state = CryoState {
        session_number: 0,
        pid: None,
        retry_count: 0,
        agent_override: Some("claude".to_string()),
        max_retries_override: Some(10),
        max_session_duration_override: Some(7200),
    };

    config.apply_overrides(&state);

    assert_eq!(config.agent, "claude");
    assert_eq!(config.max_retries, 10);
    assert_eq!(config.max_session_duration, 7200);
}

#[test]
fn test_apply_overrides_none_keeps_config() {
    let mut config = CryoConfig {
        agent: "opencode".to_string(),
        max_retries: 3,
        max_session_duration: 1800,
        watch_inbox: true,
        ..Default::default()
    };

    let state = CryoState {
        session_number: 0,
        pid: None,
        retry_count: 0,
        agent_override: None,
        max_retries_override: None,
        max_session_duration_override: None,
    };

    config.apply_overrides(&state);

    // Nothing should change
    assert_eq!(config.agent, "opencode");
    assert_eq!(config.max_retries, 3);
    assert_eq!(config.max_session_duration, 1800);
    assert!(config.watch_inbox);
}

#[test]
fn test_apply_overrides_partial() {
    let mut config = CryoConfig {
        agent: "opencode".to_string(),
        max_retries: 3,
        max_session_duration: 1800,
        watch_inbox: true,
        ..Default::default()
    };

    let state = CryoState {
        session_number: 0,
        pid: None,
        retry_count: 0,
        agent_override: Some("claude".to_string()),
        max_retries_override: None,
        max_session_duration_override: None,
    };

    config.apply_overrides(&state);

    assert_eq!(config.agent, "claude"); // overridden
    assert_eq!(config.max_retries, 3); // unchanged
    assert_eq!(config.max_session_duration, 1800); // unchanged
    assert!(config.watch_inbox); // unchanged
}

#[test]
fn test_config_template_substitution() {
    let dir = tempfile::tempdir().unwrap();
    let wrote = cryochamber::protocol::write_config_file(dir.path(), "claude").unwrap();
    assert!(wrote);

    let content = std::fs::read_to_string(dir.path().join("cryo.toml")).unwrap();
    assert!(content.contains("agent = \"claude\""));
    assert!(!content.contains("{{agent}}"));
}

#[test]
fn test_config_template_no_clobber() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cryo.toml");
    std::fs::write(&path, "custom config").unwrap();
    let wrote = cryochamber::protocol::write_config_file(dir.path(), "claude").unwrap();
    assert!(!wrote);
    let content = std::fs::read_to_string(&path).unwrap();
    assert_eq!(content, "custom config");
}

#[test]
fn test_config_path() {
    let dir = std::path::Path::new("/some/project");
    assert_eq!(
        config_path(dir),
        std::path::PathBuf::from("/some/project/cryo.toml")
    );
}
