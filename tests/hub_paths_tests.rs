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
fn hub_log_path_lives_under_xdg_state_home() {
    let state = tempfile::tempdir().unwrap();
    let _guard = EnvVarGuard::set("XDG_STATE_HOME", state.path());

    let workspace = std::path::Path::new("/tmp/example-workspace");
    let path = cryochamber::hub::paths::hub_log_path(workspace);

    assert!(path.starts_with(state.path()));
    assert!(path.ends_with("cryohub.log"));
    assert!(path.to_string_lossy().contains("/cryo/hubs/"));
    assert!(!path.starts_with(workspace));
}

#[test]
fn hub_log_path_is_scoped_by_workspace() {
    let state = tempfile::tempdir().unwrap();
    let _guard = EnvVarGuard::set("XDG_STATE_HOME", state.path());

    let first = cryochamber::hub::paths::hub_log_path(std::path::Path::new("/tmp/one"));
    let second = cryochamber::hub::paths::hub_log_path(std::path::Path::new("/tmp/two"));

    assert_ne!(first, second);
}
