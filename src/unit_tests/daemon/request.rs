use crate::daemon::request::{
    effective_linger_secs, effective_wait_timeout, handle_dialog_request, handle_todo_request,
    FileMessageEffects, MessageEffects, TodoEffects, TodoOperationError, TodoRequest,
};
use crate::message::Message;
use crate::socket::DialogFilter;
use chrono::NaiveDate;
use std::collections::BTreeMap;

fn request_now() -> chrono::NaiveDateTime {
    chrono::NaiveDate::from_ymd_opt(2026, 7, 8)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap()
}

fn msg(from: &str, body: &str, day: u32, hour: u32) -> Message {
    Message {
        from: from.to_string(),
        subject: "S".to_string(),
        body: body.to_string(),
        timestamp: NaiveDate::from_ymd_opt(2026, 4, day)
            .unwrap()
            .and_hms_opt(hour, 0, 0)
            .unwrap(),
        metadata: BTreeMap::new(),
        is_question: false,
    }
}

#[derive(Default)]
struct FakeMessages {
    pending: Vec<(String, Message)>,
    archived_inbox: Vec<(String, Message)>,
    pending_outbox: Vec<(String, Message)>,
    archived_outbox: Vec<(String, Message)>,
}

impl MessageEffects for FakeMessages {
    fn claim_inbox_batch(
        &mut self,
    ) -> std::result::Result<Vec<(String, Message)>, TodoOperationError> {
        let claimed = std::mem::take(&mut self.pending);
        for (name, message) in &claimed {
            self.archived_inbox.push((name.clone(), message.clone()));
        }
        Ok(claimed)
    }

    fn read_inbox_archive(
        &self,
    ) -> std::result::Result<Vec<(String, Message)>, TodoOperationError> {
        Ok(self.archived_inbox.clone())
    }

    fn read_outbox(&self) -> std::result::Result<Vec<(String, Message)>, TodoOperationError> {
        Ok(self.pending_outbox.clone())
    }

    fn read_outbox_archive(
        &self,
    ) -> std::result::Result<Vec<(String, Message)>, TodoOperationError> {
        Ok(self.archived_outbox.clone())
    }
}

#[derive(Default)]
struct FakeTodos {
    added: Vec<(String, String)>,
}

impl TodoEffects for FakeTodos {
    fn add_todo(&mut self, text: &str, at: &str) -> std::result::Result<u32, TodoOperationError> {
        self.added.push((text.to_string(), at.to_string()));
        Ok(self.added.len() as u32)
    }

    fn done_todo(&mut self, _id: u32) -> std::result::Result<(), TodoOperationError> {
        Ok(())
    }

    fn remove_todo(&mut self, _id: u32) -> std::result::Result<(), TodoOperationError> {
        Ok(())
    }

    fn list_todos(&mut self) -> std::result::Result<String, TodoOperationError> {
        Ok(String::new())
    }
}

#[test]
fn todo_add_rejects_unparseable_at_without_adding() {
    let mut fake = FakeTodos::default();
    let out = handle_todo_request(
        TodoRequest::Add {
            text: "water the plants".to_string(),
            at: "tomorrow 9am".to_string(),
        },
        &mut fake,
        request_now(),
    );
    assert!(!out.ok, "an unparseable at must be rejected");
    assert!(out.message.contains("not a valid scheduled time"));
    assert!(out.message.contains("cryo-agent time"));
    assert!(out.log_event.is_none());
    assert!(
        fake.added.is_empty(),
        "rejected add must not create a todo item"
    );
}

#[test]
fn todo_add_rejects_empty_at() {
    let mut fake = FakeTodos::default();
    let out = handle_todo_request(
        TodoRequest::Add {
            text: "task".to_string(),
            at: String::new(),
        },
        &mut fake,
        request_now(),
    );
    assert!(!out.ok, "empty at must be rejected");
    assert!(fake.added.is_empty());
}

