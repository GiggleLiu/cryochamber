use anyhow::Result;
use chrono::{Local, NaiveDateTime, NaiveTime, Utc};
use std::collections::BTreeMap;
use std::path::Path;

use crate::log::{self, SessionOutcome};
use crate::message::{self, Message};

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

    message::ensure_dirs(work_dir)?;
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
    message::write_message(work_dir, "outbox", &msg)
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
mod tests {
    use super::*;
    use crate::log::EventLogger;
    use chrono::Timelike;

    #[test]
    fn test_generate_report_counts() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("cryo.log");

        // 2 success + 1 failure
        let mut logger = EventLogger::begin(&log_path, 1, "t1", "agent", &[]).unwrap();
        logger.log_event("agent started (pid 1)").unwrap();
        logger.log_event("agent exited (code 0)").unwrap();
        logger.finish("session complete").unwrap();

        let mut logger = EventLogger::begin(&log_path, 2, "t2", "agent", &[]).unwrap();
        logger.log_event("agent started (pid 2)").unwrap();
        logger.log_event("agent exited (code 1)").unwrap();
        logger.finish("agent exited without hibernate").unwrap();

        let mut logger = EventLogger::begin(&log_path, 3, "t3", "agent", &[]).unwrap();
        logger.log_event("agent started (pid 3)").unwrap();
        logger
            .log_event("hibernate: wake=2026-03-01T09:00, exit=0")
            .unwrap();
        logger.log_event("agent exited (code 0)").unwrap();
        logger.finish("session complete").unwrap();

        // Session 4: exit code 0 but without hibernate — should be failure
        let mut logger = EventLogger::begin(&log_path, 4, "t4", "agent", &[]).unwrap();
        logger.log_event("agent started (pid 4)").unwrap();
        logger.log_event("agent exited (code 0)").unwrap();
        logger.finish("agent exited without hibernate").unwrap();

        let since =
            NaiveDateTime::parse_from_str("2020-01-01T00:00:00Z", "%Y-%m-%dT%H:%M:%SZ").unwrap();
        let report = generate_report(&log_path, since).unwrap();
        assert_eq!(report.total_sessions, 4);
        assert_eq!(report.failed_sessions, 2);
    }

    #[test]
    fn test_generate_report_empty_log() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("cryo.log");

        let since =
            NaiveDateTime::parse_from_str("2020-01-01T00:00:00Z", "%Y-%m-%dT%H:%M:%SZ").unwrap();
        let report = generate_report(&log_path, since).unwrap();
        assert_eq!(report.total_sessions, 0);
        assert_eq!(report.failed_sessions, 0);
    }

    #[test]
    fn test_write_report_to_outbox_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let summary = ReportSummary {
            total_sessions: 8,
            failed_sessions: 1,
            period_hours: 24,
        };
        let path = write_report_to_outbox(dir.path(), &summary, "my-project").unwrap();

        assert!(path.exists(), "outbox file should exist");
        assert!(
            path.starts_with(dir.path().join("messages").join("outbox")),
            "file should be in messages/outbox/: {}",
            path.display()
        );
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(
            content.contains("Cryochamber Report: my-project"),
            "subject missing from outbox file: {content}"
        );
        assert!(
            content.contains("Last 1d: 8 sessions, 1 failed"),
            "body missing from outbox file: {content}"
        );
    }

    #[test]
    fn test_write_report_to_outbox_period_labels() {
        let dir = tempfile::tempdir().unwrap();

        let hourly = ReportSummary {
            total_sessions: 1,
            failed_sessions: 0,
            period_hours: 3,
        };
        let p = write_report_to_outbox(dir.path(), &hourly, "p").unwrap();
        assert!(std::fs::read_to_string(&p).unwrap().contains("Last 3h:"));

        let weekly = ReportSummary {
            total_sessions: 50,
            failed_sessions: 2,
            period_hours: 168,
        };
        let p = write_report_to_outbox(dir.path(), &weekly, "p").unwrap();
        assert!(std::fs::read_to_string(&p).unwrap().contains("Last 1w:"));
    }

    #[test]
    fn test_compute_next_report_disabled() {
        assert_eq!(compute_next_report_time("09:00", 0, None), None);
    }

    #[test]
    fn test_compute_next_report_no_last_report() {
        let next = compute_next_report_time("09:00", 24, None);
        assert!(next.is_some());
        let next = next.unwrap();
        let now = Local::now().naive_local();
        assert!(next > now);
        assert_eq!(next.time().hour(), 9);
        assert_eq!(next.time().minute(), 0);
    }

    #[test]
    fn test_compute_next_report_with_last_report() {
        let last = Local::now().naive_local() - chrono::Duration::hours(25);
        let next = compute_next_report_time("09:00", 24, Some(last)).unwrap();
        let now = Local::now().naive_local();
        assert!(next > now);
        // Wall-clock aligned: should land on 09:00
        assert_eq!(next.time().hour(), 9);
        assert_eq!(next.time().minute(), 0);
        // Must be at least 24h after last report
        assert!(next >= last + chrono::Duration::hours(24));
    }

    #[test]
    fn test_compute_next_report_invalid_time() {
        // Invalid report_time should return None
        assert_eq!(compute_next_report_time("invalid", 24, None), None);
        assert_eq!(compute_next_report_time("25:99", 24, None), None);
        assert_eq!(compute_next_report_time("", 24, None), None);
    }

    #[test]
    fn test_compute_next_report_recent_last() {
        // Last report was 1 hour ago with 24h interval → next should be wall-clock aligned
        let last = Local::now().naive_local() - chrono::Duration::hours(1);
        let next = compute_next_report_time("09:00", 24, Some(last)).unwrap();
        let now = Local::now().naive_local();
        assert!(next > now);
        // Wall-clock aligned at 09:00
        assert_eq!(next.time().hour(), 9);
        assert_eq!(next.time().minute(), 0);
        // Must be at least 24h after last report
        assert!(next >= last + chrono::Duration::hours(24));
    }
}
