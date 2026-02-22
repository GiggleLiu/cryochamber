// src/fallback.rs
use anyhow::{Context, Result};
use std::fmt;

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

    pub fn execute(&self) -> Result<()> {
        match self.action.as_str() {
            "email" => self.send_email(),
            "webhook" => self.send_webhook(),
            _ => anyhow::bail!("Unknown fallback action: {}", self.action),
        }
    }

    fn send_email(&self) -> Result<()> {
        let output = std::process::Command::new("mail")
            .args(["-s", "Cryochamber Alert", &self.target])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .context("Failed to spawn mail command")?
            .wait_with_output()?;

        if !output.status.success() {
            eprintln!("Warning: email fallback may have failed (exit {})", output.status);
        }
        Ok(())
    }

    fn send_webhook(&self) -> Result<()> {
        let body = serde_json::json!({
            "text": format!("Cryochamber Alert: {}", self.message)
        });

        let output = std::process::Command::new("curl")
            .args([
                "-s", "-X", "POST",
                "-H", "Content-Type: application/json",
                "-d", &body.to_string(),
                &self.target,
            ])
            .output()
            .context("Failed to send webhook")?;

        if !output.status.success() {
            eprintln!("Warning: webhook fallback may have failed");
        }
        Ok(())
    }
}
