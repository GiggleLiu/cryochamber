// src/timer/launchd.rs
use super::{CryoTimer, TimerId, TimerStatus};
use crate::fallback::FallbackAction;
use anyhow::{Context, Result};
use chrono::NaiveDateTime;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct LaunchdTimer;

impl Default for LaunchdTimer {
    fn default() -> Self {
        Self::new()
    }
}

impl LaunchdTimer {
    pub fn new() -> Self {
        Self
    }

    pub fn make_label(work_dir: &str) -> String {
        let mut hasher = DefaultHasher::new();
        work_dir.hash(&mut hasher);
        let hash = hasher.finish();
        format!("com.cryochamber.{:x}", hash)
    }

    pub fn plist_path(&self, label: &str) -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        PathBuf::from(home)
            .join("Library/LaunchAgents")
            .join(format!("{label}.plist"))
    }

    pub fn generate_plist(
        &self,
        label: &str,
        time: &NaiveDateTime,
        command: &str,
        work_dir: &str,
    ) -> String {
        let parts: Vec<&str> = command.split_whitespace().collect();
        let mut program_args = String::new();
        for part in &parts {
            program_args.push_str(&format!("      <string>{part}</string>\n"));
        }

        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>
    <key>ProgramArguments</key>
    <array>
{program_args}    </array>
    <key>WorkingDirectory</key>
    <string>{work_dir}</string>
    <key>StartCalendarInterval</key>
    <dict>
        <key>Month</key>
        <integer>{}</integer>
        <key>Day</key>
        <integer>{}</integer>
        <key>Hour</key>
        <integer>{}</integer>
        <key>Minute</key>
        <integer>{}</integer>
    </dict>
    <key>StandardOutPath</key>
    <string>{work_dir}/cryo-launchd.out</string>
    <key>StandardErrorPath</key>
    <string>{work_dir}/cryo-launchd.err</string>
</dict>
</plist>"#,
            time.format("%m")
                .to_string()
                .trim_start_matches('0')
                .parse::<u32>()
                .unwrap(),
            time.format("%d")
                .to_string()
                .trim_start_matches('0')
                .parse::<u32>()
                .unwrap(),
            time.format("%H")
                .to_string()
                .trim_start_matches('0')
                .parse::<u32>()
                .unwrap_or(0),
            time.format("%M")
                .to_string()
                .trim_start_matches('0')
                .parse::<u32>()
                .unwrap_or(0),
        )
    }

    fn load_plist(&self, path: &Path) -> Result<()> {
        let uid = Command::new("id").arg("-u").output()?.stdout;
        let uid = String::from_utf8_lossy(&uid).trim().to_string();
        Command::new("launchctl")
            .args(["bootstrap", &format!("gui/{uid}"), &path.to_string_lossy()])
            .output()
            .context("Failed to load launchd plist")?;
        Ok(())
    }

    fn unload_plist(&self, label: &str) -> Result<()> {
        let uid = Command::new("id").arg("-u").output()?.stdout;
        let uid = String::from_utf8_lossy(&uid).trim().to_string();
        let _ = Command::new("launchctl")
            .args(["bootout", &format!("gui/{uid}/{label}")])
            .output();
        Ok(())
    }
}

impl CryoTimer for LaunchdTimer {
    fn schedule_wake(&self, time: NaiveDateTime, command: &str, work_dir: &str) -> Result<TimerId> {
        let label = Self::make_label(work_dir);
        let wake_label = format!("{label}.wake");
        let plist = self.generate_plist(&wake_label, &time, command, work_dir);
        let path = self.plist_path(&wake_label);

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        self.unload_plist(&wake_label)?;
        std::fs::write(&path, plist)?;
        self.load_plist(&path)?;

        Ok(TimerId(wake_label))
    }

    fn schedule_fallback(
        &self,
        time: NaiveDateTime,
        action: &FallbackAction,
        work_dir: &str,
    ) -> Result<TimerId> {
        let label = Self::make_label(work_dir);
        let fb_label = format!("{label}.fallback");
        let command = format!(
            "cryochamber fallback-exec {} {} \"{}\"",
            action.action, action.target, action.message
        );
        let plist = self.generate_plist(&fb_label, &time, &command, work_dir);
        let path = self.plist_path(&fb_label);

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        self.unload_plist(&fb_label)?;
        std::fs::write(&path, plist)?;
        self.load_plist(&path)?;

        Ok(TimerId(fb_label))
    }

    fn cancel(&self, id: &TimerId) -> Result<()> {
        self.unload_plist(&id.0)?;
        let path = self.plist_path(&id.0);
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }

    fn verify(&self, id: &TimerId) -> Result<TimerStatus> {
        let uid = Command::new("id").arg("-u").output()?.stdout;
        let uid = String::from_utf8_lossy(&uid).trim().to_string();
        let output = Command::new("launchctl")
            .args(["print", &format!("gui/{uid}/{}", id.0)])
            .output()?;

        if output.status.success() {
            Ok(TimerStatus::Scheduled {
                next_fire: NaiveDateTime::default(),
            })
        } else {
            Ok(TimerStatus::NotFound)
        }
    }
}
