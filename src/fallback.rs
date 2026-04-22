// src/fallback.rs
use anyhow::Result;
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use crate::message::{self, Message};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FallbackAction {
    pub action: String,
    pub target: String,
    pub message: String,
}

impl fmt::Display for FallbackAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} -> {} ({})", self.action, self.target, self.message)
    }
}

impl FallbackAction {
    pub fn is_email(&self) -> bool {
        self.action == "email"
    }

    pub fn is_webhook(&self) -> bool {
        self.action == "webhook"
    }

    /// Write the fallback alert to `messages/outbox/` for delivery by whatever
    /// reads it (sync channels, external watchers, the user tailing files).
    ///
    /// `alert_method` controls behavior:
    /// - `"outbox"` (default): write the alert file
    /// - `"none"`: suppress — no file written
    ///
    /// Legacy `"notify"` is accepted and treated as `"outbox"` for back-compat.
    pub fn execute(&self, work_dir: &Path, alert_method: &str) -> Result<()> {
        match fallback_alert_mode(alert_method) {
            FallbackAlertMode::Suppress => {
                eprintln!("Fallback: alert suppressed (fallback_alert = \"none\")");
                return Ok(());
            }
            FallbackAlertMode::Outbox => {}
        }

        message::ensure_dirs(work_dir)?;

        let msg = Message {
            from: "cryochamber".to_string(),
            subject: format!("Fallback Alert: {}", self.action),
            body: self.message.clone(),
            timestamp: Local::now().naive_local(),
            metadata: BTreeMap::from([
                ("fallback_action".to_string(), self.action.clone()),
                ("fallback_target".to_string(), self.target.clone()),
            ]),
        };

        let path = message::write_message(work_dir, "outbox", &msg)?;
        println!(
            "Fallback alert written to {}",
            path.strip_prefix(work_dir).unwrap_or(&path).display()
        );

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FallbackAlertMode {
    Suppress,
    Outbox,
}

fn fallback_alert_mode(alert_method: &str) -> FallbackAlertMode {
    match alert_method {
        "none" => FallbackAlertMode::Suppress,
        _ => FallbackAlertMode::Outbox,
    }
}

#[cfg(test)]
#[path = "unit_tests/fallback.rs"]
mod tests;
