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

    /// Write the fallback alert to messages/outbox/ as a markdown file.
    /// External runners watch the outbox and deliver via email, webhook, etc.
    pub fn execute(&self, work_dir: &Path) -> Result<()> {
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
