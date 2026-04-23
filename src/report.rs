use anyhow::Result;
use chrono::{Local, NaiveDateTime, NaiveTime, Utc};
use std::collections::BTreeMap;
use std::path::Path;

use crate::channel::store::MessageStore;
use crate::log::{self, SessionOutcome};
use crate::message::Message;

/// Aggregated report for a time period.
#[derive(Debug, Clone)]
pub struct ReportSummary {
    pub total_sessions: usize,
    pub failed_sessions: usize,
    pub period_hours: u64,
}

/// Generate a report summarizing sessions in the given time window.
pub fn generate_report(log_path: &Path, since: NaiveDateTime) -> Result<ReportSummary> {
    let summaries = log::parse_sessions_since(log_path, since)?;
    let failed = summaries
        .iter()
        .filter(|s| {
            matches!(
                s.outcome,
                SessionOutcome::Failed | SessionOutcome::Interrupted
            )
        })
        .count();
    let now = Utc::now().naive_utc();
    let period_hours = (now - since).num_hours().max(0) as u64;
    Ok(ReportSummary {
        total_sessions: summaries.len(),
        failed_sessions: failed,
        period_hours,
    })
}

/// Write the periodic report to `messages/outbox/` so it flows through whichever
/// sync channel the user has configured (Zulip, GitHub Discussions, external
/// watcher, etc.). Returns the path of the written message.
pub fn write_report_to_outbox(
    work_dir: &Path,
    summary: &ReportSummary,
    project_name: &str,
) -> Result<std::path::PathBuf> {
    let period_label = match summary.period_hours {
        0..=23 => format!("{}h", summary.period_hours),
        24..=167 => format!("{}d", summary.period_hours / 24),
        _ => format!("{}w", summary.period_hours / 168),
    };
    let body = format!(
        "Last {}: {} sessions, {} failed",
        period_label, summary.total_sessions, summary.failed_sessions,
    );

    let msg = Message {
        from: "cryochamber".to_string(),
        subject: format!("Cryochamber Report: {}", project_name),
        body,
        timestamp: Local::now().naive_local(),
        metadata: BTreeMap::from([
            (
                "total_sessions".to_string(),
                summary.total_sessions.to_string(),
            ),
            (
                "failed_sessions".to_string(),
                summary.failed_sessions.to_string(),
            ),
            ("period_hours".to_string(), summary.period_hours.to_string()),
        ]),
    };
    MessageStore::new(work_dir.to_path_buf()).send_out(&msg)
}

/// Compute the next report time based on config and last report.
/// Returns None if reporting is disabled (interval == 0) or if report_time
/// is invalid (not a valid HH:MM string).
///
/// Reports are aligned to the configured wall-clock `report_time`. When a
/// `last_report` is provided, the next time is the earliest wall-clock-aligned
/// slot that is both in the future and at least `interval_hours` after the last
/// report. This prevents drift when reports are sent late (e.g., after machine
/// suspend).
pub fn compute_next_report_time(
    report_time: &str,
    interval_hours: u64,
    last_report: Option<NaiveDateTime>,
) -> Option<NaiveDateTime> {
    if interval_hours == 0 {
        return None;
    }

    let time = NaiveTime::parse_from_str(report_time, "%H:%M").ok()?;
    let now = chrono::Local::now().naive_local();
    let interval = chrono::Duration::hours(interval_hours as i64);

    // Start from the next wall-clock time aligned to report_time
    let mut next = now.date().and_time(time);
    if next <= now {
        next += interval;
    }

    if let Some(last) = last_report {
        // Ensure at least interval since last report, staying wall-clock aligned
        let min_next = last + interval;
        while next < min_next {
            next += interval;
        }
    }

    Some(next)
}

#[cfg(test)]
#[path = "unit_tests/report.rs"]
mod tests;
