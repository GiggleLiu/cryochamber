// tests/integration_test.rs
use cryochamber::agent::build_prompt;
use cryochamber::log::{append_session, read_latest_session, session_count, Session};
use cryochamber::marker::parse_markers;
use cryochamber::state::{save_state, CryoState};
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
    let agent_output =
        "Something went wrong, couldn't connect.\n\n[CRYO:EXIT 2] Failed to connect to GitHub API";
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
