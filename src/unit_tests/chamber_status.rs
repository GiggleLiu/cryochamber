use super::*;

fn test_state(session_number: u32) -> crate::state::CryoState {
    crate::state::CryoState {
        session_number,
        pid: None,
        agent_override: None,
        max_session_duration_override: None,
        last_report_time: None,
        provider_index: None,
        instance_id: None,
        session_active: false,
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
        is_question: false,
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
    let todos = crate::todo::TodoFile::new(dir.path().join("todo.json"));
    todos
        .add("later".to_string(), "2026-05-02T10:00".to_string())
        .unwrap();
    todos
        .add("earlier".to_string(), "2026-05-01T09:00".to_string())
        .unwrap();
    let done_id = todos
        .add("done".to_string(), "2026-04-01T09:00".to_string())
        .unwrap();
    todos.done(done_id).unwrap();

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

#[test]
fn overview_agent_running_requires_live_daemon() {
    let dir = tempfile::tempdir().unwrap();
    let mut st = test_state(1);
    st.pid = Some(std::process::id());
    st.session_active = true;
    crate::state::save_state(&crate::state::state_path(dir.path()), &st).unwrap();
    let ov = overview(dir.path());
    assert!(ov.running, "live daemon should be running");
    assert!(
        ov.agent_running,
        "session_active + live pid should yield agent_running"
    );
}

#[test]
fn overview_agent_running_false_when_daemon_dead() {
    let dir = tempfile::tempdir().unwrap();
    // Spawn a throwaway process and wait for it to exit so its PID is dead.
    let mut child = std::process::Command::new("true").spawn().unwrap();
    let dead_pid = child.id();
    child.wait().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(100));

    let mut st = test_state(1);
    st.pid = Some(dead_pid);
    st.session_active = true; // stale leftover
    crate::state::save_state(&crate::state::state_path(dir.path()), &st).unwrap();
    let ov = overview(dir.path());
    assert!(!ov.running);
    assert!(
        !ov.agent_running,
        "stale session_active must not yield agent_running without a live daemon"
    );
}

#[test]
fn overview_agent_running_false_when_idle() {
    let dir = tempfile::tempdir().unwrap();
    let mut st = test_state(1);
    st.pid = Some(std::process::id());
    st.session_active = false;
    crate::state::save_state(&crate::state::state_path(dir.path()), &st).unwrap();
    let ov = overview(dir.path());
    assert!(ov.running);
    assert!(!ov.agent_running);
}

fn write_outbox_msg(dir: &std::path::Path, from: &str, body: &str, ts: &str, is_question: bool) {
    let store = MessageStore::new(dir.to_path_buf());
    store.ensure_dirs().unwrap();
    let msg = crate::message::Message {
        from: from.to_string(),
        subject: String::new(),
        body: body.to_string(),
        timestamp: chrono::NaiveDateTime::parse_from_str(ts, "%Y-%m-%dT%H:%M:%S").unwrap(),
        metadata: Default::default(),
        is_question,
    };
    store.send_out(&msg).unwrap();
}

fn write_inbox_msg(dir: &std::path::Path, from: &str, body: &str, ts: &str) {
    let store = MessageStore::new(dir.to_path_buf());
    store.ensure_dirs().unwrap();
    let msg = crate::message::Message {
        from: from.to_string(),
        subject: String::new(),
        body: body.to_string(),
        timestamp: chrono::NaiveDateTime::parse_from_str(ts, "%Y-%m-%dT%H:%M:%S").unwrap(),
        metadata: Default::default(),
        is_question: false,
    };
    store.send_in(&msg).unwrap();
}

#[test]
fn has_open_question_false_when_no_messages() {
    let dir = tempfile::tempdir().unwrap();
    assert!(!has_open_question(dir.path()));
}

#[test]
fn has_open_question_false_when_outbox_has_no_question() {
    let dir = tempfile::tempdir().unwrap();
    write_outbox_msg(
        dir.path(),
        "agent",
        "Status update",
        "2026-04-25T10:00:00",
        false,
    );
    assert!(!has_open_question(dir.path()));
}

#[test]
fn has_open_question_true_when_question_exists_and_no_inbox_reply() {
    let dir = tempfile::tempdir().unwrap();
    write_outbox_msg(
        dir.path(),
        "agent",
        "What is ice?",
        "2026-04-25T10:00:00",
        true,
    );
    assert!(has_open_question(dir.path()));
}

#[test]
fn has_open_question_false_when_human_reply_is_newer_than_question() {
    let dir = tempfile::tempdir().unwrap();
    write_outbox_msg(dir.path(), "agent", "Q", "2026-04-25T10:00:00", true);
    write_inbox_msg(dir.path(), "human", "answer", "2026-04-25T11:00:00");
    assert!(!has_open_question(dir.path()));
}

#[test]
fn has_open_question_true_when_question_is_newer_than_last_reply() {
    let dir = tempfile::tempdir().unwrap();
    write_inbox_msg(dir.path(), "human", "old reply", "2026-04-25T09:00:00");
    write_outbox_msg(
        dir.path(),
        "agent",
        "fresh question",
        "2026-04-25T10:00:00",
        true,
    );
    assert!(has_open_question(dir.path()));
}

#[test]
fn has_open_question_ignores_operator_inbox_messages() {
    let dir = tempfile::tempdir().unwrap();
    write_outbox_msg(dir.path(), "agent", "Q", "2026-04-25T10:00:00", true);
    // Operator wake message arrives later but should NOT clear the indicator.
    write_inbox_msg(dir.path(), "operator", "wake", "2026-04-25T11:00:00");
    assert!(has_open_question(dir.path()));
}

#[test]
fn has_open_question_ignores_cryochamber_inbox_messages() {
    let dir = tempfile::tempdir().unwrap();
    write_outbox_msg(dir.path(), "agent", "Q", "2026-04-25T10:00:00", true);
    write_inbox_msg(
        dir.path(),
        "cryochamber",
        "system note",
        "2026-04-25T11:00:00",
    );
    assert!(has_open_question(dir.path()));
}

#[test]
fn has_open_question_uses_archived_inbox_replies() {
    let dir = tempfile::tempdir().unwrap();
    // Reply was processed and archived (the normal case after the agent reads it).
    write_inbox_msg(dir.path(), "human", "answer", "2026-04-25T11:00:00");
    let store = MessageStore::new(dir.path().to_path_buf());
    let filenames: Vec<String> = store.list_inbox_filenames().unwrap().into_iter().collect();
    store.archive_inbox(&filenames).unwrap();
    write_outbox_msg(dir.path(), "agent", "Q", "2026-04-25T10:00:00", true);
    // Question is older than the archived reply, so no open question.
    assert!(!has_open_question(dir.path()));
}

#[test]
fn overview_exposes_has_open_question() {
    let dir = tempfile::tempdir().unwrap();
    write_outbox_msg(dir.path(), "agent", "Q", "2026-04-25T10:00:00", true);
    let ov = overview(dir.path());
    assert!(ov.has_open_question);
}
