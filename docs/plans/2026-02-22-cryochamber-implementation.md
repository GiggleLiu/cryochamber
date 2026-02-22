# Cryochamber Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a Rust CLI that schedules AI agent tasks over long timeframes using OS-level hibernation (launchd on macOS, systemd on Linux).

**Architecture:** Thin CLI daemon communicates with AI agent (opencode) via log-based marker protocol. The daemon parses markers, manages OS timers, validates before hibernating. The AI agent dynamically determines next wake time.

**Tech Stack:** Rust, clap (CLI), chrono (datetime), regex (marker parsing), serde/toml/serde_json (config/state), fslock (concurrency), lettre (email fallback), anyhow (errors)

---

### Task 1: Project Scaffolding

**Files:**
- Create: `Cargo.toml`
- Create: `src/main.rs`
- Create: `src/lib.rs`

**Step 1: Initialize Cargo project**

Run: `cargo init --name cryochamber`

**Step 2: Add dependencies to Cargo.toml**

```toml
[package]
name = "cryochamber"
version = "0.1.0"
edition = "2021"

[dependencies]
clap = { version = "4", features = ["derive"] }
chrono = { version = "0.4", features = ["serde"] }
regex = "1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
anyhow = "1"
fslock = "0.2"
lettre = "0.11"
plist = "1"
```

**Step 3: Create minimal src/lib.rs**

```rust
pub mod marker;
pub mod log;
pub mod timer;
pub mod agent;
pub mod fallback;
pub mod validate;
```

**Step 4: Create stub modules**

Create empty files:
- `src/marker.rs`
- `src/log.rs`
- `src/timer/mod.rs`
- `src/timer/launchd.rs`
- `src/timer/systemd.rs`
- `src/agent.rs`
- `src/fallback.rs`
- `src/validate.rs`

**Step 5: Verify it compiles**

Run: `cargo check`
Expected: success (with warnings about empty modules)

**Step 6: Commit**

```bash
git add -A
git commit -m "feat: scaffold cryochamber project with dependencies"
```

---

### Task 2: Marker Parser

**Files:**
- Create: `src/marker.rs`
- Create: `tests/marker_tests.rs`

**Step 1: Write failing tests**

```rust
// tests/marker_tests.rs
use cryochamber::marker::{parse_markers, CryoMarkers, ExitCode, FallbackAction};

#[test]
fn test_parse_exit_success() {
    let text = "[CRYO:EXIT 0] All tasks completed";
    let markers = parse_markers(text).unwrap();
    assert_eq!(markers.exit_code, Some(ExitCode::Success));
    assert_eq!(markers.exit_summary, Some("All tasks completed".to_string()));
}

#[test]
fn test_parse_exit_failure() {
    let text = "[CRYO:EXIT 2] Could not connect to API";
    let markers = parse_markers(text).unwrap();
    assert_eq!(markers.exit_code, Some(ExitCode::Failure));
}

#[test]
fn test_parse_exit_partial() {
    let text = "[CRYO:EXIT 1] Reviewed 2 of 5 PRs";
    let markers = parse_markers(text).unwrap();
    assert_eq!(markers.exit_code, Some(ExitCode::Partial));
}

#[test]
fn test_parse_wake() {
    let text = "[CRYO:WAKE 2025-03-08T09:00]";
    let markers = parse_markers(text).unwrap();
    assert!(markers.wake_time.is_some());
    let wake = markers.wake_time.unwrap();
    assert_eq!(wake.month(), 3);
    assert_eq!(wake.day(), 8);
    assert_eq!(wake.hour(), 9);
}

#[test]
fn test_parse_cmd() {
    let text = r#"[CRYO:CMD opencode "check PR #42"]"#;
    let markers = parse_markers(text).unwrap();
    assert_eq!(markers.command, Some(r#"opencode "check PR #42""#.to_string()));
}

#[test]
fn test_parse_plan() {
    let text = "[CRYO:PLAN waiting on CI, check status first]";
    let markers = parse_markers(text).unwrap();
    assert_eq!(markers.plan_note, Some("waiting on CI, check status first".to_string()));
}

#[test]
fn test_parse_fallback() {
    let text = r#"[CRYO:FALLBACK email user@example.com "weekly review failed"]"#;
    let markers = parse_markers(text).unwrap();
    assert_eq!(markers.fallbacks.len(), 1);
    assert_eq!(markers.fallbacks[0].action, "email");
    assert_eq!(markers.fallbacks[0].target, "user@example.com");
    assert_eq!(markers.fallbacks[0].message, "weekly review failed");
}

#[test]
fn test_parse_multiple_fallbacks() {
    let text = r#"[CRYO:FALLBACK email user@example.com "task failed"]
[CRYO:FALLBACK webhook https://hooks.slack.com/xxx "task failed"]"#;
    let markers = parse_markers(text).unwrap();
    assert_eq!(markers.fallbacks.len(), 2);
}

#[test]
fn test_parse_full_session() {
    let text = r#"Checked 3 PRs. All look good.

[CRYO:EXIT 0] Reviewed 3 PRs, all approved
[CRYO:PLAN follow up on PR #41 next week
[CRYO:WAKE 2025-03-08T09:00]
[CRYO:CMD opencode "check for new PRs"]
[CRYO:FALLBACK email user@example.com "PR review did not run"]"#;
    let markers = parse_markers(text).unwrap();
    assert_eq!(markers.exit_code, Some(ExitCode::Success));
    assert!(markers.wake_time.is_some());
    assert!(markers.command.is_some());
    assert!(markers.plan_note.is_some());
    assert_eq!(markers.fallbacks.len(), 1);
}

#[test]
fn test_parse_no_markers() {
    let text = "Just some regular text with no markers";
    let markers = parse_markers(text).unwrap();
    assert_eq!(markers.exit_code, None);
    assert!(markers.wake_time.is_none());
}

#[test]
fn test_markers_anywhere_in_text() {
    let text = r#"Some text before
[CRYO:EXIT 0] done
More text after
[CRYO:WAKE 2025-03-08T09:00]
Even more text"#;
    let markers = parse_markers(text).unwrap();
    assert_eq!(markers.exit_code, Some(ExitCode::Success));
    assert!(markers.wake_time.is_some());
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --test marker_tests`
Expected: FAIL — module and types don't exist yet

**Step 3: Implement marker parser**

