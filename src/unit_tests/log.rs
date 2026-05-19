use super::*;
use chrono::Timelike;

#[test]
fn test_event_logger_session_lifecycle() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("cryo.log");

    let mut logger = EventLogger::begin(
        &log_path,
        3,
        "Continue parser",
        "claude -p",
        &["feature.md".to_string(), "bug.md".to_string()],
    )
    .unwrap();

    logger.log_event("agent started (pid 12345)").unwrap();
    logger.log_event("note: \"Finished parsing\"").unwrap();
    logger
        .log_event("hibernate: wake=2026-03-09T09:00, exit=0")
        .unwrap();
    logger.finish("agent exited (code 0)").unwrap();

    let content = std::fs::read_to_string(&log_path).unwrap();
    assert!(content.contains("--- CRYO SESSION 3"));
    assert!(content.contains("task: Continue parser"));
    assert!(content.contains("agent: claude -p"));
    assert!(content.contains("inbox: 2 messages (feature.md, bug.md)"));
    assert!(content.contains("note: \"Finished parsing\""));
    assert!(content.contains("--- CRYO END ---"));
}

#[test]
fn test_parse_sessions_since_counts_correctly() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("cryo.log");

    // Session 1: success (agent exited code 0)
    let mut logger = EventLogger::begin(&log_path, 1, "task1", "claude", &[]).unwrap();
    logger.log_event("agent started (pid 100)").unwrap();
    logger
        .log_event("hibernate: wake=2026-03-01T09:00, exit=0")
        .unwrap();
    logger.log_event("agent exited (code 0)").unwrap();
    logger.finish("session complete").unwrap();

    // Session 2: failure (agent exited code 1)
    let mut logger = EventLogger::begin(&log_path, 2, "task2", "claude", &[]).unwrap();
    logger.log_event("agent started (pid 200)").unwrap();
    logger.log_event("agent exited (code 1)").unwrap();
    logger.finish("agent exited without hibernate").unwrap();

    // Session 3: quick exit
    let mut logger = EventLogger::begin(&log_path, 3, "task3", "claude", &[]).unwrap();
    logger.log_event("agent started (pid 300)").unwrap();
    logger
        .log_event("quick exit detected (0.5s without hibernate)")
        .unwrap();
    logger.log_event("agent exited (code 0)").unwrap();
    logger.finish("agent exited without hibernate").unwrap();

    let since = chrono::NaiveDateTime::parse_from_str("2020-01-01T00:00:00Z", "%Y-%m-%dT%H:%M:%SZ")
        .unwrap();
    let summaries = parse_sessions_since(&log_path, since).unwrap();
    assert_eq!(summaries.len(), 3);
    assert_eq!(summaries[0].outcome, SessionOutcome::Success);
    assert_eq!(summaries[1].outcome, SessionOutcome::Failed);
    assert_eq!(summaries[2].outcome, SessionOutcome::Failed);
}

#[test]
fn test_parse_sessions_since_filters_by_time() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("cryo.log");

    let mut logger = EventLogger::begin(&log_path, 1, "old task", "claude", &[]).unwrap();
    logger.log_event("agent started (pid 100)").unwrap();
    logger.finish("session complete").unwrap();

    // Use a 'since' in the far future to filter out all sessions
    let since = chrono::NaiveDateTime::parse_from_str("2099-01-01T00:00:00Z", "%Y-%m-%dT%H:%M:%SZ")
        .unwrap();
    let summaries = parse_sessions_since(&log_path, since).unwrap();
    assert_eq!(summaries.len(), 0);
}

#[test]
fn test_parse_sessions_since_interrupted() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("cryo.log");

    {
        let mut logger = EventLogger::begin(&log_path, 1, "task", "claude", &[]).unwrap();
        logger.log_event("agent started (pid 100)").unwrap();
        // Drop without finish → CRYO INTERRUPTED
    }

    let since = chrono::NaiveDateTime::parse_from_str("2020-01-01T00:00:00Z", "%Y-%m-%dT%H:%M:%SZ")
        .unwrap();
    let summaries = parse_sessions_since(&log_path, since).unwrap();
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].outcome, SessionOutcome::Interrupted);
}

