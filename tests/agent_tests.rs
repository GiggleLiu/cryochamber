// tests/agent_tests.rs
use cryochamber::agent::{build_prompt, AgentConfig};

#[test]
fn test_build_prompt_first_session() {
    let config = AgentConfig {
        log_content: None,
        session_number: 1,
        task: "Start the PR review plan".to_string(),
        inbox_messages: vec![],
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
        log_content: Some(
            "note: \"check PR #41\"\nhibernate: wake=2026-03-09T09:00, exit=0".to_string(),
        ),
        session_number: 3,
        task: "Follow up on PRs".to_string(),
        inbox_messages: vec![],
    };
    let prompt = build_prompt(&config);
    assert!(prompt.contains("Session number: 3"));
    assert!(prompt.contains("check PR #41"));
}

#[test]
fn test_build_prompt_contains_cli_reminders() {
    let config = AgentConfig {
        log_content: None,
        session_number: 1,
        task: "Do the thing".to_string(),
        inbox_messages: vec![],
    };
    let prompt = build_prompt(&config);
    assert!(prompt.contains("cryo-agent hibernate"));
    assert!(prompt.contains("cryo-agent note"));
    assert!(prompt.contains("plan.md"));
}

#[test]
fn test_build_prompt_with_inbox_messages() {
    use chrono::NaiveDateTime;
    use cryochamber::message::Message;
    use std::collections::BTreeMap;

    let msg = Message {
        from: "human".to_string(),
        subject: "CI failing".to_string(),
        body: "The lint step is broken.".to_string(),
        timestamp: NaiveDateTime::parse_from_str("2026-02-23T10:30:00", "%Y-%m-%dT%H:%M:%S")
            .unwrap(),
        metadata: BTreeMap::new(),
    };
    let config = AgentConfig {
        log_content: None,
        session_number: 2,
        task: "Continue".to_string(),
        inbox_messages: vec![msg],
    };
    let prompt = build_prompt(&config);
    assert!(prompt.contains("New Messages (1 unread)"));
    assert!(prompt.contains("From: human"));
    assert!(prompt.contains("CI failing"));
    assert!(prompt.contains("The lint step is broken."));
}

#[test]
fn test_build_prompt_no_messages_section_when_empty() {
    let config = AgentConfig {
        log_content: None,
        session_number: 1,
        task: "Do stuff".to_string(),
        inbox_messages: vec![],
    };
    let prompt = build_prompt(&config);
    assert!(!prompt.contains("New Messages"));
}

#[test]
fn test_spawn_agent_fire_and_forget() {
    let mut child = cryochamber::agent::spawn_agent("echo", "hello").unwrap();
    let exit = child.wait().unwrap();
    assert!(exit.success());
}

#[test]
fn test_spawn_agent_empty_command() {
    let result = cryochamber::agent::spawn_agent("", "test prompt");
    assert!(result.is_err());
    let err = result.err().unwrap().to_string();
    assert!(err.contains("empty"), "Expected 'empty' in error: {err}");
}
