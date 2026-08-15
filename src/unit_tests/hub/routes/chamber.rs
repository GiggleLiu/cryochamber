use super::*;

#[test]
fn status_json_for_missing_state_has_zero_session() {
    let dir = tempfile::tempdir().unwrap();
    let v = status_json(dir.path());
    assert_eq!(v["running"], false);
    assert_eq!(v["session"], 0);
}

#[tokio::test]
async fn post_start_refuses_archived_chamber_without_launching() {
    let workspace = tempfile::tempdir().unwrap();
    let chamber = workspace.path().join("alpha");
    std::fs::create_dir_all(&chamber).unwrap();
    let cfg = crate::config::CryoConfig::default();
    crate::config::save_config(&chamber.join("cryo.toml"), &cfg).unwrap();

    let app = Arc::new(AppState::local_only(workspace.path().to_path_buf()));
    app.refresh();
    let id = {
        let mut idx = app.chambers.write().unwrap();
        let (id, entry) = idx.iter_mut().next().unwrap();
        entry.archived = true;
        id.clone()
    };

    let Json(v) = post_start(State(app), AxumPath(id)).await.unwrap();

    assert_eq!(v["ok"], false);
    assert!(v["message"].as_str().unwrap_or("").contains("Unarchive"));
}

#[tokio::test]
async fn post_archive_sets_registry_flag_for_stopped_chamber() {
    let state_home = tempfile::tempdir().unwrap();
    let _guard = crate::test_support::EnvVarGuard::set_path("XDG_STATE_HOME", state_home.path());
    let workspace = tempfile::tempdir().unwrap();
    let chamber = workspace.path().join("alpha");
    std::fs::create_dir_all(&chamber).unwrap();
    let cfg = crate::config::CryoConfig::default();
    crate::config::save_config(&chamber.join("cryo.toml"), &cfg).unwrap();

    let app = Arc::new(AppState::local_only(workspace.path().to_path_buf()));
    app.refresh();
    let id = app.chambers.read().unwrap().keys().next().unwrap().clone();

    let Json(v) = post_archive(State(app), AxumPath(id)).await.unwrap();

    assert_eq!(v["ok"], true);
    assert!(crate::registry::is_archived(&chamber));
}

#[tokio::test]
async fn post_unarchive_clears_registry_flag() {
    let state_home = tempfile::tempdir().unwrap();
    let _guard = crate::test_support::EnvVarGuard::set_path("XDG_STATE_HOME", state_home.path());
    let workspace = tempfile::tempdir().unwrap();
    let chamber = workspace.path().join("alpha");
    std::fs::create_dir_all(&chamber).unwrap();
    let cfg = crate::config::CryoConfig::default();
    crate::config::save_config(&chamber.join("cryo.toml"), &cfg).unwrap();
    crate::registry::set_archived(&chamber, true).unwrap();

    let app = Arc::new(AppState::local_only(workspace.path().to_path_buf()));
    app.refresh();
    let id = app.chambers.read().unwrap().keys().next().unwrap().clone();

    let Json(v) = post_unarchive(State(app), AxumPath(id)).await.unwrap();

    assert_eq!(v["ok"], true);
    assert!(!crate::registry::is_archived(&chamber));
}

#[test]
fn status_json_includes_notes_content() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("NOTES.md"), "# hello\n- one\n- two\n").unwrap();
    let v = status_json(dir.path());
    assert_eq!(v["notes_content"], "# hello\n- one\n- two\n");
}

#[test]
fn status_json_includes_agent_running_when_session_is_active() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("timer.json"),
        format!(
            r#"{{
                "session_number": 3,
                "pid": {},
                "session_active": true
            }}"#,
            std::process::id()
        ),
    )
    .unwrap();

    let v = status_json(dir.path());
    assert_eq!(v["running"], true);
    assert_eq!(v["agent_running"], true);
}