#[test]
fn test_parse_sessions_exit_code_0_without_hibernate_is_failure() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("cryo.log");

    // Agent exits with code 0 but without hibernating — daemon treats as failure
    let mut logger = EventLogger::begin(&log_path, 1, "task", "claude", &[]).unwrap();
    logger.log_event("agent started (pid 100)").unwrap();
    logger.log_event("agent exited (code 0)").unwrap();
    logger.finish("agent exited without hibernate").unwrap();

    let since = chrono::NaiveDateTime::parse_from_str("2020-01-01T00:00:00Z", "%Y-%m-%dT%H:%M:%SZ")
        .unwrap();
    let summaries = parse_sessions_since(&log_path, since).unwrap();
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].outcome, SessionOutcome::Failed);
}

#[test]
fn test_classify_session_outcome_prioritizes_failure_markers() {
    assert_eq!(
        classify_session_outcome(
            "[12:00:01] hibernate: wake=2026-03-01T14:00, exit=0\n\
             [12:00:02] quick exit detected (0.5s without hibernate)"
        ),
        SessionOutcome::Failed
    );
    assert_eq!(
        classify_session_outcome(
            "--- CRYO INTERRUPTED ---\n\
             [12:00:01] hibernate: wake=2026-03-01T14:00, exit=0"
        ),
        SessionOutcome::Interrupted
    );
    assert_eq!(
        classify_session_outcome("[12:00:01] hibernate: wake=2026-03-01T14:00, exit=0"),
        SessionOutcome::Success
    );
    assert_eq!(
        classify_session_outcome("[12:00:01] agent exited (code 1)"),
        SessionOutcome::Failed
    );
}

#[test]
fn test_parse_sessions_since_empty_log() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("cryo.log");

    let since = chrono::NaiveDateTime::parse_from_str("2020-01-01T00:00:00Z", "%Y-%m-%dT%H:%M:%SZ")
        .unwrap();
    let summaries = parse_sessions_since(&log_path, since).unwrap();
    assert_eq!(summaries.len(), 0);
}

#[test]
fn test_daily_digests_group_sessions_by_log_date_descending() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("cryo.log");
    std::fs::write(
        &log_path,
        "--- CRYO SESSION 1 | 2026-03-01T09:00:00Z ---\n\
         [09:00:01] hibernate: wake=2026-03-01T14:00, exit=0\n\
         --- CRYO END ---\n\
         --- CRYO SESSION 2 | 2026-03-01T12:00:00Z ---\n\
         [12:00:01] agent exited without hibernate\n\
         --- CRYO END ---\n\
         --- CRYO SESSION 3 | 2026-03-02T08:00:00Z ---\n\
         [08:00:01] hibernate: wake=2026-03-02T14:00, exit=0\n\
         --- CRYO END ---\n",
    )
    .unwrap();

    let digests = daily_digests(&log_path, 10).unwrap();

    assert_eq!(digests.len(), 2);
    assert_eq!(digests[0].date, "2026-03-02");
    assert_eq!(digests[0].total_sessions, 1);
    assert_eq!(digests[0].failed_sessions, 0);
    assert_eq!(digests[0].latest_session, 3);
    assert_eq!(digests[1].date, "2026-03-01");
    assert_eq!(digests[1].total_sessions, 2);
    assert_eq!(digests[1].failed_sessions, 1);
    assert_eq!(digests[1].latest_session, 2);
}

#[test]
fn test_daily_digests_limit_days() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("cryo.log");
    std::fs::write(
        &log_path,
        "--- CRYO SESSION 1 | 2026-03-01T09:00:00Z ---\n\
         [09:00:01] hibernate: wake=2026-03-01T14:00, exit=0\n\
         --- CRYO END ---\n\
         --- CRYO SESSION 2 | 2026-03-02T09:00:00Z ---\n\
         [09:00:01] hibernate: wake=2026-03-02T14:00, exit=0\n\
         --- CRYO END ---\n",
    )
    .unwrap();

    let digests = daily_digests(&log_path, 1).unwrap();

    assert_eq!(digests.len(), 1);
    assert_eq!(digests[0].date, "2026-03-02");
}

