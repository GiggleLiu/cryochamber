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
