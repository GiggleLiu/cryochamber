// tests/protocol_tests.rs
use cryochamber::protocol;

#[test]
fn test_protocol_content_contains_commands() {
    let content = protocol::PROTOCOL_CONTENT;
    assert!(content.contains("cryo-agent hibernate"));
    assert!(content.contains("cryo-agent note"));
    assert!(content.contains("cryo-agent send"));
    assert!(content.contains("cryo-agent receive"));
    assert!(content.contains("cryo-agent alert"));
    // Phantom commands removed (code review #3)
    assert!(!content.contains("cryo-agent status"));
    assert!(!content.contains("cryo-agent inbox"));
}

#[test]
fn test_protocol_content_contains_rules() {
    let content = protocol::PROTOCOL_CONTENT;
    assert!(content.contains("cryo-agent hibernate"));
    assert!(content.contains("plan.md"));
}

#[test]
fn test_protocol_filename_claude() {
    assert_eq!(protocol::protocol_filename("claude"), "CLAUDE.md");
    assert_eq!(protocol::protocol_filename("claude-code"), "CLAUDE.md");
    assert_eq!(protocol::protocol_filename("Claude"), "CLAUDE.md");
    assert_eq!(
        protocol::protocol_filename("/usr/bin/claude -p test"),
        "CLAUDE.md"
    );
}

#[test]
fn test_protocol_filename_other() {
    assert_eq!(protocol::protocol_filename("opencode"), "AGENTS.md");
    assert_eq!(protocol::protocol_filename("aider"), "AGENTS.md");
    assert_eq!(protocol::protocol_filename(""), "AGENTS.md");
}

#[test]
fn test_protocol_filename_ignores_claude_in_args() {
    // Finding 1: "opencode --model claude-3.7" should NOT resolve to CLAUDE.md
    assert_eq!(
        protocol::protocol_filename("opencode --model claude-3.7"),
        "AGENTS.md"
    );
    assert_eq!(
        protocol::protocol_filename("aider --model claude-3-opus"),
        "AGENTS.md"
    );
}

#[test]
fn test_write_protocol_file() {
    let dir = tempfile::tempdir().unwrap();
    let wrote = protocol::write_protocol_file(dir.path(), "CLAUDE.md").unwrap();
    assert!(wrote);
    let path = dir.path().join("CLAUDE.md");
    assert!(path.exists());
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("cryo-agent hibernate"));
}

#[test]
fn test_write_protocol_file_no_clobber() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("CLAUDE.md");
    std::fs::write(&path, "custom protocol").unwrap();
    let wrote = protocol::write_protocol_file(dir.path(), "CLAUDE.md").unwrap();
    assert!(!wrote);
    let content = std::fs::read_to_string(&path).unwrap();
    assert_eq!(content, "custom protocol");
}

#[test]
fn test_find_protocol_file_none() {
    let dir = tempfile::tempdir().unwrap();
    assert!(protocol::find_protocol_file(dir.path()).is_none());
}

#[test]
fn test_find_protocol_file_claude() {
    let dir = tempfile::tempdir().unwrap();
    protocol::write_protocol_file(dir.path(), "CLAUDE.md").unwrap();
    assert_eq!(protocol::find_protocol_file(dir.path()), Some("CLAUDE.md"));
}

#[test]
fn test_find_protocol_file_agents() {
    let dir = tempfile::tempdir().unwrap();
    protocol::write_protocol_file(dir.path(), "AGENTS.md").unwrap();
    assert_eq!(protocol::find_protocol_file(dir.path()), Some("AGENTS.md"));
}

#[test]
fn test_write_template_plan_creates_new() {
    let dir = tempfile::tempdir().unwrap();
    let wrote = protocol::write_template_plan(dir.path()).unwrap();
    assert!(wrote);
    let path = dir.path().join("plan.md");
    assert!(path.exists());
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("# Hello Cryo"));
}

#[test]
fn test_write_template_plan_skips_existing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("plan.md");
    std::fs::write(&path, "existing plan").unwrap();
    let wrote = protocol::write_template_plan(dir.path()).unwrap();
    assert!(!wrote);
    let content = std::fs::read_to_string(&path).unwrap();
    assert_eq!(content, "existing plan");
}

#[test]
fn test_protocol_mentions_hibernate() {
    let content = cryochamber::protocol::PROTOCOL_CONTENT;
    assert!(content.contains("cryo-agent hibernate"));
    assert!(content.contains("cryo-agent note"));
    // No stale cryo status/inbox references
    assert!(!content.contains("cryo status"));
    assert!(!content.contains("cryo inbox"));
}

#[test]
fn test_protocol_no_old_markers() {
    let content = cryochamber::protocol::PROTOCOL_CONTENT;
    assert!(!content.contains("[CRYO:EXIT"));
    assert!(!content.contains("[CRYO:WAKE"));
}