```rust
// src/marker.rs
use anyhow::Result;
use chrono::NaiveDateTime;
use regex::Regex;

#[derive(Debug, Clone, PartialEq)]
pub enum ExitCode {
    Success,  // 0
    Partial,  // 1
    Failure,  // 2
}

impl ExitCode {
    pub fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Success),
            1 => Some(Self::Partial),
            2 => Some(Self::Failure),
            _ => None,
        }
    }

    pub fn as_code(&self) -> u8 {
        match self {
            Self::Success => 0,
            Self::Partial => 1,
            Self::Failure => 2,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FallbackAction {
    pub action: String,
    pub target: String,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct CryoMarkers {
    pub exit_code: Option<ExitCode>,
    pub exit_summary: Option<String>,
    pub wake_time: Option<NaiveDateTime>,
    pub command: Option<String>,
    pub plan_note: Option<String>,
    pub fallbacks: Vec<FallbackAction>,
}

pub fn parse_markers(text: &str) -> Result<CryoMarkers> {
    let mut markers = CryoMarkers::default();

    let exit_re = Regex::new(r"\[CRYO:EXIT\s+(\d+)\]\s*(.*)")?;
    let wake_re = Regex::new(r"\[CRYO:WAKE\s+([^\]]+)\]")?;
    let cmd_re = Regex::new(r"\[CRYO:CMD\s+(.*)\]")?;
    let plan_re = Regex::new(r"\[CRYO:PLAN\s+(.*)")?;
    let fallback_re = Regex::new(r#"\[CRYO:FALLBACK\s+(\S+)\s+(\S+)\s+"([^"]+)"\]"#)?;

    for line in text.lines() {
        if let Some(cap) = exit_re.captures(line) {
            let code: u8 = cap[1].parse()?;
            markers.exit_code = ExitCode::from_code(code);
            let summary = cap[2].trim().to_string();
            if !summary.is_empty() {
                markers.exit_summary = Some(summary);
            }
        }
        if let Some(cap) = wake_re.captures(line) {
            let time_str = cap[1].trim();
            // Try parsing with seconds, then without
            markers.wake_time = NaiveDateTime::parse_from_str(time_str, "%Y-%m-%dT%H:%M:%S")
                .or_else(|_| NaiveDateTime::parse_from_str(time_str, "%Y-%m-%dT%H:%M"))
                .ok();
        }
        if let Some(cap) = cmd_re.captures(line) {
            markers.command = Some(cap[1].trim().to_string());
        }
        if let Some(cap) = plan_re.captures(line) {
            markers.plan_note = Some(cap[1].trim().to_string());
        }
        if let Some(cap) = fallback_re.captures(line) {
            markers.fallbacks.push(FallbackAction {
                action: cap[1].to_string(),
                target: cap[2].to_string(),
                message: cap[3].to_string(),
            });
        }
    }

    Ok(markers)
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test --test marker_tests`
Expected: all 11 tests PASS

**Step 5: Commit**

```bash
git add src/marker.rs tests/marker_tests.rs
git commit -m "feat: implement CRYO marker parser with tests"
```

---

### Task 3: Log Manager

**Files:**
- Create: `src/log.rs`
- Create: `tests/log_tests.rs`

**Step 1: Write failing tests**

```rust
// tests/log_tests.rs
use cryochamber::log::{append_session, read_latest_session, Session};
use std::fs;
use tempfile::NamedTempFile;

#[test]
fn test_append_session_to_new_file() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("cryo.log");

    let session = Session {
        number: 1,
        task: "Review PRs".to_string(),
        output: "Reviewed 3 PRs.\n[CRYO:EXIT 0] Done".to_string(),
    };

    append_session(&log_path, &session).unwrap();

    let contents = fs::read_to_string(&log_path).unwrap();
    assert!(contents.contains("--- CRYO SESSION"));
    assert!(contents.contains("Session: 1"));
    assert!(contents.contains("Task: Review PRs"));
    assert!(contents.contains("[CRYO:EXIT 0] Done"));
    assert!(contents.contains("--- CRYO END ---"));
}

#[test]
fn test_append_multiple_sessions() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("cryo.log");

    let s1 = Session {
        number: 1,
        task: "Task one".to_string(),
        output: "[CRYO:EXIT 0] Done".to_string(),
    };
    let s2 = Session {
        number: 2,
        task: "Task two".to_string(),
        output: "[CRYO:EXIT 0] Also done".to_string(),
    };

    append_session(&log_path, &s1).unwrap();
    append_session(&log_path, &s2).unwrap();

    let contents = fs::read_to_string(&log_path).unwrap();
    assert_eq!(contents.matches("--- CRYO SESSION").count(), 2);
    assert_eq!(contents.matches("--- CRYO END ---").count(), 2);
}

#[test]
fn test_read_latest_session() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("cryo.log");

    let s1 = Session {
        number: 1,
        task: "Task one".to_string(),
        output: "[CRYO:EXIT 0] First".to_string(),
    };
    let s2 = Session {
        number: 2,
        task: "Task two".to_string(),
        output: "[CRYO:EXIT 0] Second".to_string(),
    };

    append_session(&log_path, &s1).unwrap();
    append_session(&log_path, &s2).unwrap();

    let latest = read_latest_session(&log_path).unwrap().unwrap();
    assert!(latest.contains("Second"));
    assert!(!latest.contains("First"));
}

#[test]
fn test_read_latest_session_empty_file() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("cryo.log");
    fs::write(&log_path, "").unwrap();

    let latest = read_latest_session(&log_path).unwrap();
    assert!(latest.is_none());
}

#[test]
fn test_read_latest_session_no_file() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("nonexistent.log");

    let latest = read_latest_session(&log_path).unwrap();
    assert!(latest.is_none());
}

#[test]
fn test_session_count() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("cryo.log");

    assert_eq!(cryochamber::log::session_count(&log_path).unwrap(), 0);

    let s1 = Session { number: 1, task: "T".into(), output: "O".into() };
    append_session(&log_path, &s1).unwrap();
    assert_eq!(cryochamber::log::session_count(&log_path).unwrap(), 1);

    let s2 = Session { number: 2, task: "T".into(), output: "O".into() };
    append_session(&log_path, &s2).unwrap();
    assert_eq!(cryochamber::log::session_count(&log_path).unwrap(), 2);
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --test log_tests`
Expected: FAIL

**Step 3: Add tempfile dev-dependency**

Add to Cargo.toml:
```toml
[dev-dependencies]
tempfile = "3"
```

**Step 4: Implement log manager**

```rust
// src/log.rs
use anyhow::Result;
use chrono::Local;
use std::fs;
use std::io::Write;
use std::path::Path;

const SESSION_START: &str = "--- CRYO SESSION";
const SESSION_END: &str = "--- CRYO END ---";

pub struct Session {
    pub number: u32,
    pub task: String,
    pub output: String,
}

pub fn append_session(log_path: &Path, session: &Session) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;

    let timestamp = Local::now().format("%Y-%m-%dT%H:%M:%S");
    writeln!(file, "{SESSION_START} {timestamp} ---")?;
    writeln!(file, "Session: {}", session.number)?;
    writeln!(file, "Task: {}", session.task)?;
    writeln!(file)?;
    writeln!(file, "{}", session.output)?;
    writeln!(file, "{SESSION_END}")?;
    writeln!(file)?;

    Ok(())
}

pub fn read_latest_session(log_path: &Path) -> Result<Option<String>> {
    if !log_path.exists() {
        return Ok(None);
    }

    let contents = fs::read_to_string(log_path)?;
    if contents.trim().is_empty() {
        return Ok(None);
    }

    // Find the last session block
    let last_start = contents.rfind(SESSION_START);
    let last_end = contents.rfind(SESSION_END);

    match (last_start, last_end) {
        (Some(start), Some(end)) if end > start => {
            let session_text = &contents[start..end + SESSION_END.len()];
            Ok(Some(session_text.to_string()))
        }
        _ => Ok(None),
    }
}

pub fn session_count(log_path: &Path) -> Result<u32> {
    if !log_path.exists() {
        return Ok(0);
    }
    let contents = fs::read_to_string(log_path)?;
    Ok(contents.matches(SESSION_START).count() as u32)
}
```

