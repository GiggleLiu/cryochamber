use super::*;
use crate::message::Message;
use chrono::NaiveDateTime;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

fn message(from: &str, subject: &str, body: &str) -> Message {
    Message {
        from: from.into(),
        subject: subject.into(),
        body: body.into(),
        timestamp: NaiveDateTime::default(),
        metadata: BTreeMap::new(),
        is_question: false,
    }
}

#[test]
fn backend_parse_roundtrip() {
    assert_eq!(SyncBackend::parse("gh"), Some(SyncBackend::Gh));
    assert_eq!(SyncBackend::parse("zulip"), Some(SyncBackend::Zulip));
    assert_eq!(SyncBackend::parse("nope"), None);
    assert_eq!(SyncBackend::Gh.as_str(), "gh");
    assert_eq!(SyncBackend::Zulip.as_str(), "zulip");
}

#[test]
fn sync_common_does_not_dispatch_to_concrete_backends() {
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/sync_common.rs"),
    )
    .unwrap();
    assert!(
        !source.contains("crate::gh_sync") && !source.contains("crate::zulip_sync"),
        "sync_common must stay backend-neutral; concrete backend dispatch belongs in sync_control"
    );
}

struct RecordingLoopBackend {
    log: Arc<Mutex<Vec<&'static str>>>,
    shutdown: Arc<AtomicBool>,
    receive_calls: usize,
    skip_first_send: bool,
}

impl RecordingLoopBackend {
    fn new(log: Arc<Mutex<Vec<&'static str>>>, shutdown: Arc<AtomicBool>) -> Self {
        Self {
            log,
            shutdown,
            receive_calls: 0,
            skip_first_send: false,
        }
    }

    fn skip_first_send(mut self) -> Self {
        self.skip_first_send = true;
        self
    }
}

impl SyncLoopBackend for RecordingLoopBackend {
    fn receive(&mut self) -> anyhow::Result<SyncLoopCommand> {
        self.log.lock().unwrap().push("receive");
        self.receive_calls += 1;
        if self.receive_calls == 2 {
            self.shutdown.store(true, Ordering::Relaxed);
        }
        if self.skip_first_send && self.receive_calls == 1 {
            Ok(SyncLoopCommand::SkipSend)
        } else {
            Ok(SyncLoopCommand::Send)
        }
    }

    fn send(&mut self) -> anyhow::Result<SyncCycleStatus> {
        self.log.lock().unwrap().push("send");
        Ok(SyncCycleStatus::Continue)
    }
}

#[test]
fn sync_loop_receives_then_sends_each_cycle_after_outbox_event() {
    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(()).unwrap();
    let shutdown = Arc::new(AtomicBool::new(false));
    let log = Arc::new(Mutex::new(Vec::new()));
    let mut backend = RecordingLoopBackend::new(Arc::clone(&log), Arc::clone(&shutdown));

    let started = std::time::Instant::now();
    run_sync_loop_with_settle_delay(
        "test sync",
        Arc::clone(&shutdown),
        rx,
        std::time::Duration::from_secs(30),
        std::time::Duration::from_millis(0),
        &mut backend,
    )
    .unwrap();

    assert!(
        started.elapsed() < std::time::Duration::from_secs(1),
        "queued outbox event should start the next receive/send cycle without waiting for the interval"
    );
    assert_eq!(
        *log.lock().unwrap(),
        vec!["receive", "send", "receive", "send"]
    );
}

struct HaltingBackend {
    log: Arc<Mutex<Vec<&'static str>>>,
}

impl SyncLoopBackend for HaltingBackend {
    fn receive(&mut self) -> anyhow::Result<SyncLoopCommand> {
        self.log.lock().unwrap().push("receive");
        Ok(SyncLoopCommand::Halt {
            reason: "mis-set-up for test".into(),
        })
    }

    fn send(&mut self) -> anyhow::Result<SyncCycleStatus> {
        self.log.lock().unwrap().push("send");
        Ok(SyncCycleStatus::Continue)
    }
}

