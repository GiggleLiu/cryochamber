// tests/validate_tests.rs
use chrono::{Duration, Local, NaiveDateTime};
use cryochamber::marker::{CryoMarkers, ExitCode, WakeTime};
use cryochamber::validate::{validate_markers, ValidationResult};

#[test]
fn test_valid_markers() {
    let future = Local::now().naive_local() + Duration::hours(24);
    let markers = CryoMarkers {
        exit_code: Some(ExitCode::Success),
        exit_summary: Some("done".to_string()),
        wake_time: Some(WakeTime(future)),
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
        wake_time: Some(WakeTime(past)),
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
