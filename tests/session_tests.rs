// tests/session_tests.rs
// Unit tests for the session module: pure business logic extracted from command handlers.

use chrono::NaiveDateTime;
use cryochamber::fallback::FallbackAction;
use cryochamber::marker::{CryoMarkers, ExitCode, WakeTime};
use cryochamber::session::{self, SessionOutcome};

fn future_time() -> NaiveDateTime {
    NaiveDateTime::parse_from_str("2099-12-31T23:59", "%Y-%m-%dT%H:%M").unwrap()
}

// --- decide_session_outcome ---

#[test]
fn test_decide_plan_complete_exit_success_no_wake() {
    let markers = CryoMarkers {
        exit_code: Some(ExitCode::Success),
        exit_summary: Some("All done".into()),
        ..Default::default()
    };
    let (outcome, warnings) = session::decide_session_outcome(&markers);
    assert!(warnings.is_empty());
    assert!(matches!(outcome, SessionOutcome::PlanComplete));
}

#[test]
fn test_decide_plan_complete_exit_partial_no_wake() {
    let markers = CryoMarkers {
        exit_code: Some(ExitCode::Partial),
        ..Default::default()
    };
    let (outcome, _) = session::decide_session_outcome(&markers);
    assert!(matches!(outcome, SessionOutcome::PlanComplete));
}

#[test]
fn test_decide_hibernate_with_wake() {
    let wake = future_time();
    let markers = CryoMarkers {
        exit_code: Some(ExitCode::Success),
        wake_time: Some(WakeTime(wake)),
        ..Default::default()
    };
    let (outcome, _) = session::decide_session_outcome(&markers);
    match outcome {
        SessionOutcome::Hibernate {
            wake_time,
            fallback,
            command,
        } => {
            assert_eq!(wake_time, wake);
            assert!(fallback.is_none());
            assert!(command.is_none());
        }
        other => panic!("Expected Hibernate, got {other:?}"),
    }
}

#[test]
fn test_decide_hibernate_with_fallback() {
    let wake = future_time();
    let fb = FallbackAction {
        action: "email".into(),
        target: "admin@co.com".into(),
        message: "Agent stuck".into(),
    };
    let markers = CryoMarkers {
        exit_code: Some(ExitCode::Success),
        wake_time: Some(WakeTime(wake)),
        fallbacks: vec![fb.clone()],
        ..Default::default()
    };
    let (outcome, _) = session::decide_session_outcome(&markers);
    match outcome {
        SessionOutcome::Hibernate { fallback, .. } => {
            let fb_out = fallback.unwrap();
            assert_eq!(fb_out.action, "email");
            assert_eq!(fb_out.target, "admin@co.com");
        }
        other => panic!("Expected Hibernate, got {other:?}"),
    }
}

#[test]
fn test_decide_hibernate_with_command() {
    let wake = future_time();
    let markers = CryoMarkers {
        exit_code: Some(ExitCode::Success),
        wake_time: Some(WakeTime(wake)),
        command: Some("echo continue".into()),
        ..Default::default()
    };
    let (outcome, _) = session::decide_session_outcome(&markers);
    match outcome {
        SessionOutcome::Hibernate { command, .. } => {
            assert_eq!(command.as_deref(), Some("echo continue"));
        }
        other => panic!("Expected Hibernate, got {other:?}"),
    }
}

#[test]
fn test_decide_validation_failed_no_exit_marker() {
    let markers = CryoMarkers::default();
    let (outcome, _) = session::decide_session_outcome(&markers);
    match outcome {
        SessionOutcome::ValidationFailed { errors, .. } => {
            assert!(!errors.is_empty());
            assert!(errors.iter().any(|e| e.contains("EXIT")));
        }
        other => panic!("Expected ValidationFailed, got {other:?}"),
    }
}

#[test]
fn test_decide_validation_failed_wake_in_past() {
    let past = NaiveDateTime::parse_from_str("2020-01-01T00:00", "%Y-%m-%dT%H:%M").unwrap();
    let markers = CryoMarkers {
        exit_code: Some(ExitCode::Success),
        wake_time: Some(WakeTime(past)),
        ..Default::default()
    };
    let (outcome, _) = session::decide_session_outcome(&markers);
    match outcome {
        SessionOutcome::ValidationFailed { errors, .. } => {
            assert!(errors.iter().any(|e| e.contains("past")));
        }
        other => panic!("Expected ValidationFailed, got {other:?}"),
    }
}