#[test]
fn sync_loop_halts_on_halt_command_without_sending() {
    // Halt is reserved for unrecoverable state (bad auth/config). The loop
    // must exit cleanly AND must not call send on this cycle — sending with
    // a known-bad client would just multiply the failure signal.
    let (_tx, rx) = std::sync::mpsc::channel::<()>();
    let shutdown = Arc::new(AtomicBool::new(false));
    let log = Arc::new(Mutex::new(Vec::new()));
    let mut backend = HaltingBackend {
        log: Arc::clone(&log),
    };

    run_sync_loop_with_settle_delay(
        "test sync",
        Arc::clone(&shutdown),
        rx,
        std::time::Duration::from_secs(30),
        std::time::Duration::from_millis(0),
        &mut backend,
    )
    .unwrap();

    assert_eq!(
        *log.lock().unwrap(),
        vec!["receive"],
        "Halt exits before send and does not start another cycle"
    );
}

#[test]
fn sync_loop_can_skip_send_for_a_receive_cycle() {
    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(()).unwrap();
    let shutdown = Arc::new(AtomicBool::new(false));
    let log = Arc::new(Mutex::new(Vec::new()));
    let mut backend =
        RecordingLoopBackend::new(Arc::clone(&log), Arc::clone(&shutdown)).skip_first_send();

    run_sync_loop_with_settle_delay(
        "test sync",
        Arc::clone(&shutdown),
        rx,
        std::time::Duration::from_secs(30),
        std::time::Duration::from_millis(0),
        &mut backend,
    )
    .unwrap();

    assert_eq!(*log.lock().unwrap(), vec!["receive", "receive", "send"]);
}

#[test]
fn outbox_post_style_uses_body_only_for_agent() {
    assert_eq!(
        outbox_post_style(&message("agent", "Reply", "hello human")),
        OutboxPostStyle::BodyOnly
    );
}

#[test]
fn outbox_post_style_uses_system_quote_for_cryochamber() {
    assert_eq!(
        outbox_post_style(&message("cryochamber", "Fallback", "summary")),
        OutboxPostStyle::SystemQuote
    );
}

#[test]
fn outbox_post_style_uses_attribution_for_other_senders() {
    assert_eq!(
        outbox_post_style(&message("teammate", "Question", "Are you free?")),
        OutboxPostStyle::Attributed
    );
}

#[test]
fn format_outbox_post_uses_body_only_for_agent_reply() {
    let out = format_outbox_post(&message("agent", "Reply", "hello human"));
    assert_eq!(out, "hello human");
}

#[test]
fn format_outbox_post_marks_system_messages_as_quotes() {
    let out = format_outbox_post(&message(
        "cryochamber",
        "Fallback Alert: demo",
        "Agent hibernated without replying.",
    ));
    assert_eq!(
        out,
        "> **Fallback Alert: demo**\n>\n> Agent hibernated without replying."
    );
}

#[test]
fn format_outbox_post_quotes_each_system_message_line() {
    let out = format_outbox_post(&message(
        "cryochamber",
        "Fallback Alert: deadline_missed",
        "Agent exceeded max retries.\nNext attempt in 60s.",
    ));
    assert_eq!(
        out,
        "> **Fallback Alert: deadline_missed**\n>\n> Agent exceeded max retries.\n> Next attempt in 60s."
    );
}

#[test]
fn format_outbox_post_keeps_attribution_for_other_senders() {
    let out = format_outbox_post(&message("teammate", "Question", "Are you free?"));
    assert_eq!(out, "**teammate** (Question)\n\nAre you free?");
}

#[test]
fn push_outbox_messages_returns_post_error_and_leaves_failed_message_unarchived() {
    let dir = tempfile::tempdir().unwrap();
    let store = crate::channel::store::MessageStore::new(dir.path().to_path_buf());
    store.send_out(&message("agent", "Reply", "hello")).unwrap();

    let err = push_outbox_messages(
        dir.path(),
        |_| "posted".to_string(),
        |_| anyhow::bail!("HTTP 401: Bad credentials"),
    )
    .unwrap_err();

    assert!(
        format!("{err:#}").contains("HTTP 401"),
        "post error should be preserved: {err:#}"
    );
    assert_eq!(store.read_outbox_named().unwrap().len(), 1);
    assert!(store.read_outbox_archive_named().unwrap().is_empty());
}

