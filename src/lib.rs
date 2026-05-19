use anyhow::Context;

pub mod agent;
pub mod chamber_status;
pub mod channel;
pub mod config;
pub mod daemon;
pub mod daemon_client;
pub mod gh_sync;
pub mod hub;
pub mod lifecycle;
pub mod log;
pub mod message;
pub mod process;
pub mod protocol;
pub mod registry;
pub mod service;
pub mod session;
pub mod socket;
pub mod state;
pub mod sync_common;
pub mod sync_control;
pub mod todo;
pub mod zulip_sync;

pub fn work_dir() -> anyhow::Result<std::path::PathBuf> {
    let dir = std::env::current_dir().context("Failed to get current directory")?;
    dir.canonicalize().or_else(|_| Ok(dir))
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::ffi::OsString;
    use std::path::Path;
    use std::sync::{Mutex, MutexGuard};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    pub(crate) struct EnvVarGuard<'a> {
        _lock: MutexGuard<'a, ()>,
        key: &'static str,
        previous: Option<OsString>,
    }

    impl<'a> EnvVarGuard<'a> {
        pub(crate) fn set_path(key: &'static str, value: &Path) -> Self {
            let lock = ENV_LOCK.lock().unwrap_or_else(|poison| poison.into_inner());
            let previous = std::env::var_os(key);
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
}