#[test]
fn status_json_log_tail_spans_last_five_sessions() {
    // The log panel should default to the last 5 sessions, not just the
    // current one, so the operator can scan recent wake/retry history.
    let dir = tempfile::tempdir().unwrap();
    let mut content = String::new();
    for i in 1..=7 {
        content.push_str(&format!(
            "--- CRYO SESSION {i} | 2026-03-01T{:02}:00:00Z ---\n\
             [xx:xx:xx] marker s{i}\n\
             --- CRYO END ---\n",
            9 + i
        ));
    }
    std::fs::write(crate::log::log_path(dir.path()), content).unwrap();
    let v = status_json(dir.path());
    let tail = v["log_tail"].as_str().unwrap_or("");
    for i in 3..=7 {
        assert!(
            tail.contains(&format!("marker s{i}")),
            "session {i} should be visible in log_tail"
        );
    }
    for i in 1..=2 {
        assert!(
            !tail.contains(&format!("marker s{i}")),
            "session {i} should be outside the last-5 window"
        );
    }
}

#[test]
fn status_json_includes_daily_digests() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        crate::log::log_path(dir.path()),
        "--- CRYO SESSION 1 | 2026-03-01T09:00:00Z ---\n\
         [09:00:01] hibernate: wake=2026-03-01T14:00, exit=0\n\
         --- CRYO END ---\n\
         --- CRYO SESSION 2 | 2026-03-02T09:00:00Z ---\n\
         [09:00:01] agent exited without hibernate\n\
         --- CRYO END ---\n",
    )
    .unwrap();

    let v = status_json(dir.path());

    assert_eq!(v["daily_digests"][0]["date"], "2026-03-02");
    assert_eq!(v["daily_digests"][0]["total_sessions"], 1);
    assert_eq!(v["daily_digests"][0]["failed_sessions"], 1);
    assert_eq!(v["daily_digests"][0]["latest_session"], 2);
    assert_eq!(v["daily_digests"][1]["date"], "2026-03-01");
}

#[test]
fn status_json_includes_latest_session_summary() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        crate::log::log_path(dir.path()),
        "--- CRYO SESSION 1 | 2026-03-01T12:00:00Z ---\n\
         [12:05:00] hibernate: wake=2026-03-01T14:00, exit=0, summary=\"Checked disk usage and scheduled the next warning check\"\n\
         --- CRYO END ---\n",
    )
    .unwrap();
    let v = status_json(dir.path());
    assert_eq!(
        v["session_summary"],
        "Checked disk usage and scheduled the next warning check"
    );
}

#[test]
fn status_json_exposes_raw_next_wake_for_browser_formatting() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("todo.json"),
        r#"[{"id":1,"text":"check disk","done":false,"claimed":false,"at":"2099-05-01T10:00","created":"unknown"}]"#,
    )
    .unwrap();
    let v = status_json(dir.path());
    assert_eq!(v["next_wake"], "2099-05-01T10:00");
    assert!(
        !v["next_wake"].as_str().unwrap_or("").contains('('),
        "status API should not pre-format relative wake text"
    );
}

#[test]
fn status_json_notes_content_empty_when_file_missing() {
    let dir = tempfile::tempdir().unwrap();
    let v = status_json(dir.path());
    assert_eq!(v["notes_content"], "");
}

#[test]
fn status_json_includes_plan_and_masked_config() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("plan.md"), "# Plan\n- step one\n").unwrap();
    std::fs::write(
        dir.path().join("cryo.toml"),
        "agent = \"opencode\"\nmax_session_duration = 600\n",
    )
    .unwrap();
    let v = status_json(dir.path());
    assert_eq!(v["plan_content"], "# Plan\n- step one\n");
    // Raw cryo.toml is never shipped to the browser (it can hold an API key).
    // Only a present/absent bool and the masked rows are exposed.
    assert!(
        v.get("config_content").is_none(),
        "raw config must not be in the status payload"
    );
    assert_eq!(v["has_config"], true);
    assert!(v["settings_rows"].is_array());
    let plan_html = v["plan_html"].as_str().expect("plan_html");
    assert!(plan_html.contains("<h1>Plan</h1>"), "got {plan_html}");
    assert!(plan_html.contains("<li>step one</li>"), "got {plan_html}");
}

