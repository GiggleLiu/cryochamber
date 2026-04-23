use super::*;

fn test_state(session_number: u32) -> crate::state::CryoState {
    crate::state::CryoState {
        session_number,
        pid: None,
        retry_count: 0,
        agent_override: None,
        max_session_duration_override: None,
        last_report_time: None,
        provider_index: None,
        instance_id: None,
        previous_session_crashed: false,
    }
}

fn test_message(from: &str, body: &str, timestamp: &str) -> crate::message::Message {
    crate::message::Message {
        from: from.to_string(),
        subject: String::new(),
        body: body.to_string(),
        timestamp: chrono::NaiveDateTime::parse_from_str(timestamp, "%Y-%m-%dT%H:%M:%S").unwrap(),
        metadata: Default::default(),
    }
}

#[test]
fn status_missing_timer_json_returns_stopped_defaults() {
    let dir = tempfile::tempdir().unwrap();

    let status = status(dir.path());

    assert!(!status.running);
    assert_eq!(status.session, 0);
    assert_eq!(status.agent, "opencode");
}

#[test]
fn status_uses_state_agent_override_over_config_agent() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = crate::config::CryoConfig {
        agent: "claude".to_string(),
        ..Default::default()
    };
    crate::config::save_config(&crate::config::config_path(dir.path()), &cfg).unwrap();
    let mut st = test_state(4);
    st.agent_override = Some("codex".to_string());
    crate::state::save_state(&crate::state::state_path(dir.path()), &st).unwrap();

    let status = status(dir.path());

    assert_eq!(status.session, 4);
    assert_eq!(status.agent, "codex");
}

#[test]
fn status_next_wake_uses_earliest_open_todo() {
    let dir = tempfile::tempdir().unwrap();
    let mut todos = crate::todo::TodoList::new();
    todos.add("later".to_string(), "2026-05-02T10:00".to_string());
    todos.add("earlier".to_string(), "2026-05-01T09:00".to_string());
    let done_id = todos.add("done".to_string(), "2026-04-01T09:00".to_string());
    todos.done(done_id).unwrap();
    todos.save(&dir.path().join("todo.json")).unwrap();

    let status = status(dir.path());

    assert_eq!(status.next_wake, Some("2026-05-01T09:00".to_string()));
}

#[test]
fn status_completion_summary_comes_from_latest_session_log() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        crate::log::log_path(dir.path()),
        "--- CRYO SESSION 1 | 2026-04-20T10:00:00Z ---\n\
         [10:00:01] hibernate: plan complete, exit=0, summary=\"old summary\"\n\
         --- CRYO END ---\n\
         --- CRYO SESSION 2 | 2026-04-20T12:00:00Z ---\n\
         task: ship the patch\n\
         [12:00:01] hibernate: plan complete, exit=0, summary=\"new summary\"\n\
         --- CRYO END ---\n",
    )
    .unwrap();

    let status = status(dir.path());

    assert!(status.completed);
    assert_eq!(status.task, Some("ship the patch".to_string()));
    assert_eq!(status.completion_summary, Some("new summary".to_string()));
}

#[test]
fn messages_are_sorted_chronologically_and_tagged_with_sessions() {
    let dir = tempfile::tempdir().unwrap();
    crate::message::ensure_dirs(dir.path()).unwrap();
    std::fs::write(
        crate::log::log_path(dir.path()),
        "--- CRYO SESSION 1 | 2026-04-20T10:00:00Z ---\n\
         --- CRYO END ---\n\
         --- CRYO SESSION 2 | 2026-04-20T12:00:00Z ---\n\
         --- CRYO END ---\n",
    )
    .unwrap();
    crate::message::write_message(
        dir.path(),
        "inbox",
        &test_message("operator", "late", "2026-04-20T13:00:00"),
    )
    .unwrap();
    crate::message::write_message(
        dir.path(),
        "outbox",
        &test_message("agent", "early", "2026-04-20T10:30:00"),
    )
    .unwrap();
    crate::message::write_message(
        dir.path(),
        "inbox",
        &test_message("operator", "pre", "2026-04-20T09:00:00"),
    )
    .unwrap();

    let messages = messages(dir.path());

    assert_eq!(
        messages
            .iter()
            .map(|msg| msg.body.as_str())
            .collect::<Vec<_>>(),
        vec!["pre", "early", "late"]
    );
    assert_eq!(messages[0].session, None);
    assert_eq!(messages[1].session, Some(1));
    assert_eq!(messages[2].session, Some(2));
}
