// tests/config_tests.rs
use cryochamber::config::{
    config_path, default_watch_dirs, load_config, save_config, CryoConfig, ProviderConfig,
    LEGACY_PROVIDERS_DEPRECATION_WARNING,
};
use cryochamber::state::CryoState;
use std::collections::HashMap;
use std::path::PathBuf;

#[test]
fn test_config_defaults() {
    let config = CryoConfig::default();
    assert_eq!(config.agent, "opencode");
    assert_eq!(config.max_session_duration, 0);
    assert_eq!(config.watch_dirs, default_watch_dirs());
}

#[test]
fn test_config_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let path = config_path(dir.path());

    let config = CryoConfig {
        agent: "claude".to_string(),
        max_session_duration: 3600,
        watch_dirs: vec![],
        ..Default::default()
    };

    save_config(&path, &config).unwrap();
    let loaded = load_config(&path).unwrap().unwrap();

    assert_eq!(loaded.agent, "claude");
    assert_eq!(loaded.max_session_duration, 3600);
    assert!(loaded.watch_dirs.is_empty());
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
    assert_eq!(loaded.max_session_duration, 0); // default
    assert_eq!(loaded.watch_dirs, default_watch_dirs()); // default
}

#[test]
fn test_apply_overrides_all() {
    let mut config = CryoConfig::default();
    let state = CryoState {
        session_number: 0,
        pid: None,
        agent_override: Some("claude".to_string()),
        max_session_duration_override: Some(7200),
        instance_id: None,
        session_active: false,
        previous_session_crashed: false,
    };

    config.apply_overrides(&state);

    assert_eq!(config.agent, "claude");
    assert_eq!(config.max_session_duration, 7200);
}

#[test]
fn test_apply_overrides_none_keeps_config() {
    let mut config = CryoConfig {
        agent: "opencode".to_string(),
        max_session_duration: 1800,
        watch_dirs: default_watch_dirs(),
        ..Default::default()
    };

    let state = CryoState {
        session_number: 0,
        pid: None,
        agent_override: None,
        max_session_duration_override: None,
        instance_id: None,
        session_active: false,
        previous_session_crashed: false,
    };

    config.apply_overrides(&state);

    // Nothing should change
    assert_eq!(config.agent, "opencode");
    assert_eq!(config.max_session_duration, 1800);
    assert_eq!(config.watch_dirs, default_watch_dirs());
}

#[test]
fn test_apply_overrides_partial() {
    let mut config = CryoConfig {
        agent: "opencode".to_string(),
        max_session_duration: 1800,
        watch_dirs: default_watch_dirs(),
        ..Default::default()
    };

    let state = CryoState {
        session_number: 0,
        pid: None,
        agent_override: Some("claude".to_string()),
        max_session_duration_override: None,
        instance_id: None,
        session_active: false,
        previous_session_crashed: false,
    };

    config.apply_overrides(&state);

    assert_eq!(config.agent, "claude"); // overridden
    assert_eq!(config.max_session_duration, 1800); // unchanged
    assert_eq!(config.watch_dirs, default_watch_dirs()); // unchanged
}

#[test]
fn test_watch_inbox_is_ignored_when_watch_dirs_missing() {
    let dir = tempfile::tempdir().unwrap();
    let path = config_path(dir.path());
    std::fs::write(&path, "agent = \"opencode\"\nwatch_inbox = false\n").unwrap();

    let loaded = load_config(&path).unwrap().unwrap();
    assert_eq!(loaded.watch_dirs, default_watch_dirs());

    save_config(&path, &loaded).unwrap();
    let serialized = std::fs::read_to_string(&path).unwrap();
    assert!(
        !serialized.contains("watch_inbox"),
        "legacy watch_inbox should not be reserialized: {serialized}"
    );
    assert!(serialized.contains("watch_dirs"));
}

#[test]
fn test_watch_inbox_does_not_override_explicit_watch_dirs() {
    let dir = tempfile::tempdir().unwrap();
    let path = config_path(dir.path());
    std::fs::write(
        &path,
        "agent = \"opencode\"\n\
         watch_inbox = false\n\
         watch_dirs = [\"custom/dir\"]\n",
    )
    .unwrap();

    let loaded = load_config(&path).unwrap().unwrap();
    assert_eq!(loaded.watch_dirs, vec![PathBuf::from("custom/dir")]);
}

