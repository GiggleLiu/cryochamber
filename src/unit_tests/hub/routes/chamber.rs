use super::*;

#[test]
fn status_json_for_missing_state_has_zero_session() {
    let dir = tempfile::tempdir().unwrap();
    let v = status_json(dir.path());
    assert_eq!(v["running"], false);
    assert_eq!(v["session"], 0);
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
fn status_json_notes_content_empty_when_file_missing() {
    let dir = tempfile::tempdir().unwrap();
    let v = status_json(dir.path());
    assert_eq!(v["notes_content"], "");
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
    let id = arr[0]["id"].as_str().unwrap();
    assert!(id.starts_with("outbox/archive/"), "id was {id}");
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

#[test]
fn wake_response_message_reports_signal_delivery() {
    assert_eq!(wake_response_message(true), "Wake signal sent");
}

#[test]
fn wake_response_message_reports_queued_without_daemon() {
    assert_eq!(
        wake_response_message(false),
        "Message queued (no daemon running)"
    );
}

#[test]
fn lifecycle_routes_dispatch_blocking_work_off_async_handlers() {
    let source = include_str!("../../../hub/routes/chamber.rs");
    let needle = ["spawn", "blocking"].join("_");
    assert!(
        source.contains(&needle),
        "lifecycle routes should move blocking process/service work off async handlers"
    );
}

#[test]
fn post_reset_drops_stale_watcher_before_archiving() {
    // Reset renames `messages/` into `history/<ts>/`; the notify handle
    // would otherwise keep watching the archived dir and miss deliveries
    // to the freshly re-created `messages/inbox/` (e.g. zulip sync).
    let source = include_str!("../../../hub/routes/chamber.rs");
    assert!(
        source.contains("app.watchers.drop_watcher(&path);"),
        "post_reset must drop the stale watcher so app.refresh rebuilds it"
    );
}