**Step 5: Run tests to verify they pass**

Run: `cargo test --test log_tests`
Expected: all 6 tests PASS

**Step 6: Commit**

```bash
git add src/log.rs tests/log_tests.rs Cargo.toml
git commit -m "feat: implement session log manager with tests"
```

---

### Task 4: Timer Trait + Platform Detection

**Files:**
- Create: `src/timer/mod.rs`

**Step 1: Write the timer trait and platform detection**

```rust
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
```

**Step 2: Verify it compiles**

Run: `cargo check`
Expected: success (with warnings about unimplemented modules)

**Step 3: Commit**

```bash
git add src/timer/mod.rs
git commit -m "feat: add CryoTimer trait and platform detection"
```

---

### Task 5: launchd Timer (macOS)

**Files:**
- Create: `src/timer/launchd.rs`
- Create: `tests/launchd_tests.rs`

Note: launchd has no Year in StartCalendarInterval. Strategy: use Month/Day/Hour/Minute and have `cryochamber wake` unload its own plist after running (making it effectively one-shot).

**Step 1: Write failing tests**

```rust
// tests/launchd_tests.rs
#[cfg(target_os = "macos")]
mod tests {
    use cryochamber::timer::launchd::LaunchdTimer;
    use cryochamber::timer::CryoTimer;
    use chrono::NaiveDateTime;

    #[test]
    fn test_plist_generation() {
        let timer = LaunchdTimer::new();
        let wake_time = NaiveDateTime::parse_from_str("2025-03-08T09:00", "%Y-%m-%dT%H:%M").unwrap();
        let plist_content = timer.generate_plist(
            "com.cryochamber.test",
            &wake_time,
            "/usr/local/bin/cryochamber wake",
            "/tmp/test",
        );
        assert!(plist_content.contains("com.cryochamber.test"));
        assert!(plist_content.contains("<key>Month</key>"));
        assert!(plist_content.contains("<integer>3</integer>"));
        assert!(plist_content.contains("<key>Day</key>"));
        assert!(plist_content.contains("<integer>8</integer>"));
        assert!(plist_content.contains("<key>Hour</key>"));
        assert!(plist_content.contains("<integer>9</integer>"));
        assert!(plist_content.contains("<key>Minute</key>"));
        assert!(plist_content.contains("<integer>0</integer>"));
    }

    #[test]
    fn test_plist_path() {
        let timer = LaunchdTimer::new();
        let path = timer.plist_path("com.cryochamber.test");
        assert!(path.to_string_lossy().contains("LaunchAgents"));
        assert!(path.to_string_lossy().contains("com.cryochamber.test.plist"));
    }

    #[test]
    fn test_label_generation() {
        let label = LaunchdTimer::make_label("/Users/me/plans/myproject");
        assert!(label.starts_with("com.cryochamber."));
        // Same path should produce same label
        let label2 = LaunchdTimer::make_label("/Users/me/plans/myproject");
        assert_eq!(label, label2);
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --test launchd_tests`
Expected: FAIL

**Step 3: Implement launchd timer**

```rust
// src/timer/launchd.rs
use super::{CryoTimer, TimerId, TimerStatus};
use crate::fallback::FallbackAction;
use anyhow::{Context, Result};
use chrono::NaiveDateTime;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::process::Command;

pub struct LaunchdTimer;

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
            time.format("%m").to_string().trim_start_matches('0').parse::<u32>().unwrap(),
            time.format("%d").to_string().trim_start_matches('0').parse::<u32>().unwrap(),
            time.format("%H").to_string().trim_start_matches('0').parse::<u32>().unwrap_or(0),
            time.format("%M").to_string().trim_start_matches('0').parse::<u32>().unwrap_or(0),
        )
    }

    fn load_plist(&self, path: &PathBuf) -> Result<()> {
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

        // Ensure LaunchAgents dir exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Unload existing if any
        self.unload_plist(&wake_label)?;

        std::fs::write(&path, plist)?;
        self.load_plist(&path)?;

        Ok(TimerId(wake_label))
    }

    fn schedule_fallback(&self, time: NaiveDateTime, action: &FallbackAction, work_dir: &str) -> Result<TimerId> {
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
            // Timer is registered — parse next fire time if possible
            // For now, just confirm it exists
            Ok(TimerStatus::Scheduled {
                next_fire: NaiveDateTime::default(),
            })
        } else {
            Ok(TimerStatus::NotFound)
        }
    }
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test --test launchd_tests`
Expected: PASS (on macOS)

**Step 5: Commit**

```bash
git add src/timer/launchd.rs tests/launchd_tests.rs
git commit -m "feat: implement launchd timer for macOS"
```

---

### Task 6: systemd Timer (Linux)

**Files:**
- Create: `src/timer/systemd.rs`
- Create: `tests/systemd_tests.rs`

**Step 1: Write failing tests**

```rust
// tests/systemd_tests.rs
#[cfg(target_os = "linux")]
mod tests {
    use cryochamber::timer::systemd::SystemdTimer;
    use chrono::NaiveDateTime;

    #[test]
    fn test_timer_unit_generation() {
        let timer = SystemdTimer::new();
        let wake_time = NaiveDateTime::parse_from_str("2025-03-08T09:00", "%Y-%m-%dT%H:%M").unwrap();
        let content = timer.generate_timer_unit("cryochamber-test", &wake_time);
        assert!(content.contains("OnCalendar=2025-03-08 09:00:00"));
        assert!(content.contains("Persistent=true"));
        assert!(content.contains("RemainAfterElapse=false"));
    }

    #[test]
    fn test_service_unit_generation() {
        let timer = SystemdTimer::new();
        let content = timer.generate_service_unit(
            "cryochamber-test",
            "cryochamber wake",
            "/home/user/plans/myproject",
        );
        assert!(content.contains("Type=oneshot"));
        assert!(content.contains("ExecStart=cryochamber wake"));
        assert!(content.contains("WorkingDirectory=/home/user/plans/myproject"));
    }

    #[test]
    fn test_unit_name_generation() {
        let name = SystemdTimer::make_unit_name("/home/user/plans/myproject");
        assert!(name.starts_with("cryochamber-"));
        // Same path = same name
        let name2 = SystemdTimer::make_unit_name("/home/user/plans/myproject");
        assert_eq!(name, name2);
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --test systemd_tests`
Expected: FAIL (or skipped on macOS)

**Step 3: Implement systemd timer**

