use anyhow::Context;

pub mod agent;
pub mod chamber_status;
pub mod channel;
pub mod config;
pub mod daemon;
pub mod daemon_client;
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

/// Environment variable naming the chamber directory. The daemon injects it
/// into every spawned agent session, so `cryo-agent` resolves the right
/// chamber no matter what cwd the agent's shell tool happens to use (agent
/// runners have been observed running commands from `~`, which used to route
/// IPC to a different chamber's socket).
pub const CHAMBER_DIR_ENV: &str = "CRYO_CHAMBER_DIR";

/// Resolve the chamber directory: `CRYO_CHAMBER_DIR` when set, else cwd.
pub fn work_dir() -> anyhow::Result<std::path::PathBuf> {
    let dir = match std::env::var_os(CHAMBER_DIR_ENV) {
        Some(dir) => std::path::PathBuf::from(dir),
        None => std::env::current_dir().context("Failed to get current directory")?,
    };
    dir.canonicalize().or_else(|_| Ok(dir))
}

/// Fail fast, with the real reason, when `dir` is not a chamber. Without
/// this, a wrong cwd surfaces downstream as "Daemon instance mismatch" or a
/// connect error against some other chamber's socket — red herrings that have
/// cost entire agent sessions of debugging.
pub fn ensure_chamber_dir(dir: &std::path::Path) -> anyhow::Result<()> {
    if dir.join("cryo.toml").exists() {
        return Ok(());
    }
    anyhow::bail!(
        "Not a cryochamber directory: {} (no cryo.toml found). \
         Run this command from the chamber root, or set CRYO_CHAMBER_DIR to it.",
        dir.display()
    )
}

#[cfg(test)]
#[path = "unit_tests/lib.rs"]
mod lib_tests;

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