#[test]
fn test_decide_hibernate_warns_no_command() {
    let wake = future_time();
    let markers = CryoMarkers {
        exit_code: Some(ExitCode::Success),
        wake_time: Some(WakeTime(wake)),
        ..Default::default()
    };
    let (_, warnings) = session::decide_session_outcome(&markers);
    assert!(warnings.iter().any(|w| w.contains("CMD")));
}

// --- format_session_summary ---

#[test]
fn test_format_summary_full_markers() {
    let wake = NaiveDateTime::parse_from_str("2099-06-15T10:00", "%Y-%m-%dT%H:%M").unwrap();
    let markers = CryoMarkers {
        exit_code: Some(ExitCode::Success),
        exit_summary: Some("Task completed".into()),
        plan_note: Some("Move to phase 2".into()),
        wake_time: Some(WakeTime(wake)),
        ..Default::default()
    };
    let summary = session::format_session_summary(3, &markers);
    assert!(summary.contains("Session 3 Summary"));
    assert!(summary.contains("Exit: 0 Task completed"));
    assert!(summary.contains("Plan: Move to phase 2"));
    assert!(summary.contains("Next wake: 2099-06-15T10:00"));
}

#[test]
fn test_format_summary_minimal_markers() {
    let markers = CryoMarkers::default();
    let summary = session::format_session_summary(1, &markers);
    assert!(summary.contains("Exit: ?"));
    assert!(summary.contains("Plan: (none)"));
    assert!(summary.contains("plan complete"));
}

#[test]
fn test_format_summary_plan_complete_no_wake() {
    let markers = CryoMarkers {
        exit_code: Some(ExitCode::Success),
        exit_summary: Some("All done".into()),
        ..Default::default()
    };
    let summary = session::format_session_summary(5, &markers);
    assert!(summary.contains("Next wake: plan complete"));
}

#[test]
fn test_format_summary_partial_exit() {
    let markers = CryoMarkers {
        exit_code: Some(ExitCode::Partial),
        exit_summary: Some("Half done".into()),
        ..Default::default()
    };
    let summary = session::format_session_summary(2, &markers);
    assert!(summary.contains("Exit: 1 Half done"));
}

#[test]
fn test_format_summary_failure_exit() {
    let markers = CryoMarkers {
        exit_code: Some(ExitCode::Failure),
        ..Default::default()
    };
    let summary = session::format_session_summary(1, &markers);
    assert!(summary.contains("Exit: 2"));
}

// --- derive_task_from_output ---

#[test]
fn test_derive_task_with_cmd_marker() {
    let output = "Some output\n[CRYO:EXIT 0] Done\n[CRYO:CMD echo continue]\n";
    let task = session::derive_task_from_output(output);
    assert_eq!(task.as_deref(), Some("echo continue"));
}

#[test]
fn test_derive_task_with_plan_note_only() {
    let output = "[CRYO:EXIT 1] Partial\n[CRYO:PLAN Review the test results]\n";
    let task = session::derive_task_from_output(output);
    assert_eq!(task.as_deref(), Some("Review the test results"));
}

#[test]
fn test_derive_task_cmd_takes_priority_over_plan() {
    let output = "[CRYO:EXIT 0] Done\n[CRYO:CMD run tests]\n[CRYO:PLAN Check coverage report]\n";
    let task = session::derive_task_from_output(output);
    assert_eq!(task.as_deref(), Some("run tests"));
}

#[test]
fn test_derive_task_no_markers() {
    let output = "Just some plain output with no markers\n";
    let task = session::derive_task_from_output(output);
    assert!(task.is_none());
}

#[test]
fn test_derive_task_empty_output() {
    let task = session::derive_task_from_output("");
    assert!(task.is_none());
}

// --- should_copy_plan ---

#[test]
fn test_should_copy_same_file() {
    let dir = tempfile::tempdir().unwrap();
    let plan = dir.path().join("plan.md");
    std::fs::write(&plan, "# Plan").unwrap();

    assert!(!session::should_copy_plan(&plan, &plan));
}

#[test]
fn test_should_copy_different_files() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("source.md");
    let dst = dir.path().join("plan.md");
    std::fs::write(&src, "# Source").unwrap();
    std::fs::write(&dst, "# Old").unwrap();

    assert!(session::should_copy_plan(&src, &dst));
}

#[test]
fn test_should_copy_dest_nonexistent() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("source.md");
    let dst = dir.path().join("plan.md");
    std::fs::write(&src, "# Source").unwrap();

    assert!(session::should_copy_plan(&src, &dst));
}

#[test]
fn test_should_copy_source_nonexistent() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("nonexistent.md");
    let dst = dir.path().join("plan.md");
    std::fs::write(&dst, "# Plan").unwrap();

    assert!(session::should_copy_plan(&src, &dst));
}