#[test]
fn status_json_never_leaks_provider_api_key() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("cryo.toml"),
        "agent = \"opencode\"\n\n[provider]\nname = \"openai\"\n\n\
         [provider.env]\nOPENAI_API_KEY = \"sk-secret-should-not-leak\"\n",
    )
    .unwrap();
    let v = status_json(dir.path());
    let serialized = v.to_string();
    assert!(
        !serialized.contains("sk-secret-should-not-leak"),
        "status payload leaked the API key: {serialized}"
    );
    assert!(v.get("config_content").is_none());
    assert_eq!(v["has_config"], true);
    // The env *key name* is still surfaced (masked), just never its value.
    let rows = v["settings_rows"].as_array().expect("settings_rows");
    let joined = rows.iter().map(|r| r.to_string()).collect::<String>();
    assert!(
        joined.contains("OPENAI_API_KEY"),
        "env key name should show: {joined}"
    );
    assert!(!joined.contains("sk-secret-should-not-leak"));
}

#[test]
fn status_json_plan_and_config_empty_when_files_missing() {
    let dir = tempfile::tempdir().unwrap();
    let v = status_json(dir.path());
    assert_eq!(v["plan_content"], "");
    assert_eq!(v["plan_html"], "");
    assert_eq!(v["notes_content"], "");
    assert_eq!(v["notes_html"], "");
    assert_eq!(v["has_config"], false);
    assert!(v.get("config_content").is_none());
}

#[test]
fn status_json_renders_notes_markdown() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("NOTES.md"), "## Notes\n\n- alpha\n- beta\n").unwrap();
    let v = status_json(dir.path());
    assert_eq!(v["notes_content"], "## Notes\n\n- alpha\n- beta\n");
    let html = v["notes_html"].as_str().expect("notes_html");
    assert!(html.contains("<h2>Notes</h2>"), "got {html}");
    assert!(html.contains("<li>alpha</li>"), "got {html}");
}

#[test]
fn todos_json_is_empty_array_when_missing() {
    let dir = tempfile::tempdir().unwrap();
    let v = todos_json(dir.path());
    assert_eq!(v, serde_json::json!([]));
}

#[test]
fn todos_json_returns_items_in_file_order() {
    let dir = tempfile::tempdir().unwrap();
    let todos = crate::todo::TodoFile::new(dir.path().join("todo.json"));
    todos
        .add("first".into(), "2026-05-01T10:00".into())
        .unwrap();
    let id2 = todos
        .add("second".into(), "2026-04-01T10:00".into())
        .unwrap();
    todos.done(id2).unwrap();

    let v = todos_json(dir.path());
    let arr = v.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0]["text"], "first");
    assert_eq!(arr[0]["done"], false);
    assert_eq!(arr[0]["at"], "2026-05-01T10:00");
    assert_eq!(arr[1]["text"], "second");
    assert_eq!(arr[1]["done"], true);
}

