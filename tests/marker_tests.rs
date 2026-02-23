// tests/marker_tests.rs
use cryochamber::marker::{parse_markers, ExitCode};

#[test]
fn test_parse_exit_success() {
    let text = "[CRYO:EXIT 0] All tasks completed";
    let markers = parse_markers(text).unwrap();
    assert_eq!(markers.exit_code, Some(ExitCode::Success));
    assert_eq!(
        markers.exit_summary,
        Some("All tasks completed".to_string())
    );
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
    assert_eq!(
        markers.command,
        Some(r#"opencode "check PR #42""#.to_string())
    );
}

#[test]
fn test_parse_plan() {
    let text = "[CRYO:PLAN waiting on CI, check status first]";
    let markers = parse_markers(text).unwrap();
    assert_eq!(
        markers.plan_note,
        Some("waiting on CI, check status first".to_string())
    );
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

#[test]
fn test_exit_code_as_code() {
    assert_eq!(ExitCode::Success.as_code(), 0);
    assert_eq!(ExitCode::Partial.as_code(), 1);
    assert_eq!(ExitCode::Failure.as_code(), 2);

    // Roundtrip
    for code in 0..=2u8 {
        let ec = ExitCode::from_code(code).unwrap();
        assert_eq!(ec.as_code(), code);
    }
}

#[test]
fn test_exit_code_from_invalid() {
    assert!(ExitCode::from_code(3).is_none());
    assert!(ExitCode::from_code(255).is_none());
}

#[test]
fn test_wake_time_accessors() {
    use chrono::NaiveDateTime;
    use cryochamber::marker::WakeTime;
    let dt = NaiveDateTime::parse_from_str("2026-07-15T14:30", "%Y-%m-%dT%H:%M").unwrap();
    let wt = WakeTime(dt);
    assert_eq!(wt.month(), 7);
    assert_eq!(wt.day(), 15);
    assert_eq!(wt.hour(), 14);
    assert_eq!(wt.minute(), 30);
    assert_eq!(*wt.inner(), dt);
}

#[test]
fn test_parse_invalid_wake_time() {
    let text = "[CRYO:WAKE not-a-date]";
    let markers = parse_markers(text).unwrap();
    assert!(markers.wake_time.is_none());
}

#[test]
fn test_parse_exit_invalid_code() {
    // Code 3 is out of range — parse_markers will error on parse (u8 parse succeeds
    // but from_code returns None), so exit_code is None
    let text = "[CRYO:EXIT 3] invalid code";
    let markers = parse_markers(text).unwrap();
    assert!(markers.exit_code.is_none());
}

#[test]
fn test_parse_reply() {
    let text = r#"[CRYO:EXIT 0] Done
[CRYO:REPLY "Updated the API endpoint as requested."]
[CRYO:WAKE 2026-03-08T09:00]"#;
    let markers = parse_markers(text).unwrap();
    assert_eq!(markers.replies.len(), 1);
    assert_eq!(markers.replies[0], "Updated the API endpoint as requested.");
}

#[test]
fn test_parse_multiple_replies() {
    let text = r#"[CRYO:REPLY "First reply"]
[CRYO:REPLY "Second reply"]
[CRYO:EXIT 0]"#;
    let markers = parse_markers(text).unwrap();
    assert_eq!(markers.replies.len(), 2);
    assert_eq!(markers.replies[0], "First reply");
    assert_eq!(markers.replies[1], "Second reply");
}

#[test]
fn test_parse_no_replies() {
    let text = "[CRYO:EXIT 0] Done";
    let markers = parse_markers(text).unwrap();
    assert!(markers.replies.is_empty());
}
