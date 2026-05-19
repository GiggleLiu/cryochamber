use super::*;

fn test_state(session_number: u32) -> crate::state::CryoState {
    crate::state::CryoState {
        session_number,
        pid: None,
        agent_override: None,
        max_session_duration_override: None,
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
fn status_includes_daily_digests_from_log() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        crate::log::log_path(dir.path()),
        "--- CRYO SESSION 1 | 2026-03-01T09:00:00Z ---\n\
         [09:00:01] hibernate: wake=2026-03-01T14:00, exit=0\n\
         --- CRYO END ---\n\
         --- CRYO SESSION 2 | 2026-03-01T12:00:00Z ---\n\
         [12:00:01] agent exited without hibernate\n\
         --- CRYO END ---\n",
    )
    .unwrap();

    let status = status(dir.path());

    assert_eq!(status.daily_digests.len(), 1);
    assert_eq!(status.daily_digests[0].date, "2026-03-01");
    assert_eq!(status.daily_digests[0].total_sessions, 2);
    assert_eq!(status.daily_digests[0].failed_sessions, 1);
    assert_eq!(status.daily_digests[0].latest_session, 2);
}

#[test]
fn status_reads_plan_and_config_from_disk() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("plan.md"), "## Plan\n1. wake\n2. work\n").unwrap();
    std::fs::write(
        dir.path().join("cryo.toml"),
        "agent = \"claude\"\nwatch_dirs = [\"messages/inbox\"]\n",
    )
    .unwrap();

    let status = status(dir.path());

    assert_eq!(status.plan_content, "## Plan\n1. wake\n2. work\n");
    assert_eq!(
        status.config_content,
        "agent = \"claude\"\nwatch_dirs = [\"messages/inbox\"]\n"
    );
    assert!(status.plan_html.contains("<h2>Plan</h2>"));
    assert!(status.plan_html.contains("<li>wake</li>"));
}

#[test]
fn status_renders_notes_markdown_to_html() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("NOTES.md"), "# Notes\n\n- one\n- two\n").unwrap();

    let status = status(dir.path());

    assert_eq!(status.notes_content, "# Notes\n\n- one\n- two\n");
    assert!(status.notes_html.contains("<h1>Notes</h1>"));
    assert!(status.notes_html.contains("<li>one</li>"));
}

#[test]
fn status_plan_and_config_empty_when_files_missing() {
    let dir = tempfile::tempdir().unwrap();

    let status = status(dir.path());

    assert!(status.plan_content.is_empty());
    assert!(status.plan_html.is_empty());
    assert!(status.notes_content.is_empty());
    assert!(status.notes_html.is_empty());
    assert!(status.config_content.is_empty());
}

#[test]
fn render_markdown_safe_escapes_raw_html() {
    let out = render_markdown_safe("Hello <script>alert(1)</script> world");
    assert!(
        !out.contains("<script>"),
        "raw script tag must be escaped, got: {out}"
    );
    assert!(
        out.contains("&lt;script&gt;"),
        "raw HTML should be escaped to text, got: {out}"
    );
}

#[test]
fn render_markdown_safe_drops_image_urls() {
    // Images become plain emphasis so a malicious plan can't make the
    // operator's browser fetch arbitrary URLs.
    let out = render_markdown_safe("see ![alt](http://attacker.example/pixel.gif)");
    assert!(!out.contains("<img"), "img tag must not appear, got: {out}");
    assert!(
        !out.contains("attacker.example"),
        "image src must not leak into output, got: {out}"
    );
}

#[test]
fn parse_settings_rows_handles_scalars_and_provider() {
    // Top-level scalars become individual rows. The `provider` table redacts
    // env *values* (which can hold API keys) but lists env *keys* so the
    // operator can verify what the provider sets.
    let toml = r#"
agent = "claude"
max_session_duration = 600
watch_dirs = ["messages/inbox"]

[provider]
name = "anthropic"
env = { ANTHROPIC_API_KEY = "sk-secret-1", ANTHROPIC_MODEL = "claude-sonnet-4-6" }
"#;
    let rows = parse_settings_rows(toml);
    let by_key: std::collections::HashMap<&str, &str> = rows
        .iter()
        .map(|r| (r.key.as_str(), r.value.as_str()))
        .collect();

    assert_eq!(by_key.get("agent").copied(), Some("\"claude\""));
    assert_eq!(by_key.get("max_session_duration").copied(), Some("600"));
    assert_eq!(
        by_key.get("watch_dirs").copied(),
        Some("[\"messages/inbox\"]")
    );

    let p0 = by_key.get("provider").expect("provider");
    assert!(p0.starts_with("anthropic"));
    assert!(p0.contains("ANTHROPIC_API_KEY"));
    assert!(p0.contains("ANTHROPIC_MODEL"));
    assert!(
        !p0.contains("sk-secret"),
        "env values must never leak into the settings rows; got {p0}"
    );
}

#[test]
fn parse_settings_rows_expands_scalar_arrays_inline() {
    // Arrays of plain scalars (e.g. `watch_dirs`) should show their items
    // inline so the operator can see what's actually configured instead of
    // a "[N items]" placeholder.
    let toml = r#"
watch_dirs = ["messages/inbox", "drop_box"]
"#;
    let rows = parse_settings_rows(toml);
    let by_key: std::collections::HashMap<&str, &str> = rows
        .iter()
        .map(|r| (r.key.as_str(), r.value.as_str()))
        .collect();
    assert_eq!(
        by_key.get("watch_dirs").copied(),
        Some("[\"messages/inbox\", \"drop_box\"]")
    );
}

#[test]
fn parse_settings_rows_handles_legacy_providers_array() {
    let toml = r#"
[[providers]]
name = "openai"
env = { OPENAI_API_KEY = "sk-secret-2" }
"#;
    let rows = parse_settings_rows(toml);
    let by_key: std::collections::HashMap<&str, &str> = rows
        .iter()
        .map(|r| (r.key.as_str(), r.value.as_str()))
        .collect();

    let p1 = by_key.get("providers[0]").expect("providers[0]");
    assert!(p1.starts_with("openai"));
    assert!(p1.contains("OPENAI_API_KEY"));
    assert!(!p1.contains("sk-secret-2"), "got {p1}");
}

#[test]
fn parse_settings_rows_returns_empty_for_invalid_toml() {
    let rows = parse_settings_rows("this is not = valid toml [[[");
    assert!(rows.is_empty());
}

#[test]
fn parse_settings_rows_returns_empty_for_empty_input() {
    assert!(parse_settings_rows("").is_empty());
}

#[test]
fn render_markdown_safe_renders_basic_markdown() {
    let out = render_markdown_safe("# Title\n\n- one\n- two\n");
    assert!(out.contains("<h1>Title</h1>"));
    assert!(out.contains("<li>one</li>"));
    assert!(out.contains("<li>two</li>"));
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
fn has_open_question_keeps_same_second_prior_reply_open() {
    let dir = tempfile::tempdir().unwrap();
    write_inbox_msg(dir.path(), "human", "trigger", "2026-04-25T10:00:00");
    write_outbox_msg(dir.path(), "agent", "Q", "2026-04-25T10:00:00", true);
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