```rust
// src/timer/systemd.rs
use super::{CryoTimer, TimerId, TimerStatus};
use crate::fallback::FallbackAction;
use anyhow::{Context, Result};
use chrono::NaiveDateTime;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::process::Command;

pub struct SystemdTimer;

impl SystemdTimer {
    pub fn new() -> Self {
        Self
    }

    pub fn make_unit_name(work_dir: &str) -> String {
        let mut hasher = DefaultHasher::new();
        work_dir.hash(&mut hasher);
        let hash = hasher.finish();
        format!("cryochamber-{:x}", hash)
    }

    fn user_unit_dir() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        PathBuf::from(home).join(".config/systemd/user")
    }

    pub fn generate_timer_unit(&self, name: &str, time: &NaiveDateTime) -> String {
        format!(
            r#"[Unit]
Description=Cryochamber wake timer: {name}

[Timer]
OnCalendar={time}
RemainAfterElapse=false
Persistent=true

[Install]
WantedBy=timers.target
"#,
            time = time.format("%Y-%m-%d %H:%M:%S")
        )
    }

    pub fn generate_service_unit(&self, name: &str, command: &str, work_dir: &str) -> String {
        format!(
            r#"[Unit]
Description=Cryochamber task: {name}

[Service]
Type=oneshot
WorkingDirectory={work_dir}
ExecStart={command}
"#
        )
    }

    fn reload_daemon() -> Result<()> {
        Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .output()
            .context("Failed to reload systemd daemon")?;
        Ok(())
    }
}

impl CryoTimer for SystemdTimer {
    fn schedule_wake(&self, time: NaiveDateTime, command: &str, work_dir: &str) -> Result<TimerId> {
        let name = Self::make_unit_name(work_dir);
        let wake_name = format!("{name}-wake");
        let unit_dir = Self::user_unit_dir();
        std::fs::create_dir_all(&unit_dir)?;

        let timer_path = unit_dir.join(format!("{wake_name}.timer"));
        let service_path = unit_dir.join(format!("{wake_name}.service"));

        std::fs::write(&timer_path, self.generate_timer_unit(&wake_name, &time))?;
        std::fs::write(&service_path, self.generate_service_unit(&wake_name, command, work_dir))?;

        Self::reload_daemon()?;

        Command::new("systemctl")
            .args(["--user", "enable", "--now", &format!("{wake_name}.timer")])
            .output()
            .context("Failed to enable systemd timer")?;

        Ok(TimerId(wake_name))
    }

    fn schedule_fallback(&self, time: NaiveDateTime, action: &FallbackAction, work_dir: &str) -> Result<TimerId> {
        let name = Self::make_unit_name(work_dir);
        let fb_name = format!("{name}-fallback");
        let unit_dir = Self::user_unit_dir();
        std::fs::create_dir_all(&unit_dir)?;

        let command = format!(
            "cryochamber fallback-exec {} {} \"{}\"",
            action.action, action.target, action.message
        );

        let timer_path = unit_dir.join(format!("{fb_name}.timer"));
        let service_path = unit_dir.join(format!("{fb_name}.service"));

        std::fs::write(&timer_path, self.generate_timer_unit(&fb_name, &time))?;
        std::fs::write(&service_path, self.generate_service_unit(&fb_name, &command, work_dir))?;

        Self::reload_daemon()?;

        Command::new("systemctl")
            .args(["--user", "enable", "--now", &format!("{fb_name}.timer")])
            .output()
            .context("Failed to enable fallback timer")?;

        Ok(TimerId(fb_name))
    }

    fn cancel(&self, id: &TimerId) -> Result<()> {
        let _ = Command::new("systemctl")
            .args(["--user", "stop", &format!("{}.timer", id.0)])
            .output();
        let _ = Command::new("systemctl")
            .args(["--user", "disable", &format!("{}.timer", id.0)])
            .output();

        let unit_dir = Self::user_unit_dir();
        let _ = std::fs::remove_file(unit_dir.join(format!("{}.timer", id.0)));
        let _ = std::fs::remove_file(unit_dir.join(format!("{}.service", id.0)));

        Self::reload_daemon()?;
        Ok(())
    }

    fn verify(&self, id: &TimerId) -> Result<TimerStatus> {
        let output = Command::new("systemctl")
            .args(["--user", "is-active", &format!("{}.timer", id.0)])
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
```

**Step 4: Run tests to verify they pass**

Run: `cargo test --test systemd_tests`
Expected: PASS (on Linux), skipped (on macOS)

**Step 5: Commit**

```bash
git add src/timer/systemd.rs tests/systemd_tests.rs
git commit -m "feat: implement systemd timer for Linux"
```

---

### Task 7: Agent Runner

**Files:**
- Create: `src/agent.rs`
- Create: `tests/agent_tests.rs`

**Step 1: Write failing tests**

```rust
// tests/agent_tests.rs
use cryochamber::agent::{build_prompt, AgentConfig};

#[test]
fn test_build_prompt_first_session() {
    let config = AgentConfig {
        plan_content: "Review PRs every Monday".to_string(),
        log_content: None,
        session_number: 1,
        task: "Start the PR review plan".to_string(),
    };
    let prompt = build_prompt(&config);
    assert!(prompt.contains("cryochamber"));
    assert!(prompt.contains("Session number: 1"));
    assert!(prompt.contains("Review PRs every Monday"));
    assert!(prompt.contains("[CRYO:EXIT"));
    assert!(prompt.contains("Start the PR review plan"));
}

#[test]
fn test_build_prompt_with_history() {
    let config = AgentConfig {
        plan_content: "Review PRs every Monday".to_string(),
        log_content: Some("[CRYO:EXIT 0] Did stuff\n[CRYO:PLAN check PR #41]".to_string()),
        session_number: 3,
        task: "Follow up on PRs".to_string(),
    };
    let prompt = build_prompt(&config);
    assert!(prompt.contains("Session number: 3"));
    assert!(prompt.contains("[CRYO:EXIT 0] Did stuff"));
}

#[test]
fn test_build_prompt_contains_marker_instructions() {
    let config = AgentConfig {
        plan_content: "Do stuff".to_string(),
        log_content: None,
        session_number: 1,
        task: "Do the thing".to_string(),
    };
    let prompt = build_prompt(&config);
    assert!(prompt.contains("[CRYO:EXIT"));
    assert!(prompt.contains("[CRYO:WAKE"));
    assert!(prompt.contains("[CRYO:CMD"));
    assert!(prompt.contains("[CRYO:PLAN"));
    assert!(prompt.contains("[CRYO:FALLBACK"));
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --test agent_tests`
Expected: FAIL

**Step 3: Implement agent runner**

