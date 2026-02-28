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
fn test_build_prompt_references_log() {
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
    // "mock" should resolve to "sh" running "scenario.sh"
    let cmd = cryochamber::agent::build_command("mock", "test prompt").unwrap();
    let program = format!("{:?}", cmd);
    assert!(
        program.contains("sh"),
        "mock should resolve to sh: {program}"
    );
}

#[test]
fn test_mock_agent_program() {
    let program = cryochamber::agent::agent_program("mock").unwrap();
    assert_eq!(program, "sh");
}
