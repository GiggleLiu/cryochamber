pub mod agent;
pub mod channel;
pub mod config;
pub mod daemon;
pub mod fallback;
pub mod gh_sync;
pub mod log;
pub mod message;
pub mod process;
pub mod protocol;
pub mod registry;
pub mod session;
pub mod socket;
pub mod state;

pub fn work_dir() -> anyhow::Result<std::path::PathBuf> {
    std::env::current_dir().context("Failed to get current directory")
}

use anyhow::Context;