#[test]
fn test_daily_digests_empty_for_missing_log() {
    let dir = tempfile::tempdir().unwrap();
    let digests = daily_digests(&dir.path().join("cryo.log"), 10).unwrap();

    assert!(digests.is_empty());
}

#[test]
fn test_event_logger_drop_writes_interrupted() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("cryo.log");

    {
        let mut logger = EventLogger::begin(&log_path, 1, "test", "agent", &[]).unwrap();
        logger.log_event("started").unwrap();
        // Drop without finish
    }

    let content = std::fs::read_to_string(&log_path).unwrap();
    assert!(content.contains("CRYO INTERRUPTED"));
}

#[test]
fn test_read_current_session_incomplete() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cryo.log");
    // Session without a CRYO END marker — still in progress
    let content = "--- CRYO SESSION 1 | 2026-03-01T12:00:00Z ---\n\
                   [12:00:01] agent started\n\
                   [12:00:05] note: \"doing work\"\n";
    std::fs::write(&path, content).unwrap();
    let result = read_current_session(&path).unwrap();
    assert!(
        result.is_some(),
        "Should return content for incomplete session"
    );
    let session = result.unwrap();
    assert!(session.contains("agent started"));
    assert!(session.contains("doing work"));
}

#[test]
fn test_read_latest_session_end_before_start() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cryo.log");
    // Only an END marker with no preceding START — should return None
    let content = "--- CRYO END ---\nsome orphaned content\n";
    std::fs::write(&path, content).unwrap();
    let result = read_latest_session(&path).unwrap();
    assert!(result.is_none(), "END before START should return None");
}

#[test]
fn test_read_recent_sessions_returns_last_n() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cryo.log");
    let mut content = String::new();
    for i in 1..=7 {
        content.push_str(&format!(
            "--- CRYO SESSION {i} | 2026-03-01T{:02}:00:00Z ---\n\
             [xx:xx:xx] body s{i}\n\
             --- CRYO END ---\n",
            9 + i
        ));
    }
    std::fs::write(&path, &content).unwrap();

    let recent = read_recent_sessions(&path, 5).unwrap().unwrap();
    for i in 3..=7 {
        assert!(
            recent.contains(&format!("body s{i}")),
            "session {i} should be included in last-5 window"
        );
    }
    for i in 1..=2 {
        assert!(
            !recent.contains(&format!("body s{i}")),
            "session {i} should be outside last-5 window"
        );
    }
}

#[test]
fn test_read_recent_sessions_returns_all_when_fewer_than_n() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cryo.log");
    let content = "--- CRYO SESSION 1 | 2026-03-01T12:00:00Z ---\n\
                   [12:00:01] only session\n\
                   --- CRYO END ---\n";
    std::fs::write(&path, content).unwrap();
    let recent = read_recent_sessions(&path, 5).unwrap().unwrap();
    assert!(recent.contains("only session"));
}

#[test]
fn test_read_recent_sessions_empty_log_returns_none() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cryo.log");
    assert!(read_recent_sessions(&path, 5).unwrap().is_none());
    std::fs::write(&path, "").unwrap();
    assert!(read_recent_sessions(&path, 5).unwrap().is_none());
}

#[test]
fn test_read_latest_session_multiple() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cryo.log");
    let content = "--- CRYO SESSION 1 | 2026-03-01T12:00:00Z ---\n\
                   [12:00:01] first session\n\
                   --- CRYO END ---\n\
                   --- CRYO SESSION 2 | 2026-03-01T13:00:00Z ---\n\
                   [13:00:01] second session\n\
                   --- CRYO END ---\n\
                   --- CRYO SESSION 3 | 2026-03-01T14:00:00Z ---\n\
                   [14:00:01] third session\n\
                   --- CRYO END ---\n";
    std::fs::write(&path, content).unwrap();
    let result = read_latest_session(&path).unwrap().unwrap();
    assert!(
        result.contains("third session"),
        "Should return only last session"
    );
    assert!(!result.contains("first session"));
    assert!(!result.contains("second session"));
}

