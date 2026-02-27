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
pub mod report;
pub mod service;
pub mod session;
pub mod socket;
pub mod state;
pub mod web;

pub fn work_dir() -> anyhow::Result<std::path::PathBuf> {
    let dir = std::env::current_dir().context("Failed to get current directory")?;
    dir.canonicalize().or_else(|_| Ok(dir))
}

use anyhow::Context;
