use std::sync::{Mutex, MutexGuard};

static ENV_LOCK: Mutex<()> = Mutex::new(());

struct EnvVarGuard<'a> {
    _lock: MutexGuard<'a, ()>,
    key: &'static str,
    previous: Option<String>,
}

impl<'a> EnvVarGuard<'a> {
    fn set(key: &'static str, value: &std::path::Path) -> Self {
        let lock = ENV_LOCK.lock().unwrap_or_else(|poison| poison.into_inner());
        let previous = std::env::var(key).ok();
        std::env::set_var(key, value);
        Self {
            _lock: lock,
            key,
            previous,
        }
    }
}

impl Drop for EnvVarGuard<'_> {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => std::env::set_var(self.key, value),
            None => std::env::remove_var(self.key),
        }
    }
}

#[test]
fn hub_log_path_lives_under_global_xdg_state_home() {
    let state = tempfile::tempdir().unwrap();
    let _guard = EnvVarGuard::set("XDG_STATE_HOME", state.path());

    let path = cryochamber::hub::paths::hub_log_path();

    assert!(path.starts_with(state.path()));
    assert!(path.ends_with("cryohub.log"));
    assert!(path.to_string_lossy().contains("/cryo/hub/"));
}

#[test]
fn hub_config_path_lives_under_xdg_config_home() {
    let config = tempfile::tempdir().unwrap();
    let _guard = EnvVarGuard::set("XDG_CONFIG_HOME", config.path());

    let path = cryochamber::hub::paths::hub_config_path();

    assert!(path.starts_with(config.path()));
    assert!(path.ends_with("cryo/cryohub.toml"));
}

#[test]
fn global_chambers_dir_defaults_to_home_dot_cryo_chambers() {
    let state = tempfile::tempdir().unwrap();
    let _guard = EnvVarGuard::set("HOME", state.path());

    let path = cryochamber::hub::paths::global_chambers_dir();

    assert_eq!(path, state.path().join(".cryo/chambers"));
}

#[test]
fn hub_config_defaults_chamber_root_to_home_dot_cryo_chambers() {
    let home = tempfile::tempdir().unwrap();
    let _home = EnvVarGuard::set("HOME", home.path());

    let cfg = cryochamber::hub::config::load_config().unwrap();

    assert_eq!(cfg.chamber_root, home.path().join(".cryo/chambers"));
}

#[test]
fn hub_config_reads_custom_chamber_root() {
    let config_home = tempfile::tempdir().unwrap();
    let _config = EnvVarGuard::set("XDG_CONFIG_HOME", config_home.path());
    let custom_root = tempfile::tempdir().unwrap();
    let path = config_home.path().join("cryo/cryohub.toml");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        format!(
            "host = \"127.0.0.1\"\nport = 8765\nchamber_root = \"{}\"\n",
            custom_root.path().display()
        ),
    )
    .unwrap();

    let cfg = cryochamber::hub::config::load_config().unwrap();

    assert_eq!(cfg.chamber_root, custom_root.path());
}

#[test]
fn hub_config_save_round_trips_config_file() {
    let config_home = tempfile::tempdir().unwrap();
    let _config = EnvVarGuard::set("XDG_CONFIG_HOME", config_home.path());
    let custom_root = tempfile::tempdir().unwrap();
    let cfg = cryochamber::hub::config::HubConfig {
        host: "0.0.0.0".to_string(),
        port: 9876,
        chamber_root: custom_root.path().to_path_buf(),
    };

    cryochamber::hub::config::save_config(&cfg).unwrap();

    let path = cryochamber::hub::paths::hub_config_path();
    assert!(path.exists());
    assert_eq!(cryochamber::hub::config::load_config().unwrap(), cfg);
}

#[test]
fn hub_config_load_or_create_writes_default_config_when_missing() {
    let config_home = tempfile::tempdir().unwrap();
    let _config = EnvVarGuard::set("XDG_CONFIG_HOME", config_home.path());

    let cfg = cryochamber::hub::config::load_or_create_config().unwrap();

    let path = cryochamber::hub::paths::hub_config_path();
    assert!(path.exists());
    assert_eq!(cryochamber::hub::config::load_config().unwrap(), cfg);
}

#[test]
fn hub_effective_config_persists_host_and_port_overrides() {
    let config_home = tempfile::tempdir().unwrap();
    let _config = EnvVarGuard::set("XDG_CONFIG_HOME", config_home.path());

    let cfg = cryochamber::hub::config::effective_config(Some("0.0.0.0".to_string()), Some(9900))
        .unwrap();

    assert_eq!(cfg.host, "0.0.0.0");
    assert_eq!(cfg.port, 9900);
    assert_eq!(cryochamber::hub::config::load_config().unwrap(), cfg);
}
