// tests/agent_tests.rs
use cryochamber::agent::{build_prompt, AgentConfig};

#[test]
fn test_build_prompt_first_session() {
    let config = AgentConfig {
        session_number: 1,
        task: "Start the PR review plan".to_string(),
        delayed_wake: None,
    };
    let prompt = build_prompt(&config);
    assert!(prompt.contains("Session number: 1"));
    assert!(prompt.contains("Start the PR review plan"));
    assert!(prompt.contains("plan.md"));
    assert!(prompt.contains("CLAUDE.md"));
    assert!(prompt.contains("cryo-agent hibernate"));
}

#[test]
fn test_build_prompt_with_history() {
    let config = AgentConfig {
        session_number: 3,
        task: "Follow up on PRs".to_string(),
        delayed_wake: None,
    };
    let prompt = build_prompt(&config);
    assert!(prompt.contains("Session number: 3"));
    assert!(prompt.contains("cryo.log"));
}

#[test]
fn test_build_prompt_contains_cli_reminders() {
    let config = AgentConfig {
        session_number: 1,
        task: "Do the thing".to_string(),
        delayed_wake: None,
    };
    let prompt = build_prompt(&config);
    assert!(prompt.contains("cryo-agent hibernate"));
    assert!(prompt.contains("cryo-agent note"));
    assert!(prompt.contains("plan.md"));
}

#[test]
fn test_build_prompt_references_inbox() {
    let config = AgentConfig {
        session_number: 2,
        task: "Continue".to_string(),
        delayed_wake: None,
    };
    let prompt = build_prompt(&config);
    assert!(prompt.contains("messages/inbox/"));
}

#[test]
fn test_build_prompt_delayed_wake() {
    let config = AgentConfig {
        session_number: 4,
        task: "Check status".to_string(),
        delayed_wake: Some("DELAYED WAKE: 2h late".to_string()),
    };
    let prompt = build_prompt(&config);
    assert!(prompt.contains("DELAYED WAKE: 2h late"));
    assert!(prompt.contains("System Notice"));
}

#[test]
fn test_spawn_agent_fire_and_forget() {
    let mut child = cryochamber::agent::spawn_agent("echo", "hello", None).unwrap();
    let exit = child.wait().unwrap();
    assert!(exit.success());
}

#[test]
fn test_spawn_agent_empty_command() {
    let result = cryochamber::agent::spawn_agent("", "test prompt", None);
    assert!(result.is_err());
    let err = result.err().unwrap().to_string();
    assert!(err.contains("empty"), "Expected 'empty' in error: {err}");
}