#[test]
fn test_multiple_watch_dirs_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let path = config_path(dir.path());
    let config = CryoConfig {
        watch_dirs: vec![
            PathBuf::from("messages/inbox"),
            PathBuf::from("incoming"),
            PathBuf::from("/tmp/external"),
        ],
        ..Default::default()
    };

    save_config(&path, &config).unwrap();
    let loaded = load_config(&path).unwrap().unwrap();

    assert_eq!(
        loaded.watch_dirs,
        vec![
            PathBuf::from("messages/inbox"),
            PathBuf::from("incoming"),
            PathBuf::from("/tmp/external"),
        ]
    );
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

#[test]
fn test_config_with_provider_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let path = config_path(dir.path());

    let toml_content = r#"
agent = "opencode"

[provider]
name = "openai"
env = { OPENCODE_PROVIDER = "openai", OPENAI_API_KEY = "sk-test" }
"#;
    std::fs::write(&path, toml_content).unwrap();
    let loaded = load_config(&path).unwrap().unwrap();

    let provider = loaded.active_provider().expect("active provider");
    assert_eq!(provider.name, "openai");
    assert_eq!(provider.env.get("OPENCODE_PROVIDER").unwrap(), "openai");
    assert_eq!(provider.env.get("OPENAI_API_KEY").unwrap(), "sk-test");
    assert!(!loaded.uses_legacy_providers());

    let serialized = toml::to_string_pretty(&loaded).unwrap();
    assert!(serialized.contains("[provider]"));
    assert!(!serialized.contains("[[providers]]"));
}

#[test]
fn test_legacy_providers_roundtrip_canonicalizes_to_provider() {
    let dir = tempfile::tempdir().unwrap();
    let path = config_path(dir.path());

    let toml_content = r#"
agent = "opencode"
# Legacy rotation settings should be ignored when read and not reserialized.
rotate_on = "quick-exit"

[[providers]]
name = "anthropic"
env = { ANTHROPIC_API_KEY = "sk-ant-test" }

[[providers]]
name = "openai"
env = { OPENAI_API_KEY = "sk-test", OPENAI_BASE_URL = "https://api.openai.com/v1" }
"#;
    std::fs::write(&path, toml_content).unwrap();
    let loaded = load_config(&path).unwrap().unwrap();

    assert!(loaded.uses_legacy_providers());
    assert_eq!(loaded.providers.len(), 2);
    assert_eq!(loaded.providers[0].name, "anthropic");
    assert_eq!(loaded.active_provider().unwrap().name, "anthropic");
    assert_eq!(
        loaded.providers[0].env.get("ANTHROPIC_API_KEY").unwrap(),
        "sk-ant-test"
    );
    assert_eq!(loaded.providers[1].name, "openai");
    assert_eq!(loaded.providers[1].env.len(), 2);

    let serialized = toml::to_string_pretty(&loaded).unwrap();
    assert!(
        !serialized.contains("rotate_on"),
        "rotation policy should not be part of saved config: {serialized}"
    );
    assert!(serialized.contains("[provider]"));
    assert!(
        !serialized.contains("[[providers]]"),
        "legacy provider arrays should be read-only compatibility input: {serialized}"
    );
    assert!(LEGACY_PROVIDERS_DEPRECATION_WARNING.contains("[[providers]]"));
}

#[test]
fn test_save_config_canonicalizes_legacy_providers() {
    let dir = tempfile::tempdir().unwrap();
    let path = config_path(dir.path());
    let mut env = HashMap::new();
    env.insert("OPENAI_API_KEY".to_string(), "sk-test".to_string());
    let config = CryoConfig {
        providers: vec![ProviderConfig {
            name: "openai".to_string(),
            env,
        }],
        ..CryoConfig::default()
    };

    save_config(&path, &config).unwrap();

    let serialized = std::fs::read_to_string(&path).unwrap();
    assert!(serialized.contains("[provider]"));
    assert!(!serialized.contains("[[providers]]"));
    let loaded = load_config(&path).unwrap().unwrap();
    assert_eq!(loaded.active_provider().unwrap().name, "openai");
}

#[test]
fn test_config_without_providers_backward_compatible() {
    let dir = tempfile::tempdir().unwrap();
    let path = config_path(dir.path());
    std::fs::write(&path, "agent = \"opencode\"\n").unwrap();

    let loaded = load_config(&path).unwrap().unwrap();
    assert!(loaded.provider.is_none());
    assert!(loaded.providers.is_empty());
    assert!(loaded.active_provider().is_none());
}
