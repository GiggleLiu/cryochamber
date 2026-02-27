// src/fallback.rs
use anyhow::Result;
use chrono::Local;
use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use crate::message::{self, Message};

#[derive(Debug, Clone)]
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

    /// Write the fallback alert to messages/outbox/ and optionally dispatch
    /// a system notification based on the configured alert method.
    ///
    /// `alert_method` controls the action:
    /// - `"notify"`: desktop notification + outbox file
    /// - `"outbox"`: outbox file only (no popup)
    /// - `"none"`: disable fallback alerts entirely
    pub fn execute(&self, work_dir: &Path, alert_method: &str) -> Result<()> {
        if alert_method == "none" {
            eprintln!("Fallback: alert suppressed (fallback_alert = \"none\")");
            return Ok(());
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

        if alert_method == "notify" {
            if let Err(e) = self.send_notification() {
                eprintln!("Fallback: desktop notification failed: {e}");
            }
        }

        Ok(())
    }

    /// Send a desktop notification via notify-rust.
    fn send_notification(&self) -> Result<()> {
        let mut notification = notify_rust::Notification::new();
        notification
            .summary(&format!("Cryochamber Alert: {}", self.action))
            .body(&self.message);
        // Platform-specific alert emphasis
        #[cfg(target_os = "linux")]
        {
            notification.urgency(notify_rust::Urgency::Critical);
            notification.timeout(notify_rust::Timeout::Never);
        }
        #[cfg(target_os = "macos")]
        {
            notification.subtitle("Dead-man switch fired");
            notification.sound_name("Sosumi");
        }
        notification.show()?;
        Ok(())
    }
}