```rust
// src/agent.rs
use anyhow::{Context, Result};
use chrono::Local;
use std::process::Command;

pub struct AgentConfig {
    pub plan_content: String,
    pub log_content: Option<String>,
    pub session_number: u32,
    pub task: String,
}

pub struct AgentResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

pub fn build_prompt(config: &AgentConfig) -> String {
    let current_time = Local::now().format("%Y-%m-%dT%H:%M:%S");

    let history_section = match &config.log_content {
        Some(log) => format!("\n## Previous Session Log\n\n{log}\n"),
        None => "\n## Previous Session Log\n\nNo previous sessions.\n".to_string(),
    };

    format!(
        r#"# Cryochamber Protocol

You are running inside cryochamber, a long-term task scheduler.
You will be hibernated after this session and woken up later.

## Your Context

Current time: {current_time}
Session number: {session_number}

## Your Plan

{plan}
{history}
## Your Task

{task}

## After Completing Your Task

You MUST write the following markers at the end of your response.

### Required:
[CRYO:EXIT <code>] <one-line summary>
  - 0 = success
  - 1 = partial success
  - 2 = failure

### Optional (write these if the plan has more work):
[CRYO:WAKE <ISO8601 datetime>]       — when to wake up next
[CRYO:CMD <command to run on wake>]   — what to execute (default: re-run same command)
[CRYO:PLAN <note for future self>]    — context you want to remember next session
[CRYO:FALLBACK <action> <target> "<message>"]  — dead man's switch
  - action: email, webhook
  - example: [CRYO:FALLBACK email user@example.com "weekly review did not run"]

### Rules:
- No WAKE marker = plan is complete, no more wake-ups
- Always read the plan and previous session log above before starting
- PLAN markers are your memory — use them to leave notes for yourself
"#,
        session_number = config.session_number,
        plan = config.plan_content,
        history = history_section,
        task = config.task,
    )
}

pub fn run_agent(agent_command: &str, prompt: &str) -> Result<AgentResult> {
    let parts: Vec<&str> = agent_command.split_whitespace().collect();
    let (program, args) = parts.split_first()
        .context("Agent command is empty")?;

    let output = Command::new(program)
        .args(args)
        .arg("--prompt")
        .arg(prompt)
        .output()
        .context(format!("Failed to spawn agent: {agent_command}"))?;

    Ok(AgentResult {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        exit_code: output.status.code().unwrap_or(-1),
    })
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test --test agent_tests`
Expected: all 3 tests PASS

**Step 5: Commit**

```bash
git add src/agent.rs tests/agent_tests.rs
git commit -m "feat: implement agent runner with prompt builder"
```

---

### Task 8: Fallback Actions

**Files:**
- Create: `src/fallback.rs`
- Create: `tests/fallback_tests.rs`

**Step 1: Write failing tests**

```rust
// tests/fallback_tests.rs
use cryochamber::fallback::FallbackAction;

#[test]
fn test_fallback_action_display() {
    let action = FallbackAction {
        action: "email".to_string(),
        target: "user@example.com".to_string(),
        message: "task failed".to_string(),
    };
    let display = format!("{action}");
    assert!(display.contains("email"));
    assert!(display.contains("user@example.com"));
}

#[test]
fn test_fallback_action_is_email() {
    let action = FallbackAction {
        action: "email".to_string(),
        target: "user@example.com".to_string(),
        message: "failed".to_string(),
    };
    assert!(action.is_email());
    assert!(!action.is_webhook());
}

#[test]
fn test_fallback_action_is_webhook() {
    let action = FallbackAction {
        action: "webhook".to_string(),
        target: "https://hooks.slack.com/xxx".to_string(),
        message: "failed".to_string(),
    };
    assert!(!action.is_email());
    assert!(action.is_webhook());
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --test fallback_tests`
Expected: FAIL

**Step 3: Implement fallback module**

```rust
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
        // Use system `mail` command as simplest cross-platform approach
        // Full lettre integration can be added later via cryo.toml SMTP config
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
```

**Step 4: Run tests to verify they pass**

Run: `cargo test --test fallback_tests`
Expected: all 3 tests PASS

**Step 5: Commit**

```bash
git add src/fallback.rs tests/fallback_tests.rs
git commit -m "feat: implement fallback actions (email, webhook)"
```

---

### Task 9: Pre-Hibernate Validation

**Files:**
- Create: `src/validate.rs`
- Create: `tests/validate_tests.rs`

**Step 1: Write failing tests**

```rust
// tests/validate_tests.rs
use cryochamber::validate::{ValidationResult, validate_markers};
use cryochamber::marker::{CryoMarkers, ExitCode};
use chrono::{Local, NaiveDateTime, Duration};

#[test]
fn test_valid_markers() {
    let future = Local::now().naive_local() + Duration::hours(24);
    let markers = CryoMarkers {
        exit_code: Some(ExitCode::Success),
        exit_summary: Some("done".to_string()),
        wake_time: Some(future),
        command: Some("opencode test".to_string()),
        plan_note: None,
        fallbacks: vec![],
    };
    let result = validate_markers(&markers);
    assert!(result.can_hibernate);
    assert!(result.errors.is_empty());
}

#[test]
fn test_wake_time_in_past() {
    let past = NaiveDateTime::parse_from_str("2020-01-01T00:00", "%Y-%m-%dT%H:%M").unwrap();
    let markers = CryoMarkers {
        exit_code: Some(ExitCode::Success),
        exit_summary: Some("done".to_string()),
        wake_time: Some(past),
        command: Some("opencode test".to_string()),
        plan_note: None,
        fallbacks: vec![],
    };
    let result = validate_markers(&markers);
    assert!(!result.can_hibernate);
    assert!(result.errors.iter().any(|e| e.contains("past")));
}

#[test]
fn test_no_exit_marker() {
    let markers = CryoMarkers::default();
    let result = validate_markers(&markers);
    assert!(!result.can_hibernate);
    assert!(result.errors.iter().any(|e| e.contains("EXIT")));
}

#[test]
fn test_no_wake_means_plan_complete() {
    let markers = CryoMarkers {
        exit_code: Some(ExitCode::Success),
        exit_summary: Some("all done".to_string()),
        wake_time: None,
        command: None,
        plan_note: None,
        fallbacks: vec![],
    };
    let result = validate_markers(&markers);
    // No wake = plan complete, this is valid (no hibernate needed)
    assert!(!result.can_hibernate);
    assert!(result.plan_complete);
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --test validate_tests`
Expected: FAIL

**Step 3: Implement validation**

```rust
// src/validate.rs
use crate::marker::CryoMarkers;
use chrono::Local;

pub struct ValidationResult {
    pub can_hibernate: bool,
    pub plan_complete: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

pub fn validate_markers(markers: &CryoMarkers) -> ValidationResult {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    // Check EXIT marker exists
    if markers.exit_code.is_none() {
        errors.push("No [CRYO:EXIT] marker found. Agent must report exit status.".to_string());
    }

    // No WAKE = plan complete
    if markers.wake_time.is_none() {
        return ValidationResult {
            can_hibernate: false,
            plan_complete: true,
            errors: vec![],
            warnings: vec![],
        };
    }

    // Check wake time is in the future
    if let Some(wake) = &markers.wake_time {
        let now = Local::now().naive_local();
        if *wake <= now {
            errors.push(format!(
                "Wake time {} is in the past (now: {})",
                wake.format("%Y-%m-%dT%H:%M"),
                now.format("%Y-%m-%dT%H:%M")
            ));
        }
    }

    // Check command exists (warn if missing, will re-use previous)
    if markers.command.is_none() {
        warnings.push("No [CRYO:CMD] marker. Will re-use previous command.".to_string());
    }

    ValidationResult {
        can_hibernate: errors.is_empty(),
        plan_complete: false,
        errors,
        warnings,
    }
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test --test validate_tests`
Expected: all 4 tests PASS

