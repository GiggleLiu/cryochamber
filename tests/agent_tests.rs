// tests/agent_tests.rs
use cryochamber::agent::{build_prompt, AgentConfig};

#[test]
fn test_build_prompt_first_session() {
    let config = AgentConfig {
        session_number: 1,
        task: "Start the PR review plan".to_string(),
        delayed_wake: None,
        todo_list: "No todos.".to_string(),
        inbox_waiting: false,
    };
    let prompt = build_prompt(&config);
    assert!(prompt.contains("Session number: 1"));
    assert!(prompt.contains("Start the PR review plan"));
    assert!(prompt.contains("plan.md"));
    assert!(prompt.contains("CLAUDE.md"));
}

#[test]
fn test_build_prompt_renders_session_and_todos() {
    let config = AgentConfig {
        session_number: 3,
        task: "Follow up on PRs".to_string(),
        delayed_wake: None,
        todo_list: "1. [#a1b2] Review PR #47".to_string(),
        inbox_waiting: false,
    };
    let prompt = build_prompt(&config);
    assert!(prompt.contains("Session number: 3"));
    assert!(prompt.contains("Follow up on PRs"));
    assert!(prompt.contains("Review PR #47"));
}

#[test]
fn test_build_prompt_omits_standing_orders() {
    let config = AgentConfig {
        session_number: 1,
        task: "Do the thing".to_string(),
        delayed_wake: None,
        todo_list: "No todos.".to_string(),
        inbox_waiting: false,
    };
    let prompt = build_prompt(&config);
    assert!(!prompt.contains("## Reminders"));
    assert!(!prompt.contains("## Context"));
    assert!(!prompt.contains("cryo-agent hibernate"));
    assert!(!prompt.contains("cryo-agent send"));
    assert!(!prompt.contains("NOTES.md"));
}

#[test]
fn test_build_prompt_section_hints_when_complete() {
    let config = AgentConfig {
        session_number: 1,
        task: "Work".to_string(),
        delayed_wake: None,
        todo_list: "1. [#a] short item".to_string(),
        inbox_waiting: false,
    };
    let prompt = build_prompt(&config);
    assert!(prompt.contains("## Current Time (no need to call `cryo-agent time` again)"));
    assert!(prompt.contains("## TODO List (no need to call `cryo-agent todo list` again)"));
    assert!(!prompt.contains("## Inbox"));
    assert!(!prompt.contains("(output of"));
}

#[test]
fn test_build_prompt_todo_hint_flips_on_overflow() {
    let long_list = "1. task with a reasonable description\n".repeat(200); // ~7.6 KB
    let config = AgentConfig {
        session_number: 1,
        task: "Work".to_string(),
        delayed_wake: None,
        todo_list: long_list,
        inbox_waiting: false,
    };
    let prompt = build_prompt(&config);
    assert!(prompt.contains("## TODO List (use `cryo-agent todo list` to get full text)"));
    // Over-cap content is omitted entirely.
    assert!(!prompt.contains("1. task with a reasonable description"));
    assert!(prompt.len() < 1200, "prompt was {} bytes", prompt.len());
}

#[test]
fn test_build_prompt_preserves_short_todo_list() {
    let config = AgentConfig {
        session_number: 1,
        task: "Work".to_string(),
        delayed_wake: None,
        todo_list: "1. [#a] short item".to_string(),
        inbox_waiting: false,
    };
    let prompt = build_prompt(&config);
    assert!(prompt.contains("1. [#a] short item"));
}

#[test]
fn test_fit_section_under_cap_is_complete() {
    let s = cryochamber::agent::fit_section("short", 2048);
    assert!(s.complete);
    assert_eq!(s.content, "short");
}

#[test]
fn test_fit_section_over_cap_is_empty_and_incomplete() {
    let big = "x".repeat(5_000);
    let s = cryochamber::agent::fit_section(&big, 2048);
    assert!(!s.complete);
    assert!(s.content.is_empty());
}

#[test]
fn test_build_prompt_inbox_section_shows_no_messages_when_empty() {
    let config = AgentConfig {
        session_number: 1,
        task: "Work".to_string(),
        delayed_wake: None,
        todo_list: "No todos.".to_string(),
        inbox_waiting: false,
    };
    let prompt = build_prompt(&config);
    assert!(!prompt.contains("## Inbox"));
}

