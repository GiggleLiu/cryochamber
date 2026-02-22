// src/timer/mod.rs
pub mod launchd;
pub mod systemd;

use anyhow::Result;
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

use crate::fallback::FallbackAction;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimerId(pub String);

#[derive(Debug)]
pub enum TimerStatus {
    Scheduled { next_fire: NaiveDateTime },
    NotFound,
}

pub trait CryoTimer {
    fn schedule_wake(&self, time: NaiveDateTime, command: &str, work_dir: &str) -> Result<TimerId>;
    fn schedule_fallback(&self, time: NaiveDateTime, action: &FallbackAction, work_dir: &str) -> Result<TimerId>;
    fn cancel(&self, id: &TimerId) -> Result<()>;
    fn verify(&self, id: &TimerId) -> Result<TimerStatus>;
}

pub fn create_timer() -> Result<Box<dyn CryoTimer>> {
    if cfg!(target_os = "macos") {
        Ok(Box::new(launchd::LaunchdTimer::new()))
    } else if cfg!(target_os = "linux") {
        Ok(Box::new(systemd::SystemdTimer::new()))
    } else {
        anyhow::bail!("Unsupported platform. Cryochamber supports macOS (launchd) and Linux (systemd).")
    }
}