**Step 5: Commit**

```bash
git add src/validate.rs tests/validate_tests.rs
git commit -m "feat: implement pre-hibernate validation"
```

---

### Task 10: State Management (timer.json)

**Files:**
- Modify: `src/lib.rs`
- Create: `src/state.rs`
- Create: `tests/state_tests.rs`

**Step 1: Write failing tests**

```rust
// tests/state_tests.rs
use cryochamber::state::{CryoState, load_state, save_state};

#[test]
fn test_save_and_load_state() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("timer.json");

    let state = CryoState {
        plan_path: "plan.md".to_string(),
        session_number: 3,
        last_command: Some("opencode test".to_string()),
        wake_timer_id: Some("com.cryochamber.abc.wake".to_string()),
        fallback_timer_id: Some("com.cryochamber.abc.fallback".to_string()),
        pid: Some(std::process::id()),
    };

    save_state(&state_path, &state).unwrap();
    let loaded = load_state(&state_path).unwrap().unwrap();

    assert_eq!(loaded.session_number, 3);
    assert_eq!(loaded.last_command, Some("opencode test".to_string()));
}

#[test]
fn test_load_missing_state() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("nonexistent.json");
    let loaded = load_state(&state_path).unwrap();
    assert!(loaded.is_none());
}

#[test]
fn test_lock_mechanism() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("timer.json");

    let state = CryoState {
        plan_path: "plan.md".to_string(),
        session_number: 1,
        last_command: None,
        wake_timer_id: None,
        fallback_timer_id: None,
        pid: Some(std::process::id()),
    };
    save_state(&state_path, &state).unwrap();

    // Current process PID should be considered "running"
    let loaded = load_state(&state_path).unwrap().unwrap();
    assert_eq!(loaded.pid, Some(std::process::id()));
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --test state_tests`
Expected: FAIL

**Step 3: Implement state module**

```rust
// src/state.rs
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CryoState {
    pub plan_path: String,
    pub session_number: u32,
    pub last_command: Option<String>,
    pub wake_timer_id: Option<String>,
    pub fallback_timer_id: Option<String>,
    pub pid: Option<u32>,
}

pub fn save_state(path: &Path, state: &CryoState) -> Result<()> {
    let json = serde_json::to_string_pretty(state)?;
    std::fs::write(path, json)?;
    Ok(())
}

pub fn load_state(path: &Path) -> Result<Option<CryoState>> {
    if !path.exists() {
        return Ok(None);
    }
    let contents = std::fs::read_to_string(path)?;
    let state: CryoState = serde_json::from_str(&contents)?;
    Ok(Some(state))
}

pub fn is_locked(state: &CryoState) -> bool {
    if let Some(pid) = state.pid {
        // Check if process is still alive
        unsafe {
            libc::kill(pid as i32, 0) == 0
        }
    } else {
        false
    }
}
```

Note: Add `libc = "0.2"` to Cargo.toml dependencies for PID checking.

**Step 4: Add `pub mod state;` to lib.rs and `libc` dependency**

**Step 5: Run tests to verify they pass**

Run: `cargo test --test state_tests`
Expected: all 3 tests PASS

**Step 6: Commit**

```bash
git add src/state.rs tests/state_tests.rs src/lib.rs Cargo.toml
git commit -m "feat: implement state management with lock mechanism"
```

---

### Task 11: CLI Entry Point

**Files:**
- Create: `src/main.rs`

**Step 1: Implement CLI with clap**