#[test]
fn test_build_prompt_hides_inbox_contents_even_when_waiting() {
    let config = AgentConfig {
        session_number: 1,
        task: "Work".to_string(),
        delayed_wake: None,
        todo_list: "No todos.".to_string(),
        inbox_waiting: true,
    };
    let prompt = build_prompt(&config);
    assert!(prompt.contains("## Inbox"));
    assert!(prompt.contains("Run `cryo-agent receive`"));
    assert!(!prompt.contains("From: alice"));
    assert!(!prompt.contains("Hello"));
}

#[test]
fn test_build_prompt_hides_inbox_when_not_waiting() {
    let config = AgentConfig {
        session_number: 1,
        task: "Work".to_string(),
        delayed_wake: None,
        todo_list: "No todos.".to_string(),
        inbox_waiting: false,
    };
    let prompt = build_prompt(&config);
    assert!(!prompt.contains("## Inbox"));
}

#[test]
fn test_build_prompt_delayed_wake() {
    let config = AgentConfig {
        session_number: 4,
        task: "Check status".to_string(),
        delayed_wake: Some("DELAYED WAKE: 2h late".to_string()),
        todo_list: "No todos.".to_string(),
        inbox_waiting: false,
    };
    let prompt = build_prompt(&config);
    assert!(prompt.contains("DELAYED WAKE: 2h late"));
    assert!(prompt.contains("System Notice"));
}

#[test]
fn test_spawn_agent_fire_and_forget() {
    let mut child =
        cryochamber::agent::spawn_agent("echo", "hello", None, &std::collections::HashMap::new())
            .unwrap();
    let exit = child.wait().unwrap();
    assert!(exit.success());
}

#[test]
fn test_spawn_agent_empty_command() {
    let result =
        cryochamber::agent::spawn_agent("", "test prompt", None, &std::collections::HashMap::new());
    assert!(result.is_err());
    let err = result.err().unwrap().to_string();
    assert!(err.contains("empty"), "Expected 'empty' in error: {err}");
}

#[test]
fn test_spawn_agent_with_env_vars() {
    use std::collections::HashMap;

    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("agent.log");
    let log_file = std::fs::File::create(&log_path).unwrap();

    let mut env = HashMap::new();
    env.insert("TEST_CRYO_KEY".to_string(), "test_value_123".to_string());

    let mut child =
        cryochamber::agent::spawn_agent("printenv", "TEST_CRYO_KEY", Some(log_file), &env).unwrap();
    let status = child.wait().unwrap();
    assert!(status.success());

    let output = std::fs::read_to_string(&log_path).unwrap();
    assert!(
        output.contains("test_value_123"),
        "Expected env var in output: {output}"
    );
}

#[test]
fn test_spawn_agent_with_empty_env_vars() {
    use std::collections::HashMap;
    let env = HashMap::new();

    let child = cryochamber::agent::spawn_agent("echo", "hello", None, &env);
    assert!(child.is_ok());
    let mut child = child.unwrap();
    let _ = child.wait();
}

#[test]
fn test_resolve_mock_agent() {
    let cmd = cryochamber::agent::build_command("mock", "test prompt").unwrap();
    let program = format!("{cmd:?}");
    assert!(
        program.contains("cryo-mock"),
        "mock should resolve to cryo-mock: {program}"
    );
}

#[test]
fn test_mock_agent_program() {
    let program = cryochamber::agent::agent_program("mock").unwrap();
    assert_eq!(program, "cryo-mock");
}

#[test]
fn test_build_command_claude_p_flag_is_idempotent() {
    let cmd = cryochamber::agent::build_command("claude -p", "test prompt").unwrap();
    let args: Vec<String> = cmd
        .get_args()
        .map(|arg| arg.to_string_lossy().to_string())
        .collect();

    assert_eq!(args.iter().filter(|arg| arg.as_str() == "-p").count(), 1);
    assert_eq!(args.last().map(String::as_str), Some("test prompt"));
}