#[test]
fn push_outbox_messages_archives_successful_posts() {
    let dir = tempfile::tempdir().unwrap();
    let store = crate::channel::store::MessageStore::new(dir.path().to_path_buf());
    store.send_out(&message("agent", "Reply", "hello")).unwrap();
    let mut posted = Vec::new();

    push_outbox_messages(
        dir.path(),
        |_| "posted".to_string(),
        |body| {
            posted.push(body.to_string());
            Ok(())
        },
    )
    .unwrap();

    assert_eq!(posted, vec!["hello".to_string()]);
    assert!(store.read_outbox_named().unwrap().is_empty());
    assert_eq!(store.read_outbox_archive_named().unwrap().len(), 1);
}

#[test]
fn classify_sync_error_detects_auth_or_config() {
    let cases = [
        "HTTP 401: Requires authentication",
        "HTTP 403: Resource not accessible by integration",
        "Bad credentials",
        "Authentication failed: token expired",
        "Invalid API key",
    ];
    for msg in cases {
        let err = anyhow::anyhow!(msg.to_string());
        assert_eq!(
            classify_sync_error(&err),
            SyncErrorKind::AuthOrConfig,
            "expected AuthOrConfig for {msg:?}"
        );
    }
}

#[test]
fn classify_sync_error_treats_unknown_as_transient() {
    let cases = [
        "connection reset by peer",
        "HTTP 500: internal server error",
        "HTTP 502: bad gateway",
        "timeout waiting for response",
        "HTTP 429: rate limited",
        "gh: unknown non-auth error occurred",
        "failed to archive outbox: Permission denied",
        "Permission denied reading messages/inbox",
    ];
    for msg in cases {
        let err = anyhow::anyhow!(msg.to_string());
        assert_eq!(
            classify_sync_error(&err),
            SyncErrorKind::Transient,
            "expected Transient for {msg:?}"
        );
    }
}

#[test]
fn classify_sync_error_walks_error_chain() {
    // The real anyhow chains we care about stack a context() wrapper over the
    // underlying stderr text. The classifier must look past the outer layer.
    let inner = anyhow::anyhow!("gh api graphql failed: HTTP 401: Bad credentials");
    let wrapped = inner.context("pull_comments");
    assert_eq!(classify_sync_error(&wrapped), SyncErrorKind::AuthOrConfig);
}

struct HaltingSendBackend {
    log: Arc<Mutex<Vec<&'static str>>>,
}

impl SyncLoopBackend for HaltingSendBackend {
    fn receive(&mut self) -> anyhow::Result<SyncLoopCommand> {
        self.log.lock().unwrap().push("receive");
        Ok(SyncLoopCommand::Send)
    }
    fn send(&mut self) -> anyhow::Result<SyncCycleStatus> {
        self.log.lock().unwrap().push("send");
        Ok(SyncCycleStatus::Halt {
            reason: "push auth failed".into(),
        })
    }
}

#[test]
fn sync_loop_halts_when_send_reports_halt() {
    // Push-side auth/config failure must stop the loop cleanly, not silently
    // retry. After the halting send we should see exactly one receive+send
    // and no further cycles.
    let (_tx, rx) = std::sync::mpsc::channel::<()>();
    let shutdown = Arc::new(AtomicBool::new(false));
    let log = Arc::new(Mutex::new(Vec::new()));
    let mut backend = HaltingSendBackend {
        log: Arc::clone(&log),
    };

    run_sync_loop_with_settle_delay(
        "test sync",
        Arc::clone(&shutdown),
        rx,
        std::time::Duration::from_secs(30),
        std::time::Duration::from_millis(0),
        &mut backend,
    )
    .unwrap();

    assert_eq!(*log.lock().unwrap(), vec!["receive", "send"]);
}