```rust
// src/main.rs
use anyhow::{Context, Result};
use chrono::Duration;
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};

use cryochamber::agent::{self, AgentConfig};
use cryochamber::log::{self, Session};
use cryochamber::marker;
use cryochamber::state::{self, CryoState};
use cryochamber::timer;
use cryochamber::validate;

#[derive(Parser)]
#[command(name = "cryochamber", about = "Long-term AI agent task scheduler")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Begin a new plan: initialize and run the first task
    Start {
        /// Path to the natural language plan file
        plan: PathBuf,
        /// Agent command to use (default: opencode)
        #[arg(long, default_value = "opencode")]
        agent: String,
    },
    /// Called by OS timer: execute the next scheduled task
    Wake,
    /// Show current status: next wake time, last result
    Status,
    /// Cancel all timers and stop the schedule
    Cancel,
    /// Run pre-hibernate validation checks
    Validate,
    /// Print the session log
    Log,
    /// Execute a fallback action (used internally by timers)
    FallbackExec {
        action: String,
        target: String,
        message: String,
    },
}

fn work_dir() -> Result<PathBuf> {
    std::env::current_dir().context("Failed to get current directory")
}

fn state_path(dir: &Path) -> PathBuf {
    dir.join("timer.json")
}

fn log_path(dir: &Path) -> PathBuf {
    dir.join("cryo.log")
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Start { plan, agent } => cmd_start(&plan, &agent),
        Commands::Wake => cmd_wake(),
        Commands::Status => cmd_status(),
        Commands::Cancel => cmd_cancel(),
        Commands::Validate => cmd_validate(),
        Commands::Log => cmd_log(),
        Commands::FallbackExec { action, target, message } => {
            let fb = cryochamber::fallback::FallbackAction { action, target, message };
            fb.execute()
        }
    }
}

fn cmd_start(plan_path: &Path, agent_cmd: &str) -> Result<()> {
    let dir = work_dir()?;
    let plan_dest = dir.join("plan.md");

    // Copy plan to working directory if not already there
    if plan_path != plan_dest {
        std::fs::copy(plan_path, &plan_dest)
            .context("Failed to copy plan file")?;
    }

    let plan_content = std::fs::read_to_string(&plan_dest)?;

    // Initialize state
    let mut cryo_state = CryoState {
        plan_path: "plan.md".to_string(),
        session_number: 1,
        last_command: Some(agent_cmd.to_string()),
        wake_timer_id: None,
        fallback_timer_id: None,
        pid: Some(std::process::id()),
    };
    state::save_state(&state_path(&dir), &cryo_state)?;

    println!("Cryochamber initialized. Running first task...");

    // Run the agent
    run_session(&dir, &mut cryo_state, agent_cmd, &plan_content, "Execute the first task from the plan")?;

    Ok(())
}

fn cmd_wake() -> Result<()> {
    let dir = work_dir()?;
    let mut cryo_state = state::load_state(&state_path(&dir))?
        .context("No cryochamber state found. Run 'cryochamber start' first.")?;

    // Check lock
    if state::is_locked(&cryo_state) && cryo_state.pid != Some(std::process::id()) {
        anyhow::bail!("Another cryochamber session is running (PID: {:?})", cryo_state.pid);
    }

    cryo_state.pid = Some(std::process::id());
    cryo_state.session_number += 1;
    state::save_state(&state_path(&dir), &cryo_state)?;

    let plan_content = std::fs::read_to_string(dir.join("plan.md"))?;
    let agent_cmd = cryo_state.last_command.clone()
        .unwrap_or_else(|| "opencode".to_string());

    // Get task from previous session's CMD marker, or default
    let task = get_task_from_log(&dir).unwrap_or_else(|| "Continue the plan".to_string());

    // Cancel the fallback timer (we woke up successfully)
    if let Some(fb_id) = &cryo_state.fallback_timer_id {
        let timer_impl = timer::create_timer()?;
        let _ = timer_impl.cancel(&timer::TimerId(fb_id.clone()));
        cryo_state.fallback_timer_id = None;
    }

    run_session(&dir, &mut cryo_state, &agent_cmd, &plan_content, &task)?;

    Ok(())
}

fn run_session(dir: &Path, cryo_state: &mut CryoState, agent_cmd: &str, plan_content: &str, task: &str) -> Result<()> {
    let log = log_path(dir);

    // Build prompt
    let log_content = cryochamber::log::read_latest_session(&log)?;
    let config = AgentConfig {
        plan_content: plan_content.to_string(),
        log_content: log_content,
        session_number: cryo_state.session_number,
        task: task.to_string(),
    };
    let prompt = agent::build_prompt(&config);

    // Run agent
    println!("Session #{}: Running agent...", cryo_state.session_number);
    let result = agent::run_agent(agent_cmd, &prompt)?;

    if result.exit_code != 0 {
        eprintln!("Agent exited with code {}. Stderr:\n{}", result.exit_code, result.stderr);
    }

    // Append to log
    let session = Session {
        number: cryo_state.session_number,
        task: task.to_string(),
        output: result.stdout.clone(),
    };
    log::append_session(&log, &session)?;

    // Parse markers
    let markers = marker::parse_markers(&result.stdout)?;

    // Validate
    let validation = validate::validate_markers(&markers);

    for warning in &validation.warnings {
        eprintln!("Warning: {warning}");
    }

    if validation.plan_complete {
        println!("Plan complete! No more wake-ups scheduled.");
        cryo_state.pid = None;
        state::save_state(&state_path(dir), cryo_state)?;
        return Ok(());
    }

    for error in &validation.errors {
        eprintln!("Error: {error}");
    }

    if !validation.can_hibernate {
        cryo_state.pid = None;
        state::save_state(&state_path(dir), cryo_state)?;
        anyhow::bail!("Pre-hibernate validation failed. Not hibernating.");
    }

    // Schedule next wake
    let timer_impl = timer::create_timer()?;
    let wake_time = markers.wake_time.unwrap();
    let dir_str = dir.to_string_lossy().to_string();

    let wake_cmd = format!(
        "{} wake",
        std::env::current_exe()?.to_string_lossy()
    );

    let wake_id = timer_impl.schedule_wake(wake_time, &wake_cmd, &dir_str)?;
    cryo_state.wake_timer_id = Some(wake_id.0.clone());

    // Update command for next session
    if let Some(cmd) = &markers.command {
        cryo_state.last_command = Some(cmd.clone());
    }

    // Schedule fallback if specified
    if let Some(fb) = markers.fallbacks.first() {
        let fallback_time = wake_time + Duration::hours(1);
        let fb_id = timer_impl.schedule_fallback(fallback_time, fb, &dir_str)?;
        cryo_state.fallback_timer_id = Some(fb_id.0.clone());
    }

    // Verify timer
    let status = timer_impl.verify(&timer::TimerId(cryo_state.wake_timer_id.clone().unwrap()))?;
    match status {
        timer::TimerStatus::Scheduled { .. } => {
            println!("Hibernating. Next wake: {}", wake_time.format("%Y-%m-%d %H:%M"));
        }
        timer::TimerStatus::NotFound => {
            anyhow::bail!("Timer registration verification failed!");
        }
    }

    // Release lock
    cryo_state.pid = None;
    state::save_state(&state_path(dir), cryo_state)?;

    Ok(())
}

fn get_task_from_log(dir: &Path) -> Option<String> {
    let log = log_path(dir);
    let latest = cryochamber::log::read_latest_session(&log).ok()??;
    let markers = marker::parse_markers(&latest).ok()?;
    markers.command.or(markers.plan_note)
}

fn cmd_status() -> Result<()> {
    let dir = work_dir()?;
    let cryo_state = state::load_state(&state_path(&dir))?;

    match cryo_state {
        None => println!("No cryochamber instance in this directory."),
        Some(state) => {
            println!("Plan: {}", state.plan_path);
            println!("Session: {}", state.session_number);
            println!("Wake timer: {}", state.wake_timer_id.as_deref().unwrap_or("none"));
            println!("Fallback timer: {}", state.fallback_timer_id.as_deref().unwrap_or("none"));
            println!("Locked by PID: {}", state.pid.map(|p| p.to_string()).unwrap_or("none".into()));

            // Show last log entry
            let log = log_path(&dir);
            if let Some(latest) = cryochamber::log::read_latest_session(&log)? {
                println!("\n--- Latest session ---");
                // Print just the last few lines
                let lines: Vec<&str> = latest.lines().collect();
                let start = lines.len().saturating_sub(10);
                for line in &lines[start..] {
                    println!("{line}");
                }
            }
        }
    }
    Ok(())
}

fn cmd_cancel() -> Result<()> {
    let dir = work_dir()?;
    let cryo_state = state::load_state(&state_path(&dir))?
        .context("No cryochamber instance found.")?;

    let timer_impl = timer::create_timer()?;

    if let Some(wake_id) = &cryo_state.wake_timer_id {
        timer_impl.cancel(&timer::TimerId(wake_id.clone()))?;
        println!("Cancelled wake timer: {wake_id}");
    }
    if let Some(fb_id) = &cryo_state.fallback_timer_id {
        timer_impl.cancel(&timer::TimerId(fb_id.clone()))?;
        println!("Cancelled fallback timer: {fb_id}");
    }

    // Remove state file
    let sp = state_path(&dir);
    if sp.exists() {
        std::fs::remove_file(sp)?;
    }

    println!("Cryochamber cancelled.");
    Ok(())
}

fn cmd_validate() -> Result<()> {
    let dir = work_dir()?;
    let log = log_path(&dir);

    let latest = cryochamber::log::read_latest_session(&log)?
        .context("No sessions found in log.")?;
    let markers = marker::parse_markers(&latest)?;
    let result = validate::validate_markers(&markers);

    if result.plan_complete {
        println!("Plan is complete. No validation needed.");
        return Ok(());
    }

    for error in &result.errors {
        println!("ERROR: {error}");
    }
    for warning in &result.warnings {
        println!("WARN:  {warning}");
    }

    if result.can_hibernate {
        println!("\nAll checks passed. Ready to hibernate.");
    } else {
        println!("\nValidation FAILED. Cannot hibernate.");
    }
    Ok(())
}

fn cmd_log() -> Result<()> {
    let dir = work_dir()?;
    let log = log_path(&dir);
    if log.exists() {
        let contents = std::fs::read_to_string(log)?;
        println!("{contents}");
    } else {
        println!("No log file found.");
    }
    Ok(())
}
```

**Step 2: Verify it compiles**

Run: `cargo build`
Expected: success

**Step 3: Commit**

```bash
git add src/main.rs
git commit -m "feat: implement CLI entry point with all subcommands"
```

---