#[test]
fn messages_json_sorted_by_timestamp() {
    let dir = tempfile::tempdir().unwrap();
    crate::message::ensure_dirs(dir.path()).unwrap();
    let early = crate::message::Message {
        from: "a".into(),
        subject: "".into(),
        body: "first".into(),
        timestamp: chrono::NaiveDate::from_ymd_opt(2026, 1, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap(),
        metadata: Default::default(),
        is_question: false,
    };
    let late = crate::message::Message {
        from: "b".into(),
        subject: "".into(),
        body: "second".into(),
        timestamp: chrono::NaiveDate::from_ymd_opt(2026, 1, 2)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap(),
        metadata: Default::default(),
        is_question: false,
    };
    crate::message::write_message(dir.path(), "inbox", &late).unwrap();
    crate::message::write_message(dir.path(), "outbox", &early).unwrap();
    let arr = messages_json(dir.path());
    let arr = arr.as_array().unwrap();
    assert_eq!(arr[0]["body"], "first");
    assert_eq!(arr[1]["body"], "second");
}

#[test]
fn messages_json_tags_each_message_with_the_session_that_owns_it() {
    // Each message JSON needs a `session` field so the hub UI can emit a
    // session divider when the number changes. Sessions are parsed out
    // of `cryo.log` headers; a message's session is the latest header
    // whose timestamp does not exceed the message's own timestamp.
    let dir = tempfile::tempdir().unwrap();
    crate::message::ensure_dirs(dir.path()).unwrap();
    let log = dir.path().join("cryo.log");
    std::fs::write(
        &log,
        "--- CRYO SESSION 1 | 2026-04-20T10:00:00Z ---\n\
         hibernate: nap\n\
         --- CRYO END ---\n\
         --- CRYO SESSION 2 | 2026-04-20T12:00:00Z ---\n\
         hibernate: nap\n\
         --- CRYO END ---\n",
    )
    .unwrap();
    // Before any session.
    let pre = crate::message::Message {
        from: "op".into(),
        subject: "".into(),
        body: "pre".into(),
        timestamp: chrono::NaiveDate::from_ymd_opt(2026, 4, 20)
            .unwrap()
            .and_hms_opt(9, 0, 0)
            .unwrap(),
        metadata: Default::default(),
        is_question: false,
    };
    // Inside session 1.
    let s1 = crate::message::Message {
        from: "op".into(),
        subject: "".into(),
        body: "s1".into(),
        timestamp: chrono::NaiveDate::from_ymd_opt(2026, 4, 20)
            .unwrap()
            .and_hms_opt(10, 30, 0)
            .unwrap(),
        metadata: Default::default(),
        is_question: false,
    };
    // Inside session 2.
    let s2 = crate::message::Message {
        from: "op".into(),
        subject: "".into(),
        body: "s2".into(),
        timestamp: chrono::NaiveDate::from_ymd_opt(2026, 4, 20)
            .unwrap()
            .and_hms_opt(13, 0, 0)
            .unwrap(),
        metadata: Default::default(),
        is_question: false,
    };
    crate::message::write_message(dir.path(), "inbox", &pre).unwrap();
    crate::message::write_message(dir.path(), "inbox", &s1).unwrap();
    crate::message::write_message(dir.path(), "inbox", &s2).unwrap();

    let arr = messages_json(dir.path());
    let arr = arr.as_array().unwrap();
    assert_eq!(arr.len(), 3);
    let find = |body: &str| arr.iter().find(|m| m["body"] == body).unwrap();
    assert!(find("pre")["session"].is_null(), "pre-session → null");
    assert_eq!(find("s1")["session"], 1);
    assert_eq!(find("s2")["session"], 2);
}

#[test]
fn messages_json_includes_outbox_archive() {
    let dir = tempfile::tempdir().unwrap();
    crate::message::ensure_dirs(dir.path()).unwrap();
    let msg = crate::message::Message {
        from: "agent".into(),
        subject: "".into(),
        body: "archived outbox body".into(),
        timestamp: chrono::NaiveDate::from_ymd_opt(2026, 1, 1)
            .unwrap()
            .and_hms_opt(9, 0, 0)
            .unwrap(),
        metadata: Default::default(),
        is_question: false,
    };
    let path = crate::message::write_message(dir.path(), "outbox", &msg).unwrap();
    // Simulate sync daemon archiving the delivered outbox message.
    let archive = dir.path().join("messages").join("outbox").join("archive");
    std::fs::create_dir_all(&archive).unwrap();
    std::fs::rename(&path, archive.join(path.file_name().unwrap())).unwrap();

    let arr = messages_json(dir.path());
    let arr = arr.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["body"], "archived outbox body");
    assert_eq!(arr[0]["direction"], "outbox");
    // The id names the top-level mailbox only: archiving must not renumber a
    // message the client has already seen over SSE.
    let id = arr[0]["id"].as_str().unwrap();
    assert!(id.starts_with("outbox/"), "id was {id}");
    assert!(!id.contains("/archive/"), "id was {id}");
}