#[test]
fn test_parse_wake_valid() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cryo.log");
    // Actual format: hibernate: wake=TIME, exit=0
    let content = "--- CRYO SESSION 1 | 2026-03-01T12:00:00Z ---\n\
                   [12:00:01] hibernate: wake=2026-03-01T14:00, exit=0\n\
                   --- CRYO END ---\n";
    std::fs::write(&path, content).unwrap();
    let wake = parse_latest_session_wake(&path).unwrap();
    assert_eq!(wake, Some("2026-03-01T14:00".to_string()));
}

#[test]
fn test_parse_wake_no_wake_line() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cryo.log");
    // Session with no hibernate line at all
    let content = "--- CRYO SESSION 1 | 2026-03-01T12:00:00Z ---\n\
                   [12:00:01] plan complete\n\
                   --- CRYO END ---\n";
    std::fs::write(&path, content).unwrap();
    let wake = parse_latest_session_wake(&path).unwrap();
    assert!(wake.is_none(), "No wake line should return None");
}

#[test]
fn test_parse_wake_wrong_format() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cryo.log");
    // Uses "hibernate, wake_time:" instead of "hibernate: wake=" — parser won't match
    let content = "--- CRYO SESSION 1 | 2026-03-01T12:00:00Z ---\n\
                   [12:00:01] hibernate, wake_time: 2026-03-01T14:00\n\
                   --- CRYO END ---\n";
    std::fs::write(&path, content).unwrap();
    let wake = parse_latest_session_wake(&path).unwrap();
    // The parser looks for "hibernate: wake=" — this format doesn't match
    assert!(
        wake.is_none(),
        "Wrong format should not be recognized as a wake line"
    );
}

#[test]
fn test_parse_task_present() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cryo.log");
    // parse_latest_session_task uses strip_prefix("task: ") on each line,
    // so the task line must start with "task: " (as EventLogger.begin writes it)
    let content = "--- CRYO SESSION 1 | 2026-03-01T12:00:00Z ---\n\
                   task: implement auth\n\
                   [12:00:02] agent started\n\
                   --- CRYO END ---\n";
    std::fs::write(&path, content).unwrap();
    let task = parse_latest_session_task(&path).unwrap();
    assert!(task.is_some(), "Should find task line");
    assert_eq!(task.unwrap(), "implement auth");
}

#[test]
fn test_parse_plan_complete_present() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cryo.log");
    let content = "--- CRYO SESSION 1 | 2026-03-01T12:00:00Z ---\n\
                   task: ship it\n\
                   [12:00:01] agent started (pid 1)\n\
                   [12:05:00] hibernate: plan complete, exit=0, summary=\"All tests green\"\n\
                   --- CRYO END ---\n";
    std::fs::write(&path, content).unwrap();
    let summary = parse_latest_session_plan_complete(&path).unwrap();
    assert_eq!(summary, Some("All tests green".into()));
}

#[test]
fn test_parse_latest_session_summary_from_hibernate_wake() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cryo.log");
    let content = "--- CRYO SESSION 1 | 2026-03-01T12:00:00Z ---\n\
                   [12:05:00] hibernate: wake=2026-03-01T14:00, exit=0, summary=\"old\"\n\
                   --- CRYO END ---\n\
                   --- CRYO SESSION 2 | 2026-03-01T14:00:00Z ---\n\
                   [14:05:00] hibernate: wake=2026-03-01T15:00, exit=0, summary=\"Checked disk usage and scheduled the next warning check\"\n\
                   --- CRYO END ---\n";
    std::fs::write(&path, content).unwrap();
    let summary = parse_latest_session_summary(&path).unwrap();
    assert_eq!(
        summary,
        Some("Checked disk usage and scheduled the next warning check".into())
    );
}