### Task 12: Cryo-Skill Prompt Template

**Files:**
- Create: `cryo-skill.md`

**Step 1: Write the skill template**

Create `cryo-skill.md` at the project root — this is the reference copy of the prompt that gets injected into the AI agent. It's also embedded in the Rust binary via `agent.rs` (already done in Task 7).

```markdown
# Cryochamber Protocol

You are running inside cryochamber, a long-term task scheduler.
You will be hibernated after this session and woken up later.

## Your Context

- Current time: {{CURRENT_TIME}}
- Session number: {{SESSION_NUMBER}}
- Your plan: see the "Your Plan" section below
- Your history: see the "Previous Session Log" section below

## After Completing Your Task

You MUST write the following markers at the end of your response.

### Required:
[CRYO:EXIT <code>] <one-line summary>
  - 0 = success
  - 1 = partial success
  - 2 = failure

### Optional (write these if the plan has more work):
[CRYO:WAKE <ISO8601 datetime>]       — when to wake up next
[CRYO:CMD <command to run on wake>]   — what to execute next
[CRYO:PLAN <note for future self>]    — context to remember
[CRYO:FALLBACK <action> <target> "<message>"]  — dead man's switch
  - action: email, webhook
  - example: [CRYO:FALLBACK email user@example.com "weekly review did not run"]

### Rules:
- No WAKE marker = plan is complete, no more wake-ups
- Always read the plan and previous session log before starting
- PLAN markers are your memory — use them to leave notes for yourself
```

**Step 2: Commit**

```bash
git add cryo-skill.md
git commit -m "docs: add cryo-skill prompt template reference"
```

---

### Task 13: Integration Test

**Files:**
- Create: `tests/integration_test.rs`

**Step 1: Write integration test with mock agent**

```rust
// tests/integration_test.rs
use cryochamber::agent::build_prompt;
use cryochamber::log::{append_session, read_latest_session, session_count, Session};
use cryochamber::marker::parse_markers;
use cryochamber::state::{save_state, load_state, CryoState};
use cryochamber::validate::validate_markers;

/// Simulate a full cycle: build prompt -> "agent output" -> parse -> validate -> log
#[test]
fn test_full_cycle_simulation() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("cryo.log");
    let state_path = dir.path().join("timer.json");

    // Session 1: Start
    let config = cryochamber::agent::AgentConfig {
        plan_content: "Review PRs every Monday morning".to_string(),
        log_content: None,
        session_number: 1,
        task: "Start the PR review plan".to_string(),
    };
    let prompt = build_prompt(&config);
    assert!(prompt.contains("cryochamber"));

    // Simulate agent output
    let agent_output = r#"Reviewed all open PRs. Found 3 PRs ready for review.
Approved PR #42 and #43. Left comments on PR #41.

[CRYO:EXIT 0] Reviewed 3 PRs: approved 2, commented on 1
[CRYO:PLAN PR #41 needs author to fix lint issues]
[CRYO:WAKE 2026-12-08T09:00]
[CRYO:CMD opencode "Follow up on PR #41, check for new PRs"]
[CRYO:FALLBACK email user@example.com "Monday PR review did not run"]"#;

    // Parse markers
    let markers = parse_markers(agent_output).unwrap();
    assert!(markers.exit_code.is_some());
    assert!(markers.wake_time.is_some());
    assert_eq!(markers.fallbacks.len(), 1);

    // Validate
    let validation = validate_markers(&markers);
    assert!(validation.can_hibernate);
    assert!(!validation.plan_complete);

    // Append to log
    let session = Session {
        number: 1,
        task: "Start the PR review plan".to_string(),
        output: agent_output.to_string(),
    };
    append_session(&log_path, &session).unwrap();
    assert_eq!(session_count(&log_path).unwrap(), 1);

    // Save state
    let state = CryoState {
        plan_path: "plan.md".to_string(),
        session_number: 1,
        last_command: Some("opencode".to_string()),
        wake_timer_id: Some("com.cryochamber.test.wake".to_string()),
        fallback_timer_id: Some("com.cryochamber.test.fallback".to_string()),
        pid: None,
    };
    save_state(&state_path, &state).unwrap();

    // Session 2: Wake
    let latest = read_latest_session(&log_path).unwrap().unwrap();
    assert!(latest.contains("Reviewed 3 PRs"));

    let config2 = cryochamber::agent::AgentConfig {
        plan_content: "Review PRs every Monday morning".to_string(),
        log_content: Some(latest),
        session_number: 2,
        task: "Follow up on PR #41, check for new PRs".to_string(),
    };
    let prompt2 = build_prompt(&config2);
    assert!(prompt2.contains("Session number: 2"));
    assert!(prompt2.contains("Reviewed 3 PRs"));

    // Simulate agent completing the plan
    let agent_output2 = r#"PR #41 has been fixed and merged. No new PRs open.
All caught up!

[CRYO:EXIT 0] All PRs reviewed and merged"#;

    let markers2 = parse_markers(agent_output2).unwrap();
    let validation2 = validate_markers(&markers2);
    assert!(!validation2.can_hibernate);
    assert!(validation2.plan_complete); // No WAKE = done

    let session2 = Session {
        number: 2,
        task: "Follow up on PR #41".to_string(),
        output: agent_output2.to_string(),
    };
    append_session(&log_path, &session2).unwrap();
    assert_eq!(session_count(&log_path).unwrap(), 2);
}

#[test]
fn test_agent_failure_cycle() {
    let agent_output = "Something went wrong, couldn't connect.\n\n[CRYO:EXIT 2] Failed to connect to GitHub API";
    let markers = parse_markers(agent_output).unwrap();
    let validation = validate_markers(&markers);
    // EXIT 2 + no WAKE = plan complete (agent gave up)
    assert!(validation.plan_complete);
}

#[test]
fn test_no_markers_output() {
    let agent_output = "I did some stuff but forgot to write markers";
    let markers = parse_markers(agent_output).unwrap();
    let validation = validate_markers(&markers);
    assert!(!validation.can_hibernate);
    assert!(validation.errors.iter().any(|e| e.contains("EXIT")));
}
```

**Step 2: Run all tests**

Run: `cargo test`
Expected: all tests PASS

**Step 3: Commit**

```bash
git add tests/integration_test.rs
git commit -m "test: add integration tests for full wake/hibernate cycle"
```

---

### Task 14: Final Polish

**Step 1: Run clippy**

Run: `cargo clippy -- -W clippy::all`
Expected: no errors (fix any warnings)

**Step 2: Run fmt**

Run: `cargo fmt`

**Step 3: Build release binary**

Run: `cargo build --release`
Expected: binary at `target/release/cryochamber`

**Step 4: Quick smoke test**

```bash
mkdir /tmp/cryo-test && cd /tmp/cryo-test
echo "Every Monday at 9am, check for new GitHub issues in my repo" > plan.md
# Don't run 'start' yet (requires opencode), just verify the binary runs
../../path/to/cryochamber status
../../path/to/cryochamber --help
```

**Step 5: Commit**

```bash
git add -A
git commit -m "chore: clippy fixes and formatting"
```