#[test]
fn messages_json_includes_unique_stable_ids_for_duplicate_messages() {
    let dir = tempfile::tempdir().unwrap();
    crate::message::ensure_dirs(dir.path()).unwrap();
    let timestamp = chrono::NaiveDate::from_ymd_opt(2026, 1, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let msg = crate::message::Message {
        from: "human".into(),
        subject: "".into(),
        body: "same body".into(),
        timestamp,
        metadata: Default::default(),
        is_question: false,
    };
    crate::message::write_message(dir.path(), "inbox", &msg).unwrap();
    crate::message::write_message(dir.path(), "inbox", &msg).unwrap();

    let arr = messages_json(dir.path());
    let arr = arr.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    let first = arr[0]["id"].as_str().expect("message id");
    let second = arr[1]["id"].as_str().expect("message id");
    assert!(!first.is_empty());
    assert!(!second.is_empty());
    assert_ne!(
        first, second,
        "duplicate timestamp/body messages need distinct ids"
    );
}

/// Workspace with one discoverable chamber. Returns `(tempdir, app, id, dir)`.
fn chamber_app(name: &str) -> (tempfile::TempDir, Arc<AppState>, String, std::path::PathBuf) {
    let workspace = tempfile::tempdir().unwrap();
    let chamber = workspace.path().join(name);
    std::fs::create_dir_all(&chamber).unwrap();
    let cfg = crate::config::CryoConfig::default();
    crate::config::save_config(&chamber.join("cryo.toml"), &cfg).unwrap();

    let app = Arc::new(AppState::local_only(workspace.path().to_path_buf()));
    app.refresh();
    let id = app.chambers.read().unwrap().keys().next().unwrap().clone();
    let dir = chamber.canonicalize().unwrap();
    (workspace, app, id, dir)
}

#[tokio::test]
async fn send_stamps_invite_name_ignoring_client_from() {
    // An invite may not impersonate anyone: whatever `from` the client sends
    // is discarded and the invite's own name is stamped on the message.
    let (_workspace, app, id, dir) = chamber_app("alpha");
    let role = crate::hub::tokens::Role::Invite {
        name: "Alice".into(),
        chambers: vec![id.clone()],
    };
    let payload: SendRequest =
        serde_json::from_value(json!({"body": "hi", "from": "owner-imposter"})).unwrap();

    let Json(v) = post_send(
        State(app),
        AxumPath(id),
        Some(axum::Extension(role)),
        Json(payload),
    )
    .await
    .unwrap();
    assert_eq!(v["ok"], true);

    let inbox = crate::message::read_inbox(&dir).unwrap();
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].1.from, "Alice");
    assert_eq!(inbox[0].1.body, "hi");
}

#[tokio::test]
async fn send_without_role_keeps_default_human() {
    // Local (open) mode and the owner keep today's behavior.
    let (_workspace, app, id, dir) = chamber_app("alpha");
    let payload: SendRequest = serde_json::from_value(json!({"body": "hi"})).unwrap();

    let Json(v) = post_send(State(app), AxumPath(id), None, Json(payload))
        .await
        .unwrap();
    assert_eq!(v["ok"], true);

    let inbox = crate::message::read_inbox(&dir).unwrap();
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].1.from, "human");
}

#[tokio::test]
async fn send_broadcasts_the_mailbox_id_of_the_written_message() {
    // The pushed event and `GET /api/chambers/{id}/messages` must agree on the
    // id, otherwise the client renders the message twice until the next full
    // refetch.
    let (_workspace, app, id, dir) = chamber_app("alpha");
    let mut rx = app.tx.subscribe();
    let payload: SendRequest = serde_json::from_value(json!({"body": "hi"})).unwrap();

    let Json(v) = post_send(State(app.clone()), AxumPath(id), None, Json(payload))
        .await
        .unwrap();
    assert_eq!(v["ok"], true);

    let expected = messages_json(&dir)[0]["id"].as_str().unwrap().to_string();
    assert!(expected.starts_with("inbox/"), "got {expected}");
    match rx.recv().await.unwrap() {
        SseEvent::NewMessage { id, .. } => assert_eq!(id, expected),
        other => panic!("expected NewMessage, got {other:?}"),
    }
}

#[tokio::test]
async fn send_as_owner_honors_client_supplied_from() {
    let (_workspace, app, id, dir) = chamber_app("alpha");
    let payload: SendRequest =
        serde_json::from_value(json!({"body": "hi", "from": "operator"})).unwrap();

    let Json(v) = post_send(
        State(app),
        AxumPath(id),
        Some(axum::Extension(crate::hub::tokens::Role::Owner)),
        Json(payload),
    )
    .await
    .unwrap();
    assert_eq!(v["ok"], true);

    let inbox = crate::message::read_inbox(&dir).unwrap();
    assert_eq!(inbox[0].1.from, "operator");
}

#[test]
fn lifecycle_status_json_reports_success_message() {
    let value = lifecycle_status_json(Ok(()), "Started");

    assert_eq!(
        value,
        serde_json::json!({
            "ok": true,
            "message": "Started",
        })
    );
}

#[test]
fn lifecycle_status_json_reports_error_message() {
    let value = lifecycle_status_json(Err(anyhow::anyhow!("start failed")), "Started");

    assert_eq!(
        value,
        serde_json::json!({
            "ok": false,
            "message": "start failed",
        })
    );
}