#[test]
fn test_parse_latest_session_summary_ignores_old_session() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cryo.log");
    let content = "--- CRYO SESSION 1 | 2026-03-01T12:00:00Z ---\n\
                   [12:05:00] hibernate: wake=2026-03-01T14:00, exit=0, summary=\"old\"\n\
                   --- CRYO END ---\n\
                   --- CRYO SESSION 2 | 2026-03-01T14:00:00Z ---\n\
                   [14:05:00] agent started\n\
                   --- CRYO END ---\n";
    std::fs::write(&path, content).unwrap();
    assert!(parse_latest_session_summary(&path).unwrap().is_none());
}

#[test]
fn test_parse_plan_complete_absent() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cryo.log");
    let content = "--- CRYO SESSION 1 | 2026-03-01T12:00:00Z ---\n\
                   [12:00:01] agent started\n\
                   [12:05:00] hibernate: wake=2026-03-01T14:00, exit=0, summary=\"working\"\n\
                   --- CRYO END ---\n";
    std::fs::write(&path, content).unwrap();
    assert!(parse_latest_session_plan_complete(&path).unwrap().is_none());
}

#[test]
fn test_parse_plan_complete_only_latest_session() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cryo.log");
    let content = "--- CRYO SESSION 1 | 2026-03-01T12:00:00Z ---\n\
                   [12:05:00] hibernate: plan complete, exit=0, summary=\"old\"\n\
                   --- CRYO END ---\n\
                   --- CRYO SESSION 2 | 2026-03-01T14:00:00Z ---\n\
                   [14:05:00] hibernate: wake=2026-03-01T15:00, exit=0, summary=\"resumed\"\n\
                   --- CRYO END ---\n";
    std::fs::write(&path, content).unwrap();
    assert!(parse_latest_session_plan_complete(&path).unwrap().is_none());
}

#[test]
fn test_parse_task_absent() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cryo.log");
    let content = "--- CRYO SESSION 1 | 2026-03-01T12:00:00Z ---\n\
                   [12:00:01] agent started\n\
                   --- CRYO END ---\n";
    std::fs::write(&path, content).unwrap();
    let task = parse_latest_session_task(&path).unwrap();
    assert!(task.is_none(), "No task line should return None");
}

#[test]
fn test_parse_session_header_valid() {
    let result = parse_session_header("--- CRYO SESSION 5 | 2026-03-01T14:30:45Z ---");
    assert!(result.is_some());
    let (num, ts) = result.unwrap();
    assert_eq!(num, 5);
    assert_eq!(ts.hour(), 14);
    assert_eq!(ts.minute(), 30);
}

#[test]
fn test_parse_session_header_malformed() {
    // Non-numeric session number
    assert!(parse_session_header("--- CRYO SESSION abc | 2026-03-01T14:30:45Z ---").is_none());
    // Invalid timestamp
    assert!(parse_session_header("--- CRYO SESSION 5 | not-a-date ---").is_none());
    // Completely unrelated text
    assert!(parse_session_header("random text").is_none());
}

#[test]
fn test_parse_sessions_since_filters_by_date() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cryo.log");
    let content = "--- CRYO SESSION 1 | 2026-02-27T10:00:00Z ---\n\
                   [10:00:01] plan complete\n\
                   --- CRYO END ---\n\
                   --- CRYO SESSION 2 | 2026-02-28T10:00:00Z ---\n\
                   [10:00:01] plan complete\n\
                   --- CRYO END ---\n\
                   --- CRYO SESSION 3 | 2026-03-01T10:00:00Z ---\n\
                   [10:00:01] agent exited without hibernate\n\
                   --- CRYO END ---\n";
    std::fs::write(&path, content).unwrap();

    let since = chrono::NaiveDate::from_ymd_opt(2026, 2, 28)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap();
    let sessions = parse_sessions_since(&path, since).unwrap();
    assert_eq!(sessions.len(), 2, "Should return sessions 2 and 3");
    assert_eq!(sessions[0].session_number, 2);
    assert_eq!(sessions[1].session_number, 3);
}
