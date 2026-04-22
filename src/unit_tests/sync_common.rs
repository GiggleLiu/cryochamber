use super::*;
use crate::message::Message;
use chrono::NaiveDateTime;
use std::collections::BTreeMap;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn message(from: &str, subject: &str, body: &str) -> Message {
    Message {
        from: from.into(),
        subject: subject.into(),
        body: body.into(),
        timestamp: NaiveDateTime::default(),
        metadata: BTreeMap::new(),
    }
}

fn make_stub(dir: &Path, name: &str, exit_code: i32, stdout: &str) -> std::path::PathBuf {
    let p = dir.join(name);
    let mut f = std::fs::File::create(&p).unwrap();
    writeln!(f, "#!/bin/sh").unwrap();
    writeln!(f, "echo {stdout}").unwrap();
    writeln!(f, "exit {exit_code}").unwrap();
    let mut perms = std::fs::metadata(&p).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&p, perms).unwrap();
    p
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
fn summarize_all_empty_for_unconfigured_dir() {
    let dir = tempfile::tempdir().unwrap();
    assert!(summarize_all(dir.path()).is_empty());
}

#[test]
fn summarize_all_returns_configured_backends() {
    let dir = tempfile::tempdir().unwrap();
    let state = crate::gh_sync::GhSyncState {
        repo: "alice/notes".into(),
        discussion_number: 7,
        discussion_node_id: "node".into(),
        last_read_cursor: None,
        self_login: None,
        last_pushed_session: Some(3),
    };
    crate::gh_sync::save_sync_state(&dir.path().join("gh-sync.json"), &state).unwrap();

    let summaries = summarize_all(dir.path());
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].backend, SyncBackend::Gh);
    assert_eq!(summaries[0].target, "alice/notes#7");
    assert_eq!(summaries[0].last_pushed_session, Some(3));
    assert!(!summaries[0].running);
}

#[test]
fn start_invokes_sync_subcommand_via_env_override() {
    let _guard = ENV_LOCK.lock().unwrap();
    let bin = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let stub = make_stub(bin.path(), "cryo-gh-stub", 0, "ok");
    std::env::set_var("CRYO_GH_CLI", &stub);
    let res = start(SyncBackend::Gh, work.path());
    std::env::remove_var("CRYO_GH_CLI");
    assert!(res.is_ok(), "{res:?}");
}

#[test]
fn stop_propagates_non_zero_exit_as_error() {
    let _guard = ENV_LOCK.lock().unwrap();
    let bin = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let stub = make_stub(bin.path(), "cryo-gh-stub", 7, "boom");
    std::env::set_var("CRYO_GH_CLI", &stub);
    let res = stop(SyncBackend::Gh, work.path());
    std::env::remove_var("CRYO_GH_CLI");
    assert!(res.is_err());
}

#[test]
fn pull_and_push_use_zulip_env_override() {
    let _guard = ENV_LOCK.lock().unwrap();
    let bin = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let stub = make_stub(bin.path(), "cryo-zulip-stub", 0, "ok");
    std::env::set_var("CRYO_ZULIP_CLI", &stub);
    assert!(pull(SyncBackend::Zulip, work.path()).is_ok());
    assert!(push(SyncBackend::Zulip, work.path()).is_ok());
    std::env::remove_var("CRYO_ZULIP_CLI");
}

#[test]
fn wait_for_state_returns_immediately_when_already_matching() {
    let work = tempfile::tempdir().unwrap();
    // No pid file → not running. Expect `false` to match right away.
    let start = std::time::Instant::now();
    let ok = wait_for_state(
        SyncBackend::Zulip,
        work.path(),
        false,
        std::time::Duration::from_secs(2),
    );
    assert!(ok);
    assert!(
        start.elapsed() < std::time::Duration::from_millis(200),
        "fast-path should not sleep when already matching"
    );
}

#[test]
fn wait_for_state_observes_transition_before_deadline() {
    let work = tempfile::tempdir().unwrap();
    let pid_path = crate::zulip_sync::sync_pid_path(work.path());
    let pid_path_writer = pid_path.clone();

    // Drop a live pid file ~300ms after we start waiting — our own PID is
    // alive by definition, so `is_sync_running` will flip to true.
    let writer = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));
        std::fs::write(&pid_path_writer, std::process::id().to_string()).unwrap();
    });

    let ok = wait_for_state(
        SyncBackend::Zulip,
        work.path(),
        true,
        std::time::Duration::from_secs(2),
    );
    writer.join().unwrap();
    assert!(ok, "wait_for_state should observe the transition");
}

#[test]
fn wait_for_state_returns_false_on_timeout() {
    let work = tempfile::tempdir().unwrap();
    let ok = wait_for_state(
        SyncBackend::Zulip,
        work.path(),
        true,
        std::time::Duration::from_millis(250),
    );
    assert!(!ok, "no pid file will ever appear — must time out");
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
        outbox_post_style(&message("cryochamber", "Report", "summary")),
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
        "Cryochamber Report: demo",
        "Last 24h: 3 sessions, 0 failed",
    ));
    assert_eq!(
        out,
        "> **Cryochamber Report: demo**\n>\n> Last 24h: 3 sessions, 0 failed"
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
fn classify_sync_error_detects_auth_or_config() {
    let cases = [
        "HTTP 401: Requires authentication",
        "HTTP 403: Resource not accessible by integration",
        "Bad credentials",
        "Authentication failed: token expired",
        "Invalid API key",
        "permission denied on discussion",
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