#[test]
fn todo_add_accepts_valid_at() {
    let mut fake = FakeTodos::default();
    let out = handle_todo_request(
        TodoRequest::Add {
            text: "task".to_string(),
            at: "2026-04-25T10:00".to_string(),
        },
        &mut fake,
        request_now(),
    );
    assert!(out.ok);
    assert_eq!(
        fake.added,
        vec![("task".to_string(), "2026-04-25T10:00".to_string())]
    );
}

#[test]
fn todo_add_normalizes_relative_offset_against_now() {
    let mut fake = FakeTodos::default();
    let out = handle_todo_request(
        TodoRequest::Add {
            text: "water the plants".to_string(),
            at: "+30 minutes".to_string(),
        },
        &mut fake,
        request_now(),
    );
    assert!(out.ok, "relative offsets must be accepted: {out:?}");
    assert_eq!(
        fake.added,
        vec![(
            "water the plants".to_string(),
            "2026-07-08T12:30".to_string()
        )]
    );
}

#[test]
fn todo_add_normalizes_seconds_to_canonical_minute() {
    let mut fake = FakeTodos::default();
    let out = handle_todo_request(
        TodoRequest::Add {
            text: "brief".to_string(),
            at: "2026-08-01T09:15:59".to_string(),
        },
        &mut fake,
        request_now(),
    );
    assert!(out.ok);
    assert_eq!(fake.added[0].1, "2026-08-01T09:15");
}

#[test]
fn todo_add_rejects_tz_offset_loudly() {
    let mut fake = FakeTodos::default();
    let out = handle_todo_request(
        TodoRequest::Add {
            text: "daily brief".to_string(),
            at: "2026-05-12T07:30+08:00".to_string(),
        },
        &mut fake,
        request_now(),
    );
    assert!(!out.ok, "a TZ-offset at must be rejected");
    assert!(out.message.contains("not a valid scheduled time"));
    assert!(fake.added.is_empty());
}

#[test]
fn dialog_with_pending_claims_and_marks_new() {
    let mut fake = FakeMessages {
        pending: vec![("2026-04-25T09-30-h.md".into(), msg("human", "fresh", 25, 9))],
        archived_inbox: vec![("2026-04-24T18-00-h.md".into(), msg("human", "old", 24, 18))],
        pending_outbox: vec![],
        archived_outbox: vec![],
    };
    let out = handle_dialog_request(DialogFilter::All, &[], &mut fake);
    assert!(out.ok);
    assert_eq!(
        out.claimed_filenames,
        vec!["2026-04-25T09-30-h.md".to_string()]
    );
    assert!(out.message.contains("old"));
    assert!(out.message.contains("fresh"));
    assert!(out.message.contains("new since last session"));
    assert_eq!(fake.pending.len(), 0);
    assert_eq!(fake.archived_inbox.len(), 2);
}

#[test]
fn dialog_with_no_pending_no_marker() {
    let mut fake = FakeMessages {
        pending: vec![],
        archived_inbox: vec![("2026-04-24T18-00-h.md".into(), msg("human", "old", 24, 18))],
        pending_outbox: vec![],
        archived_outbox: vec![("2026-04-24T18-05-a.md".into(), msg("agent", "hi", 24, 18))],
    };
    let out = handle_dialog_request(DialogFilter::All, &[], &mut fake);
    assert!(out.ok);
    assert!(out.claimed_filenames.is_empty());
    assert!(!out.message.contains("new since last session"));
}

#[test]
fn dialog_uses_extra_session_new_when_no_pending() {
    let mut fake = FakeMessages {
        pending: vec![],
        archived_inbox: vec![
            ("2026-04-24T18-00-h.md".into(), msg("human", "old", 24, 18)),
            (
                "2026-04-25T09-30-h.md".into(),
                msg("human", "received-earlier", 25, 9),
            ),
        ],
        pending_outbox: vec![],
        archived_outbox: vec![],
    };
    let out = handle_dialog_request(
        DialogFilter::All,
        &["2026-04-25T09-30-h.md".to_string()],
        &mut fake,
    );
    assert!(out.ok);
    assert!(out.message.contains("new since last session"));
    assert!(out.message.contains("received-earlier"));
}

