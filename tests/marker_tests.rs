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
