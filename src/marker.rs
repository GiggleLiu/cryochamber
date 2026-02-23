// src/marker.rs
use anyhow::Result;
use chrono::{Datelike, NaiveDateTime, Timelike};
use regex::Regex;

use crate::fallback::FallbackAction;

#[derive(Debug, Clone, PartialEq)]
pub enum ExitCode {
    Success, // 0
    Partial, // 1
    Failure, // 2
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

/// Wrapper around NaiveDateTime that provides inherent accessor methods.
#[derive(Debug, Clone)]
pub struct WakeTime(pub NaiveDateTime);

impl WakeTime {
    pub fn month(&self) -> u32 {
        self.0.month()
    }

    pub fn day(&self) -> u32 {
        self.0.day()
    }

    pub fn hour(&self) -> u32 {
        self.0.hour()
    }

    pub fn minute(&self) -> u32 {
        self.0.minute()
    }

    pub fn inner(&self) -> &NaiveDateTime {
        &self.0
    }
}

#[derive(Debug, Clone, Default)]
pub struct CryoMarkers {
    pub exit_code: Option<ExitCode>,
    pub exit_summary: Option<String>,
    pub wake_time: Option<WakeTime>,
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
            let parsed = NaiveDateTime::parse_from_str(time_str, "%Y-%m-%dT%H:%M:%S")
                .or_else(|_| NaiveDateTime::parse_from_str(time_str, "%Y-%m-%dT%H:%M"))
                .ok();
            markers.wake_time = parsed.map(WakeTime);
        }
        if let Some(cap) = cmd_re.captures(line) {
            markers.command = Some(cap[1].trim().to_string());
        }
        if let Some(cap) = plan_re.captures(line) {
            let plan = cap[1].trim().to_string();
            // Strip trailing ] if present (PLAN marker may or may not have closing bracket)
            let plan = plan.strip_suffix(']').unwrap_or(&plan).trim().to_string();
            markers.plan_note = Some(plan);
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