#[test]
fn dialog_includes_pending_outbox_messages() {
    let dir = tempfile::tempdir().unwrap();
    let store = crate::channel::store::MessageStore::new(dir.path().to_path_buf());
    store.ensure_dirs().unwrap();
    store
        .send_out(&msg("agent", "pending reply", 25, 9))
        .unwrap();

    let mut effects = FileMessageEffects::new(dir.path());
    let out = handle_dialog_request(DialogFilter::All, &[], &mut effects);

    assert!(out.ok);
    assert!(
        out.message.contains("pending reply"),
        "pending outbox messages should be part of dialog history: {:?}",
        out.message
    );
}

#[test]
fn dialog_last_zero_rejected() {
    let mut fake = FakeMessages::default();
    let out = handle_dialog_request(DialogFilter::LastN { count: 0 }, &[], &mut fake);
    assert!(!out.ok);
    assert!(out.message.contains("--last must be at least 1"));
}

#[test]
fn dialog_since_unparseable_rejected() {
    let mut fake = FakeMessages::default();
    let out = handle_dialog_request(
        DialogFilter::Since {
            iso: "yesterday".to_string(),
        },
        &[],
        &mut fake,
    );
    assert!(!out.ok);
    assert!(out.message.contains("not a recognized timestamp"));
}

#[test]
fn dialog_since_iso_filters() {
    let mut fake = FakeMessages {
        pending: vec![],
        archived_inbox: vec![
            ("a.md".into(), msg("human", "old", 24, 18)),
            ("b.md".into(), msg("human", "fresh", 25, 9)),
        ],
        pending_outbox: vec![],
        archived_outbox: vec![],
    };
    let out = handle_dialog_request(
        DialogFilter::Since {
            iso: "2026-04-25".to_string(),
        },
        &[],
        &mut fake,
    );
    assert!(out.ok);
    assert!(!out.message.contains("old"));
    assert!(out.message.contains("fresh"));
}

#[test]
fn effective_wait_timeout_uses_request_value() {
    assert_eq!(effective_wait_timeout(Some(300), 14400), 300);
}

#[test]
fn effective_wait_timeout_falls_back_to_default() {
    assert_eq!(effective_wait_timeout(None, 7200), 7200);
}

#[test]
fn effective_wait_timeout_clamps_to_cap() {
    assert_eq!(
        effective_wait_timeout(Some(999_999), 14400),
        crate::config::MAX_WAIT_TIMEOUT_SECS
    );
    assert_eq!(
        effective_wait_timeout(None, 999_999),
        crate::config::MAX_WAIT_TIMEOUT_SECS
    );
}

#[test]
fn effective_wait_timeout_zero_becomes_one_second() {
    assert_eq!(effective_wait_timeout(Some(0), 14400), 1);
}

#[test]
fn test_effective_linger_secs() {
    // Cap applies; zero is allowed (no window).
    assert_eq!(effective_linger_secs(600), 600);
    assert_eq!(effective_linger_secs(0), 0);
    assert_eq!(
        effective_linger_secs(1_000_000),
        crate::config::MAX_WAIT_TIMEOUT_SECS
    );
}

#[test]
fn receive_wait_socket_request_maps_to_daemon_request() {
    let mapped = crate::daemon::request::DaemonRequest::from(crate::socket::Request::ReceiveWait {
        timeout_secs: Some(60),
    });
    assert_eq!(
        mapped,
        crate::daemon::request::DaemonRequest::ReceiveWait {
            timeout_secs: Some(60)
        }
    );
}
