// tests/integration_test.rs
use cryochamber::agent::build_prompt;
use cryochamber::log::{session_count, EventLogger};
use cryochamber::state::{save_state, CryoState};

#[test]
fn test_event_log_full_cycle() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("cryo.log");

    // Session 1
    let mut logger = EventLogger::begin(&log_path, 1, "Start review", "claude -p", &[]).unwrap();
    logger.log_event("agent started (pid 1234)").unwrap();
    logger.log_event("note: \"Reviewed PRs\"").unwrap();
    logger
        .log_event("hibernate: wake=2026-03-08T09:00, exit=0")
        .unwrap();
    logger.finish("agent exited (code 0)").unwrap();

    assert_eq!(session_count(&log_path).unwrap(), 1);

    // Session 2
    let mut logger2 = EventLogger::begin(&log_path, 2, "Follow up", "claude -p", &[]).unwrap();
    logger2.log_event("agent started (pid 5678)").unwrap();
    logger2.log_event("hibernate: complete, exit=0").unwrap();
    logger2.finish("agent exited (code 0)").unwrap();

    assert_eq!(session_count(&log_path).unwrap(), 2);
}

#[test]
fn test_build_prompt_with_context() {
    let config = cryochamber::agent::AgentConfig {
        log_content: Some("Previous session info".to_string()),
        session_number: 3,
        task: "Continue work".to_string(),
        inbox_messages: vec![],
        delayed_wake: None,
    };
    let prompt = build_prompt(&config);
    assert!(prompt.contains("Session number: 3"));
    assert!(prompt.contains("Previous session info"));
}

#[test]
fn test_state_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("timer.json");

    let state = CryoState {
        plan_path: "plan.md".to_string(),
        session_number: 5,
        last_command: Some("opencode".to_string()),
        pid: None,
        max_retries: 3,
        retry_count: 0,
        max_session_duration: 1800,
        watch_inbox: true,
        daemon_mode: false,
    };
    save_state(&state_path, &state).unwrap();

    let loaded = cryochamber::state::load_state(&state_path)
        .unwrap()
        .unwrap();
    assert_eq!(loaded.session_number, 5);
    assert_eq!(loaded.max_retries, 3);
}

#[test]
fn test_file_channel_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    cryochamber::message::ensure_dirs(dir.path()).unwrap();

    let msg = cryochamber::message::Message {
        from: "human".to_string(),
        subject: "Test".to_string(),
        body: "Hello agent".to_string(),
        timestamp: chrono::NaiveDateTime::parse_from_str(
            "2026-02-23T10:00:00",
            "%Y-%m-%dT%H:%M:%S",
        )
        .unwrap(),
        metadata: std::collections::BTreeMap::new(),
    };
    cryochamber::message::write_message(dir.path(), "inbox", &msg).unwrap();

    use cryochamber::channel::MessageChannel;
    let channel = cryochamber::channel::file::FileChannel::new(dir.path().to_path_buf());
    let messages = channel.read_inbox().unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].from, "human");

    channel.post_reply("Got it, thanks!").unwrap();

    let outbox_entries: Vec<_> = std::fs::read_dir(dir.path().join("messages/outbox"))
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    assert_eq!(outbox_entries.len(), 1);
}
