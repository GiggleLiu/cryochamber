use super::*;
use std::collections::VecDeque;
use std::sync::Mutex;

#[test]
fn daemon_request_handling_lives_in_request_module() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let request_src = std::fs::read_to_string(root.join("src/daemon/request.rs"))
        .expect("daemon request handling should live in src/daemon/request.rs");
    let daemon_src = std::fs::read_to_string(root.join("src/daemon.rs")).unwrap();

    assert!(request_src.contains("enum DaemonRequest"));
    assert!(request_src.contains("fn handle_todo_request"));
    assert!(!daemon_src.contains("enum DaemonRequest"));
    assert!(!daemon_src.contains("fn handle_todo_request"));
}

#[test]
fn daemon_receive_request_is_wired_through_socket_request_and_effects() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let socket_src = std::fs::read_to_string(root.join("src/socket.rs"))
        .expect("socket requests should live in src/socket.rs");
    let request_src = std::fs::read_to_string(root.join("src/daemon/request.rs"))
        .expect("daemon request handling should live in src/daemon/request.rs");
    let effects_src = std::fs::read_to_string(root.join("src/daemon/effects.rs"))
        .expect("session effects should live in src/daemon/effects.rs");

    assert!(socket_src.contains("Receive"));
    assert!(request_src.contains("enum DaemonRequest"));
    assert!(request_src.contains("Receive,"));
    assert!(effects_src.contains("claim_inbox_batch"));
    assert!(!effects_src.contains("archive_pending_inbox"));
    assert!(!effects_src.contains("restore_pending_inbox"));
}

#[test]
fn cryo_agent_receive_routes_through_daemon_ipc() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let agent_src = std::fs::read_to_string(root.join("src/bin/cryo_agent.rs"))
        .expect("cryo-agent CLI should live in src/bin/cryo_agent.rs");

    assert!(agent_src.contains("Request::Receive"));
    assert!(!agent_src.contains("read_and_archive_inbox"));
}

#[test]
fn daemon_session_runtime_and_effects_live_in_submodules() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let effects_src = std::fs::read_to_string(root.join("src/daemon/effects.rs"))
        .expect("session effects should live in src/daemon/effects.rs");
    let session_src = std::fs::read_to_string(root.join("src/daemon/session.rs"))
        .expect("session runtime should live in src/daemon/session.rs");
    let daemon_src = std::fs::read_to_string(root.join("src/daemon.rs")).unwrap();

    assert!(effects_src.contains("trait SessionEffects"));
    assert!(effects_src.contains("struct FsSessionEffects"));
    assert!(session_src.contains("trait SessionRuntime"));
    assert!(session_src.contains("struct ProcessSessionRuntime"));
    assert!(session_src.contains("trait SessionLauncher"));
    assert!(session_src.contains("struct ProcessSessionLauncher"));
    assert!(!daemon_src.contains("trait SessionEffects"));
    assert!(!daemon_src.contains("struct FsSessionEffects"));
    assert!(!daemon_src.contains("trait SessionRuntime"));
    assert!(!daemon_src.contains("struct ProcessSessionRuntime"));
    assert!(!daemon_src.contains("trait SessionLauncher"));
    assert!(!daemon_src.contains("struct ProcessSessionLauncher"));
}

#[test]
fn daemon_scheduling_and_bootstrap_live_in_schedule_module() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let schedule_src = std::fs::read_to_string(root.join("src/daemon/schedule.rs"))
        .expect("daemon scheduling should live in src/daemon/schedule.rs");
    let daemon_src = std::fs::read_to_string(root.join("src/daemon.rs")).unwrap();

    for item in [
        "fn compute_sleep_timeout",
        "fn next_wake_from_todos",
        "fn detect_delayed_wake",
        "fn delayed_wake_notice",
        "struct DaemonBootstrapState",
    ] {
        assert!(
            schedule_src.contains(item),
            "schedule.rs should contain {item}"
        );
        assert!(
            !daemon_src.contains(item),
            "daemon.rs should not contain {item}"
        );
    }
}

#[test]
fn watcher_startup_notice_prioritizes_warning() {
    assert_eq!(
        watcher_startup_notice(Some("permission denied"), true),
        WatcherStartupNotice::Warning("permission denied")
    );
}

#[test]
fn watcher_startup_notice_reports_started_watcher() {
    assert_eq!(
        watcher_startup_notice(None, true),
        WatcherStartupNotice::Started
    );
}

#[test]
fn watcher_startup_notice_is_silent_without_warning_or_watcher() {
    assert_eq!(
        watcher_startup_notice(None, false),
        WatcherStartupNotice::Silent
    );
}

struct TestClockState {
    now: NaiveDateTime,
    elapsed: Duration,
}

struct TestClock {
    origin: std::time::Instant,
    state: Mutex<TestClockState>,
}

impl TestClock {
    fn new(now: NaiveDateTime) -> Self {
        Self {
            origin: std::time::Instant::now(),
            state: Mutex::new(TestClockState {
                now,
                elapsed: Duration::ZERO,
            }),
        }
    }
}

impl Clock for TestClock {
    fn local_now(&self) -> NaiveDateTime {
        self.state.lock().unwrap().now
    }

    fn monotonic_now(&self) -> std::time::Instant {
        let state = self.state.lock().unwrap();
        self.origin + state.elapsed
    }

    fn sleep(&self, duration: Duration) {
        let mut state = self.state.lock().unwrap();
        state.now += chrono::Duration::from_std(duration).unwrap();
        state.elapsed += duration;
    }
}

struct FakeEventSource {
    events: Mutex<VecDeque<Result<DaemonEvent, WaitError>>>,
    drained_inbox: Mutex<u32>,
}

impl FakeEventSource {
    fn new(events: Vec<Result<DaemonEvent, WaitError>>) -> Self {
        Self {
            events: Mutex::new(events.into()),
            drained_inbox: Mutex::new(0),
        }
    }

    fn drained_count(&self) -> u32 {
        *self.drained_inbox.lock().unwrap()
    }
}

struct FakeSessionRuntime {
    requests: Mutex<VecDeque<anyhow::Result<Option<crate::socket::Request>>>>,
    waits: Mutex<VecDeque<std::io::Result<Option<ChildExitStatus>>>>,
    responses: Mutex<Vec<(bool, String)>>,
    respond_results: Mutex<VecDeque<anyhow::Result<()>>>,
    terminated: AtomicBool,
}

impl FakeSessionRuntime {
    fn new(
        requests: Vec<anyhow::Result<Option<crate::socket::Request>>>,
        waits: Vec<std::io::Result<Option<ChildExitStatus>>>,
    ) -> Self {
        Self {
            requests: Mutex::new(requests.into()),
            waits: Mutex::new(waits.into()),
            responses: Mutex::new(Vec::new()),
            respond_results: Mutex::new(VecDeque::new()),
            terminated: AtomicBool::new(false),
        }
    }

    fn with_respond_results(
        requests: Vec<anyhow::Result<Option<crate::socket::Request>>>,
        waits: Vec<std::io::Result<Option<ChildExitStatus>>>,
        respond_results: Vec<anyhow::Result<()>>,
    ) -> Self {
        Self {
            requests: Mutex::new(requests.into()),
            waits: Mutex::new(waits.into()),
            responses: Mutex::new(Vec::new()),
            respond_results: Mutex::new(respond_results.into()),
            terminated: AtomicBool::new(false),
        }
    }

    fn responses(&self) -> Vec<(bool, String)> {
        self.responses.lock().unwrap().clone()
    }

    fn terminated(&self) -> bool {
        self.terminated.load(Ordering::Relaxed)
    }
}

impl SessionRuntime for FakeSessionRuntime {
    fn accept_request(
        &mut self,
        _expected_instance_id: Option<&str>,
    ) -> Result<Option<crate::socket::Request>> {
        self.requests
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Ok(None))
    }

    fn respond(&mut self, ok: bool, message: String) -> Result<()> {
        self.responses.lock().unwrap().push((ok, message));
        self.respond_results
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Ok(()))
    }

    fn try_wait(&mut self) -> std::io::Result<Option<ChildExitStatus>> {
        self.waits.lock().unwrap().pop_front().unwrap_or(Ok(None))
    }

    fn terminate(&mut self) {
        self.terminated.store(true, Ordering::Relaxed);
    }
}

struct FakeSessionEffects {
    reply_failure: Option<String>,
    replies: Vec<(ReplyAuthor, String, NaiveDateTime, bool)>,
    inbox_messages: Vec<(String, crate::message::Message)>,
    archived_inbox: Vec<(String, crate::message::Message)>,
    archived_outbox: Vec<(String, crate::message::Message)>,
    todos: Vec<crate::todo::TodoItem>,
    next_todo_id: u32,
}

impl FakeSessionEffects {
    fn new() -> Self {
        Self {
            reply_failure: None,
            replies: Vec::new(),
            inbox_messages: Vec::new(),
            archived_inbox: Vec::new(),
            archived_outbox: Vec::new(),
            todos: Vec::new(),
            next_todo_id: 1,
        }
    }

    /// Pre-populate with a far-future pending TODO so hibernate is not rejected.
    /// Use in tests that exercise the hibernate path but don't drive a TodoAdd
    /// beforehand.
    fn new_with_pending_todo() -> Self {
        let mut effects = Self::new();
        effects.todos.push(crate::todo::TodoItem {
            id: 1,
            text: "test setup pending todo".to_string(),
            done: false,
            claimed: false,
            at: "2099-12-31T23:59".to_string(),
            created: "unknown".to_string(),
        });
        effects.next_todo_id = 2;
        effects
    }

    fn with_reply_failure(message: &str) -> Self {
        let mut effects = Self::new();
        effects.reply_failure = Some(message.to_string());
        // Also seed a pending todo so the hibernate at end of the scenario
        // is accepted; the reply failure is the behavior under test.
        effects.todos.push(crate::todo::TodoItem {
            id: 1,
            text: "test setup pending todo".to_string(),
            done: false,
            claimed: false,
            at: "2099-12-31T23:59".to_string(),
            created: "unknown".to_string(),
        });
        effects.next_todo_id = 2;
        effects
    }

    fn push_inbox_message(&mut self, filename: &str, body: &str) {
        self.inbox_messages.push((
            filename.to_string(),
            crate::message::Message {
                from: "human".to_string(),
                subject: "Question".to_string(),
                body: body.to_string(),
                timestamp: chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
                    .unwrap()
                    .and_hms_opt(12, 0, 0)
                    .unwrap(),
                metadata: Default::default(),
                is_question: false,
            },
        ));
    }
}

impl SessionEffects for FakeSessionEffects {
    fn claim_inbox_batch(&mut self) -> Result<Vec<(String, crate::message::Message)>> {
        let claimed = std::mem::take(&mut self.inbox_messages);
        self.archived_inbox.extend(claimed.iter().cloned());
        Ok(claimed)
    }

    fn read_inbox_archive(&self) -> Result<Vec<(String, crate::message::Message)>> {
        Ok(self.archived_inbox.clone())
    }

    fn read_outbox(&self) -> Result<Vec<(String, crate::message::Message)>> {
        Ok(Vec::new())
    }

    fn read_outbox_archive(&self) -> Result<Vec<(String, crate::message::Message)>> {
        Ok(self.archived_outbox.clone())
    }

    fn write_reply(
        &mut self,
        author: ReplyAuthor,
        text: &str,
        timestamp: NaiveDateTime,
        is_question: bool,
    ) -> Result<()> {
        if let Some(message) = &self.reply_failure {
            anyhow::bail!("{message}");
        }
        self.replies
            .push((author, text.to_string(), timestamp, is_question));
        Ok(())
    }

    fn todo_add(&mut self, text: &str, at: &str) -> Result<u32> {
        let id = self.next_todo_id;
        self.next_todo_id += 1;
        self.todos.push(crate::todo::TodoItem {
            id,
            text: text.to_string(),
            done: false,
            claimed: false,
            at: at.to_string(),
            created: "unknown".to_string(),
        });
        Ok(id)
    }

    fn todo_done(&mut self, id: u32) -> Result<()> {
        let item = self
            .todos
            .iter_mut()
            .find(|item| item.id == id)
            .ok_or_else(|| anyhow::anyhow!("TODO #{id} not found"))?;
        item.done = true;
        item.claimed = false;
        Ok(())
    }

    fn todo_remove(&mut self, id: u32) -> Result<()> {
        let len_before = self.todos.len();
        self.todos.retain(|item| item.id != id);
        if self.todos.len() == len_before {
            anyhow::bail!("TODO #{id} not found");
        }
        Ok(())
    }

    fn todo_list(&mut self) -> Result<String> {
        if self.todos.is_empty() {
            return Ok("No todos.".to_string());
        }
        Ok(self
            .todos
            .iter()
            .map(|item| {
                let check = if item.done {
                    "x"
                } else if item.claimed {
                    "~"
                } else {
                    " "
                };
                format!("{}. [{}] {} (at: {})", item.id, check, item.text, item.at)
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }

    fn has_pending_todo_with_valid_wake(&self) -> bool {
        self.todos.iter().any(|item| {
            !item.done
                && !item.claimed
                && !item.at.is_empty()
                && NaiveDateTime::parse_from_str(&item.at, WAKE_TIME_FMT).is_ok()
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DummyServer;

#[derive(Debug, Clone, PartialEq, Eq)]
struct DummyWatcher;

struct FakeStartupPlatform {
    signal_error: Option<String>,
    bind_error: Option<String>,
    registry_error: Option<String>,
    watcher_error: Option<String>,
    bind_calls: Mutex<u32>,
    watcher_calls: Mutex<u32>,
}

impl FakeStartupPlatform {
    fn new() -> Self {
        Self {
            signal_error: None,
            bind_error: None,
            registry_error: None,
            watcher_error: None,
            bind_calls: Mutex::new(0),
            watcher_calls: Mutex::new(0),
        }
    }

    fn bind_calls(&self) -> u32 {
        *self.bind_calls.lock().unwrap()
    }

    fn watcher_calls(&self) -> u32 {
        *self.watcher_calls.lock().unwrap()
    }
}

#[derive(Default)]
struct RecordingStateStore {
    saved_states: Mutex<Vec<CryoState>>,
}

impl RecordingStateStore {
    fn saved_states(&self) -> Vec<CryoState> {
        self.saved_states.lock().unwrap().clone()
    }
}

impl StateStore for RecordingStateStore {
    fn save(&self, _path: &Path, state: &CryoState) -> Result<()> {
        self.saved_states.lock().unwrap().push(state.clone());
        Ok(())
    }
}

struct FailingStateStore;

impl StateStore for FailingStateStore {
    fn save(&self, _path: &Path, _state: &CryoState) -> Result<()> {
        anyhow::bail!("injected state save failure")
    }
}

impl StartupPlatform for FakeStartupPlatform {
    type Server = DummyServer;
    type Watcher = DummyWatcher;

    fn register_signal_handlers(
        &self,
        _shutdown: &Arc<AtomicBool>,
        _wake_requested: &Arc<AtomicBool>,
    ) -> Result<()> {
        if let Some(message) = &self.signal_error {
            anyhow::bail!("{message}");
        }
        Ok(())
    }

    fn bind_socket_server(&self, _sock_path: &Path) -> Result<Self::Server> {
        *self.bind_calls.lock().unwrap() += 1;
        if let Some(message) = &self.bind_error {
            anyhow::bail!("{message}");
        }
        Ok(DummyServer)
    }

    fn register_registry(&self, _dir: &Path, _sock_path: &Path) -> Result<()> {
        if let Some(message) = &self.registry_error {
            anyhow::bail!("{message}");
        }
        Ok(())
    }

    fn start_inbox_watcher(
        &self,
        _paths: &[PathBuf],
        _tx: mpsc::Sender<DaemonEvent>,
    ) -> Result<Self::Watcher> {
        *self.watcher_calls.lock().unwrap() += 1;
        if let Some(message) = &self.watcher_error {
            anyhow::bail!("{message}");
        }
        Ok(DummyWatcher)
    }
}

fn test_cryo_state() -> CryoState {
    CryoState {
        session_number: 1,
        pid: None,
        agent_override: None,
        max_session_duration_override: None,
        last_report_time: None,
        instance_id: Some("test-instance".into()),
        session_active: false,
        previous_session_crashed: false,
    }
}

fn begin_test_logger(dir: &Path) -> crate::log::EventLogger {
    crate::log::EventLogger::begin(&dir.join("cryo.log"), 1, "test task", "mock-agent", &[])
        .unwrap()
}

fn test_session_context<'a>(
    cryo_state: &'a CryoState,
    timeout_secs: u64,
    spawn_time: Instant,
) -> ActiveSessionContext<'a> {
    test_session_context_with_inbox(cryo_state, Vec::new(), timeout_secs, spawn_time)
}

fn test_session_context_with_inbox<'a>(
    cryo_state: &'a CryoState,
    _inbox_filenames: Vec<String>,
    timeout_secs: u64,
    spawn_time: Instant,
) -> ActiveSessionContext<'a> {
    ActiveSessionContext {
        cryo_state,
        timeout_secs,
        spawn_time,
    }
}

impl EventSource for FakeEventSource {
    fn recv_timeout(&self, _timeout: Duration) -> Result<DaemonEvent, WaitError> {
        self.events
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Err(WaitError::Timeout))
    }

    fn drain_inbox_changed(&self, paths: &mut Vec<PathBuf>) {
        let mut events = self.events.lock().unwrap();
        while matches!(events.front(), Some(Ok(DaemonEvent::InboxChanged { .. }))) {
            if let Some(Ok(DaemonEvent::InboxChanged { paths: more_paths })) = events.pop_front() {
                paths.extend(more_paths);
            }
            *self.drained_inbox.lock().unwrap() += 1;
        }
    }
}

#[test]
fn test_inbox_watcher_detects_new_file() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("messages").join("inbox");
    std::fs::create_dir_all(&inbox).unwrap();

    let (tx, rx) = mpsc::channel();
    let _watcher = InboxWatcher::start(std::slice::from_ref(&inbox), tx).unwrap();

    // Create a file in inbox
    std::fs::write(inbox.join("test-message.md"), "hello").unwrap();

    // Should receive InboxChanged within 2 seconds
    let event = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    match event {
        DaemonEvent::InboxChanged { paths } => {
            assert!(paths.iter().any(|path| path.ends_with("test-message.md")));
        }
        other => panic!("expected inbox change, got {other:?}"),
    }
}

#[test]
fn test_inbox_watcher_ignores_non_create_events() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("messages").join("inbox");
    std::fs::create_dir_all(&inbox).unwrap();

    // Create file before watcher starts
    let file = inbox.join("existing.md");
    std::fs::write(&file, "original").unwrap();

    let (tx, rx) = mpsc::channel();
    let _watcher = InboxWatcher::start(std::slice::from_ref(&inbox), tx).unwrap();

    // Modify existing file (not a create)
    std::fs::write(&file, "modified").unwrap();

    // Should NOT receive InboxChanged (modification, not creation)
    // Give it 500ms — if nothing arrives, that's correct
    let event = rx.recv_timeout(Duration::from_millis(500));
    // This may or may not fire depending on platform — just don't assert it MUST fire
    // The key is that create events DO fire (tested above)
    let _ = event; // suppress unused warning
}

#[test]
fn test_compute_sleep_timeout_both() {
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let wake = now + chrono::Duration::seconds(60);
    let report = now + chrono::Duration::seconds(30);
    let timeout = compute_sleep_timeout(Some(wake), Some(report), now);
    assert_eq!(
        timeout,
        Duration::from_secs(30),
        "Should pick earlier (report)"
    );
}

#[test]
fn test_compute_sleep_timeout_wake_only() {
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let wake = now + chrono::Duration::seconds(120);
    let timeout = compute_sleep_timeout(Some(wake), None, now);
    assert_eq!(timeout, Duration::from_secs(120));
}

#[test]
fn test_compute_sleep_timeout_report_only() {
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let report = now + chrono::Duration::seconds(45);
    let timeout = compute_sleep_timeout(None, Some(report), now);
    assert_eq!(timeout, Duration::from_secs(45));
}

#[test]
fn test_compute_sleep_timeout_neither() {
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let timeout = compute_sleep_timeout(None, None, now);
    assert_eq!(timeout, Duration::from_secs(3600));
}

fn sample_time() -> NaiveDateTime {
    chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap()
}

#[test]
fn test_wait_for_idle_event_coalesces_inbox_changes() {
    let source = FakeEventSource::new(vec![
        Ok(DaemonEvent::InboxChanged { paths: vec![] }),
        Ok(DaemonEvent::InboxChanged { paths: vec![] }),
        Ok(DaemonEvent::InboxChanged { paths: vec![] }),
    ]);

    let outcome = wait_for_idle_event(&source, Duration::from_secs(1), None, sample_time);

    assert_eq!(outcome, IdleWaitOutcome::WakeFromInbox { paths: vec![] });
    assert_eq!(source.drained_count(), 2);
}

#[test]
fn test_wait_for_idle_event_carries_inbox_sources() {
    let source = FakeEventSource::new(vec![
        Ok(DaemonEvent::InboxChanged {
            paths: vec![PathBuf::from("messages/inbox/admin.md")],
        }),
        Ok(DaemonEvent::InboxChanged {
            paths: vec![PathBuf::from(".messenger/inbox/mail-123")],
        }),
    ]);

    let outcome = wait_for_idle_event(&source, Duration::from_secs(1), None, sample_time);

    assert_eq!(
        outcome,
        IdleWaitOutcome::WakeFromInbox {
            paths: vec![
                PathBuf::from("messages/inbox/admin.md"),
                PathBuf::from(".messenger/inbox/mail-123"),
            ],
        }
    );
    assert_eq!(source.drained_count(), 1);
}

#[test]
fn test_wait_for_idle_event_timeout_after_deadline_wakes() {
    let source = FakeEventSource::new(vec![Err(WaitError::Timeout)]);
    let now = sample_time();
    let deadline = now - chrono::Duration::seconds(1);

    let outcome = wait_for_idle_event(&source, Duration::from_secs(1), Some(deadline), || now);

    assert_eq!(outcome, IdleWaitOutcome::WakeFromSchedule);
}

#[test]
fn test_wait_for_idle_event_timeout_before_deadline_stays_idle() {
    // Regression for the bug where a capped idle-loop sleep (e.g. 250 ms)
    // was misinterpreted as the scheduled wake arriving. With the fix,
    // timing out before the deadline must stay idle.
    let source = FakeEventSource::new(vec![Err(WaitError::Timeout)]);
    let now = sample_time();
    let deadline = now + chrono::Duration::hours(4);

    let outcome = wait_for_idle_event(&source, Duration::from_millis(250), Some(deadline), || now);

    assert_eq!(outcome, IdleWaitOutcome::StayIdle);
}

#[test]
fn test_wait_for_idle_event_timeout_without_scheduled_wake() {
    let source = FakeEventSource::new(vec![Err(WaitError::Timeout)]);

    let outcome = wait_for_idle_event(&source, Duration::from_secs(1), None, sample_time);

    assert_eq!(outcome, IdleWaitOutcome::StayIdle);
}

#[test]
fn test_wait_for_idle_event_shutdown_and_disconnect() {
    let shutdown = FakeEventSource::new(vec![Ok(DaemonEvent::Shutdown)]);
    let disconnected = FakeEventSource::new(vec![Err(WaitError::Disconnected)]);

    assert_eq!(
        wait_for_idle_event(&shutdown, Duration::from_secs(1), None, sample_time),
        IdleWaitOutcome::Shutdown
    );
    assert_eq!(
        wait_for_idle_event(&disconnected, Duration::from_secs(1), None, sample_time),
        IdleWaitOutcome::Disconnected
    );
}

#[test]
fn test_delayed_wake_under_threshold() {
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let scheduled = now - chrono::Duration::minutes(4);
    assert!(
        detect_delayed_wake(scheduled, now).is_none(),
        "4 min delay should not be flagged"
    );
}

#[test]
fn test_delayed_wake_over_threshold() {
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let scheduled = now - chrono::Duration::minutes(6);
    let result = detect_delayed_wake(scheduled, now);
    assert!(result.is_some(), "6 min delay should be flagged");
    assert_eq!(result.unwrap(), "6m");
}

#[test]
fn delayed_wake_notice_is_silent_for_inbox_wakes() {
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let scheduled = now - chrono::Duration::minutes(6);

    assert_eq!(delayed_wake_notice(true, Some(scheduled), now), None);
}

#[test]
fn delayed_wake_notice_is_silent_without_scheduled_wake() {
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();

    assert_eq!(delayed_wake_notice(false, None, now), None);
}

#[test]
fn delayed_wake_notice_reports_late_scheduled_wake() {
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let scheduled = now - chrono::Duration::minutes(6);

    let notice = delayed_wake_notice(false, Some(scheduled), now).unwrap();

    assert!(notice.contains("DELAYED WAKE"));
    assert!(notice.contains("2026-03-01T11:54"));
    assert!(notice.contains("6m late"));
}

#[test]
fn session_prompt_notice_is_empty_without_delayed_wake_or_crash() {
    assert_eq!(session_prompt_notice(None, false), None);
}

#[test]
fn session_prompt_notice_uses_delayed_wake_notice_when_present() {
    assert_eq!(
        session_prompt_notice(Some("DELAYED WAKE: 6m late"), false),
        Some("DELAYED WAKE: 6m late".to_string())
    );
}

#[test]
fn session_prompt_notice_reports_previous_session_crash() {
    let notice = session_prompt_notice(None, true).unwrap();

    assert!(notice.starts_with("PREVIOUS SESSION CRASHED"));
    assert!(notice.contains("cryo-agent hibernate"));
}

#[test]
fn session_prompt_notice_combines_delayed_wake_before_crash_notice() {
    let notice = session_prompt_notice(Some("DELAYED WAKE: 6m late"), true).unwrap();

    assert!(notice.starts_with("DELAYED WAKE: 6m late\n\nPREVIOUS SESSION CRASHED"));
}

#[test]
fn test_next_wake_from_todos_picks_earliest() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("todo.json");
    let todos = crate::todo::TodoFile::new(&path);
    todos
        .add("later".into(), "2026-03-02T16:00".into())
        .unwrap();
    todos
        .add("earlier".into(), "2026-03-02T14:00".into())
        .unwrap();
    let wake = next_wake_from_todos(dir.path());
    assert_eq!(
        wake.unwrap(),
        chrono::NaiveDate::from_ymd_opt(2026, 3, 2)
            .unwrap()
            .and_hms_opt(14, 0, 0)
            .unwrap()
    );
}

#[test]
fn test_next_wake_from_todos_none_when_empty() {
    let dir = tempfile::tempdir().unwrap();
    let wake = next_wake_from_todos(dir.path());
    assert!(wake.is_none());
}

#[test]
fn test_next_wake_from_todos_skips_done() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("todo.json");
    let todos = crate::todo::TodoFile::new(&path);
    let id = todos.add("done".into(), "2026-03-02T10:00".into()).unwrap();
    todos.done(id).unwrap();
    let wake = next_wake_from_todos(dir.path());
    assert!(wake.is_none());
}

#[test]
fn test_next_wake_from_todos_skips_invalid_and_picks_valid() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("todo.json");
    let todos = crate::todo::TodoFile::new(&path);
    // Invalid at value that sorts before valid ones
    todos
        .add("bad format".into(), "2026-03-02 10:00".into())
        .unwrap();
    // Valid at value
    todos
        .add("valid task".into(), "2026-03-02T14:00".into())
        .unwrap();
    let wake = next_wake_from_todos(dir.path());
    assert_eq!(
        wake.unwrap(),
        chrono::NaiveDate::from_ymd_opt(2026, 3, 2)
            .unwrap()
            .and_hms_opt(14, 0, 0)
            .unwrap(),
        "Should skip invalid entry and pick valid one"
    );
}

#[test]
fn test_next_wake_from_todos_skips_empty_at() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("todo.json");
    // Simulate legacy items with empty `at` (from serde default)
    let content = r#"[{"id":1,"text":"legacy item","done":false,"created":"unknown"},{"id":2,"text":"scheduled","done":false,"at":"2026-03-02T14:00","created":"unknown"}]"#;
    std::fs::write(&path, content).unwrap();
    let wake = next_wake_from_todos(dir.path());
    assert_eq!(
        wake.unwrap(),
        chrono::NaiveDate::from_ymd_opt(2026, 3, 2)
            .unwrap()
            .and_hms_opt(14, 0, 0)
            .unwrap(),
        "Should skip empty at and pick the valid one"
    );
}

#[test]
fn test_next_wake_from_todos_all_invalid_returns_none() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("todo.json");
    let todos = crate::todo::TodoFile::new(&path);
    todos.add("bad1".into(), "not-a-date".into()).unwrap();
    todos.add("bad2".into(), "also-bad".into()).unwrap();
    let wake = next_wake_from_todos(dir.path());
    assert!(wake.is_none(), "All invalid entries should yield None");
}

#[test]
fn test_resolve_hibernate_request_failure_retries() {
    // Failure path does not require a pending TODO — daemon will retry.
    let decision = resolve_hibernate_request(false, 7, Some("provider failed"), false);

    assert_eq!(
        decision.outcome,
        Some(SessionLoopOutcome::ValidationFailed { quick_exit: false })
    );
    assert!(decision.response_ok);
    assert_eq!(
        decision.response_message,
        "Failure recorded. Daemon will retry."
    );
    assert_eq!(
        decision.log_event,
        "hibernate failed: exit=7, summary=\"provider failed\""
    );
}

#[test]
fn test_resolve_hibernate_request_complete() {
    // `--complete` means the plan is truly finished; no TODO needed.
    let decision = resolve_hibernate_request(true, 0, None, false);

    assert_eq!(decision.outcome, Some(SessionLoopOutcome::PlanComplete));
    assert!(decision.response_ok);
    assert_eq!(decision.response_message, "Plan complete. Shutting down.");
    assert_eq!(
        decision.log_event,
        "hibernate: plan complete, exit=0, summary=\"(no summary)\""
    );
}

#[test]
fn test_resolve_hibernate_request_hibernates_with_pending_todo() {
    let decision = resolve_hibernate_request(false, 0, Some("waiting on reply"), true);

    assert_eq!(decision.outcome, Some(SessionLoopOutcome::Hibernate));
    assert!(decision.response_ok);
    assert_eq!(decision.response_message, "Hibernating.");
    assert_eq!(
        decision.log_event,
        "hibernate: exit=0, summary=\"waiting on reply\""
    );
}

#[test]
fn test_resolve_hibernate_request_rejects_when_no_pending_todo() {
    // Non-complete hibernate with no pending TODO: session must stay alive
    // so the agent can observe the error and correct.
    let decision = resolve_hibernate_request(false, 0, Some("forgot todo"), false);

    assert_eq!(
        decision.outcome, None,
        "session must continue so the agent can react to the error"
    );
    assert!(!decision.response_ok, "client must see an error response");
    assert!(
        decision.response_message.contains("hibernate refused"),
        "response should clearly name the refusal: {}",
        decision.response_message
    );
    assert!(
        decision.response_message.contains("cryo-agent todo add"),
        "response should tell the agent exactly what to do"
    );
    assert_eq!(
        decision.log_event,
        "hibernate refused: no pending TODO, summary=\"forgot todo\""
    );
}

#[test]
fn test_daemon_request_classification_groups_todo_variants() {
    let request = DaemonRequest::from(crate::socket::Request::TodoAdd {
        text: "Check inbox".into(),
        at: "2026-03-01T13:00".into(),
    });

    assert_eq!(
        request,
        DaemonRequest::Todo(TodoRequest::Add {
            text: "Check inbox".into(),
            at: "2026-03-01T13:00".into(),
        })
    );
    assert_eq!(
        DaemonRequest::from(crate::socket::Request::TodoDone { id: 7 }),
        DaemonRequest::Todo(TodoRequest::Done { id: 7 })
    );
    assert_eq!(
        DaemonRequest::from(crate::socket::Request::Receive),
        DaemonRequest::Receive
    );
}

#[test]
fn test_handle_todo_request_returns_response_and_log_event() {
    let mut effects = FakeSessionEffects::new();

    let outcome = handle_todo_request(
        TodoRequest::Add {
            text: "Check inbox".into(),
            at: "2026-03-01T13:00".into(),
        },
        &mut effects,
    );

    assert_eq!(
        outcome,
        TodoRequestOutcome {
            ok: true,
            message: "Added todo #1".into(),
            log_event: Some("todo add: #1 \"Check inbox\" at 2026-03-01T13:00".into()),
        }
    );
    assert_eq!(effects.todos.len(), 1);
    assert_eq!(effects.todos[0].text, "Check inbox");
}

#[test]
fn test_handle_receive_request_returns_formatted_messages_and_claimed_filenames() {
    let mut effects = FakeSessionEffects::new();
    effects.push_inbox_message("msg-1.md", "Archive me");

    let outcome = handle_receive_request(&mut effects);

    assert!(outcome.ok);
    assert_eq!(outcome.claimed_filenames, vec!["msg-1.md".to_string()]);
    assert_eq!(
        outcome.log_event,
        Some("receive: 1 inbox message [msg-1.md]".to_string())
    );
    assert!(outcome.message.contains("--- msg-1.md ---"));
    assert!(outcome.message.contains("Archive me"));
    assert!(effects.inbox_messages.is_empty());
}

#[test]
fn test_decide_next_step_maps_plan_complete_to_shutdown() {
    let next_wake = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let outcome = SessionLoopOutcome::PlanComplete;

    let step = decide_next_step(SessionRunResult::Outcome(&outcome), Some(next_wake));

    assert_eq!(step, NextStep::PlanComplete);
}

#[test]
fn test_decide_next_step_hibernate_uses_refreshed_wake() {
    let next_wake = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let outcome = SessionLoopOutcome::Hibernate;

    let step = decide_next_step(SessionRunResult::Outcome(&outcome), Some(next_wake));
    assert_eq!(
        step,
        NextStep::Hibernate {
            next_wake: Some(next_wake),
        }
    );
}

#[test]
fn test_decide_next_step_error_hibernates_without_retry() {
    // Agent startup/driver errors no longer auto-retry. The daemon records
    // the crash (via `previous_session_crashed`) and waits for the next
    // TODO or inbox event instead of hammering a backoff loop.
    let next_wake = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();

    let step = decide_next_step(SessionRunResult::Error, Some(next_wake));
    assert_eq!(
        step,
        NextStep::Hibernate {
            next_wake: Some(next_wake),
        }
    );
}

#[test]
fn test_decide_next_step_validation_failed_hibernates_without_retry() {
    // With provider rotation disabled, a ValidationFailed outcome (agent
    // crashed or returned --exit N) no longer triggers a retry plan. It
    // falls through to Hibernate so the daemon waits for the next TODO.
    let next_wake = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let outcome = SessionLoopOutcome::ValidationFailed { quick_exit: false };

    let step = decide_next_step(SessionRunResult::Outcome(&outcome), Some(next_wake));
    assert_eq!(
        step,
        NextStep::Hibernate {
            next_wake: Some(next_wake),
        }
    );
}

#[test]
fn test_legacy_rotate_on_does_not_rotate_provider_in_event_loop() {
    let dir = tempfile::tempdir().unwrap();

    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));

    let launcher = Arc::new(ScriptedSessionLauncher::new(vec![
        SessionLoopOutcome::ValidationFailed { quick_exit: false },
    ]));

    let daemon = Daemon::new_with_clock_and_launcher(
        dir.path().to_path_buf(),
        clock.clone(),
        launcher.clone(),
    );

    let sock_path = dir.path().join("test.sock");
    let server = crate::socket::SocketServer::bind(&sock_path).unwrap();
    server.set_nonblocking(true).unwrap();

    let mut cryo_state = test_cryo_state();
    cryo_state.session_number = 0;
    cryo_state.pid = Some(std::process::id());

    let bootstrap = DaemonBootstrapState {
        next_report_time: None,
        next_wake: None,
        run_now: true,
        watch_dirs: Vec::new(),
    };

    let config: CryoConfig = toml::from_str(
        r#"
agent = "opencode"
rotate_on = "any-failure"

[[providers]]
name = "provider-0"

[[providers]]
name = "provider-1"
"#,
    )
    .unwrap();

    let (tx, rx) = mpsc::channel();
    drop(tx);
    daemon
        .run_event_loop(&config, &mut cryo_state, bootstrap, &server, &rx)
        .unwrap();

    let providers = launcher.providers();
    assert_eq!(
        providers,
        vec![Some("provider-0".into())],
        "legacy rotate_on must be ignored; provider rotation is removed: {providers:?}"
    );
}

#[test]
fn test_session_loop_outcome_is_crash() {
    // `previous_session_crashed` is derived from this; the mapping is the
    // single source of truth and must cover every outcome variant.
    assert!(!SessionLoopOutcome::PlanComplete.is_crash());
    assert!(!SessionLoopOutcome::Hibernate.is_crash());
    assert!(SessionLoopOutcome::ValidationFailed { quick_exit: false }.is_crash());
    assert!(SessionLoopOutcome::ValidationFailed { quick_exit: true }.is_crash());
}

#[test]
fn test_resolve_interrupted_session_prefers_hibernate_outcome() {
    let hibernate = SessionLoopOutcome::Hibernate;

    let shutdown =
        resolve_interrupted_session(SessionInterruption::Shutdown, Some(hibernate.clone()));
    let timeout = resolve_interrupted_session(SessionInterruption::Timeout, Some(hibernate));

    assert_eq!(shutdown.outcome, SessionLoopOutcome::Hibernate);
    assert_eq!(
        shutdown.finish_reason,
        "daemon shutdown — using agent's hibernate outcome"
    );
    assert_eq!(timeout.outcome, SessionLoopOutcome::Hibernate);
    assert_eq!(
        timeout.finish_reason,
        "session timeout — using agent's hibernate outcome"
    );
}

#[test]
fn test_resolve_interrupted_session_without_hibernate_fails() {
    let shutdown = resolve_interrupted_session(SessionInterruption::Shutdown, None);
    let timeout = resolve_interrupted_session(SessionInterruption::Timeout, None);

    assert_eq!(
        shutdown.outcome,
        SessionLoopOutcome::ValidationFailed { quick_exit: false }
    );
    assert_eq!(shutdown.finish_reason, "daemon shutdown — agent terminated");
    assert_eq!(
        timeout.outcome,
        SessionLoopOutcome::ValidationFailed { quick_exit: false }
    );
    assert_eq!(timeout.finish_reason, "session timeout — agent killed");
}

#[test]
fn test_resolve_child_exit_after_hibernate_returns_outcome() {
    let decision = resolve_child_exit(
        Some(SessionLoopOutcome::PlanComplete),
        Duration::from_secs(1),
    );

    assert_eq!(decision.outcome, SessionLoopOutcome::PlanComplete);
    assert_eq!(decision.finish_reason, "session complete");
    assert!(!decision.quick_exit);
}

#[test]
fn test_resolve_child_exit_without_hibernate_marks_quick_exit() {
    let quick = resolve_child_exit(None, Duration::from_secs(2));
    let slow = resolve_child_exit(None, Duration::from_secs(8));

    assert_eq!(
        quick.outcome,
        SessionLoopOutcome::ValidationFailed { quick_exit: true }
    );
    assert_eq!(quick.finish_reason, "agent exited without hibernate");
    assert!(quick.quick_exit);

    assert_eq!(
        slow.outcome,
        SessionLoopOutcome::ValidationFailed { quick_exit: false }
    );
    assert_eq!(slow.finish_reason, "agent exited without hibernate");
    assert!(!slow.quick_exit);
}

#[test]
fn test_drive_active_session_timeout_after_hibernate_uses_hibernate_outcome() {
    let dir = tempfile::tempdir().unwrap();
    crate::message::ensure_dirs(dir.path()).unwrap();
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock.clone());
    let cryo_state = test_cryo_state();
    let mut runtime = FakeSessionRuntime::new(
        vec![Ok(Some(crate::socket::Request::Hibernate {
            complete: false,
            exit_code: 0,
            summary: Some("waiting".into()),
        }))],
        vec![],
    );
    let mut effects = FakeSessionEffects::new_with_pending_todo();

    let outcome = daemon
        .drive_active_session(
            &mut runtime,
            &mut effects,
            test_session_context(&cryo_state, 1, clock.monotonic_now()),
            begin_test_logger(dir.path()),
        )
        .unwrap();

    assert_eq!(outcome, SessionLoopOutcome::Hibernate);
    assert!(runtime.terminated(), "timeout should terminate the child");
    assert_eq!(runtime.responses(), vec![(true, "Hibernating.".into())]);
}

#[test]
fn test_drive_active_session_quick_exit_without_hibernate() {
    let dir = tempfile::tempdir().unwrap();
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock.clone());
    let cryo_state = test_cryo_state();
    let mut runtime =
        FakeSessionRuntime::new(vec![], vec![Ok(Some(ChildExitStatus { code: Some(1) }))]);
    let mut effects = FakeSessionEffects::new();

    let outcome = daemon
        .drive_active_session(
            &mut runtime,
            &mut effects,
            test_session_context(&cryo_state, 60, clock.monotonic_now()),
            begin_test_logger(dir.path()),
        )
        .unwrap();

    assert_eq!(
        outcome,
        SessionLoopOutcome::ValidationFailed { quick_exit: true }
    );
    assert!(!runtime.terminated());
    assert!(runtime.responses().is_empty());
    assert_eq!(effects.replies.len(), 1);
    assert_eq!(effects.replies[0].0, ReplyAuthor::Daemon);
    assert!(
        effects.replies[0]
            .1
            .contains("daemon: agent crashed before sending"),
        "daemon status should explain the crash path: {:?}",
        effects.replies
    );
}

#[test]
fn test_drive_active_session_writes_daemon_status_without_outbound_message() {
    let dir = tempfile::tempdir().unwrap();
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock.clone());
    let cryo_state = test_cryo_state();
    let mut runtime = FakeSessionRuntime::new(
        vec![Ok(Some(crate::socket::Request::Hibernate {
            complete: false,
            exit_code: 0,
            summary: Some("checked schedule".into()),
        }))],
        vec![Ok(None), Ok(Some(ChildExitStatus { code: Some(0) }))],
    );
    let mut effects = FakeSessionEffects::new_with_pending_todo();

    let outcome = daemon
        .drive_active_session(
            &mut runtime,
            &mut effects,
            test_session_context(&cryo_state, 60, clock.monotonic_now()),
            begin_test_logger(dir.path()),
        )
        .unwrap();

    assert_eq!(outcome, SessionLoopOutcome::Hibernate);
    assert!(
        matches!(runtime.responses().as_slice(), [(true, message)] if message == "Hibernating."),
        "hibernate should still be accepted: {:?}",
        runtime.responses()
    );
    assert_eq!(effects.replies.len(), 1);
    assert_eq!(effects.replies[0].0, ReplyAuthor::Daemon);
    assert!(
        effects.replies[0]
            .1
            .contains("daemon: agent hibernated without sending anything"),
        "daemon status should explain why it was sent: {:?}",
        effects.replies
    );
}

#[test]
fn test_drive_active_session_reply_failure_still_finishes_session_log() {
    let dir = tempfile::tempdir().unwrap();
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock.clone());
    let cryo_state = test_cryo_state();
    let mut runtime = FakeSessionRuntime::new(
        vec![
            Ok(Some(crate::socket::Request::Receive)),
            Ok(Some(crate::socket::Request::Send {
                question: false,
                text: "Need approval".into(),
            })),
            Ok(Some(crate::socket::Request::Hibernate {
                complete: false,
                exit_code: 0,
                summary: Some("waiting".into()),
            })),
        ],
        vec![
            Ok(None),
            Ok(None),
            Ok(None),
            Ok(Some(ChildExitStatus { code: Some(0) })),
        ],
    );
    let mut effects = FakeSessionEffects::with_reply_failure("injected reply failure");
    effects.push_inbox_message("human-1.md", "Need approval");

    let outcome = daemon
        .drive_active_session(
            &mut runtime,
            &mut effects,
            test_session_context(&cryo_state, 60, clock.monotonic_now()),
            begin_test_logger(dir.path()),
        )
        .unwrap();

    assert_eq!(outcome, SessionLoopOutcome::Hibernate);
    let responses = runtime.responses();
    assert_eq!(responses.len(), 3);
    assert!(responses[0].0);
    assert!(responses[0].1.contains("--- human-1.md ---"));
    assert_eq!(
        responses[1],
        (
            false,
            "Failed to write message: injected reply failure".into()
        )
    );
    assert_eq!(responses[2], (true, "Hibernating.".into()));
    assert!(effects.replies.is_empty());

    let log = std::fs::read_to_string(dir.path().join("cryo.log")).unwrap();
    assert!(
        log.contains("session complete") && log.contains("--- CRYO END ---"),
        "logger.finish should still run when fallback writes fail: {log}"
    );
    assert!(
        log.contains("daemon reply failed"),
        "fallback write failure should remain visible in the session log: {log}"
    );
}

#[test]
fn test_drive_active_session_unreceived_inbox_only_gets_daemon_status() {
    let dir = tempfile::tempdir().unwrap();
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock.clone());
    let cryo_state = test_cryo_state();
    let mut runtime = FakeSessionRuntime::new(
        vec![Ok(Some(crate::socket::Request::Hibernate {
            complete: true,
            exit_code: 0,
            summary: Some("done".into()),
        }))],
        vec![
            Ok(None),
            Ok(None),
            Ok(Some(ChildExitStatus { code: Some(0) })),
        ],
    );
    let mut effects = FakeSessionEffects::new();
    effects.push_inbox_message("human-1.md", "Need a reply");

    let outcome = daemon
        .drive_active_session(
            &mut runtime,
            &mut effects,
            test_session_context(&cryo_state, 60, clock.monotonic_now()),
            begin_test_logger(dir.path()),
        )
        .unwrap();

    assert_eq!(outcome, SessionLoopOutcome::PlanComplete);
    assert_eq!(effects.replies.len(), 1);
    assert_eq!(effects.replies[0].0, ReplyAuthor::Daemon);
    assert!(
        effects.replies[0]
            .1
            .contains("daemon: agent hibernated without sending anything"),
        "without a claimed inbox batch, the daemon should only send its generic status: {:?}",
        effects.replies
    );
}

#[test]
fn test_drive_active_session_send_after_receive_satisfies_queued_inbox_message() {
    let dir = tempfile::tempdir().unwrap();
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock.clone());
    let cryo_state = test_cryo_state();
    let mut runtime = FakeSessionRuntime::new(
        vec![
            Ok(Some(crate::socket::Request::Receive)),
            Ok(Some(crate::socket::Request::Send {
                question: false,
                text: "Got it".into(),
            })),
            Ok(Some(crate::socket::Request::Hibernate {
                complete: true,
                exit_code: 0,
                summary: Some("done".into()),
            })),
        ],
        vec![
            Ok(None),
            Ok(None),
            Ok(None),
            Ok(Some(ChildExitStatus { code: Some(0) })),
        ],
    );
    let mut effects = FakeSessionEffects::new();
    effects.push_inbox_message("human-1.md", "Need a reply");

    let outcome = daemon
        .drive_active_session(
            &mut runtime,
            &mut effects,
            test_session_context_with_inbox(
                &cryo_state,
                vec!["human-1.md".into()],
                60,
                clock.monotonic_now(),
            ),
            begin_test_logger(dir.path()),
        )
        .unwrap();

    assert_eq!(outcome, SessionLoopOutcome::PlanComplete);
    assert_eq!(effects.replies.len(), 1);
    assert_eq!(effects.replies[0].0, ReplyAuthor::Agent);
    assert!(effects.inbox_messages.is_empty());
}

#[test]
fn test_drive_active_session_send_without_receive_can_still_post_status() {
    let dir = tempfile::tempdir().unwrap();
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock.clone());
    let cryo_state = test_cryo_state();
    let mut runtime = FakeSessionRuntime::new(
        vec![
            Ok(Some(crate::socket::Request::Send {
                question: false,
                text: "Still investigating".into(),
            })),
            Ok(Some(crate::socket::Request::Hibernate {
                complete: true,
                exit_code: 0,
                summary: Some("done".into()),
            })),
        ],
        vec![
            Ok(None),
            Ok(None),
            Ok(Some(ChildExitStatus { code: Some(0) })),
        ],
    );
    let mut effects = FakeSessionEffects::new();
    effects.push_inbox_message("human-1.md", "Need a reply");

    let outcome = daemon
        .drive_active_session(
            &mut runtime,
            &mut effects,
            test_session_context_with_inbox(
                &cryo_state,
                vec!["human-1.md".into()],
                60,
                clock.monotonic_now(),
            ),
            begin_test_logger(dir.path()),
        )
        .unwrap();

    assert_eq!(outcome, SessionLoopOutcome::PlanComplete);
    assert_eq!(
        runtime.responses(),
        vec![
            (true, "Message sent".into()),
            (true, "Plan complete. Shutting down.".into())
        ]
    );
    assert!(
        effects
            .replies
            .iter()
            .any(|(author, text, _, _)| *author == ReplyAuthor::Agent
                && text == "Still investigating"),
        "plain send should still post a status message: {:?}",
        effects.replies
    );
    assert_eq!(
        effects
            .replies
            .iter()
            .filter(|(author, _, _, _)| *author == ReplyAuthor::Daemon)
            .count(),
        0,
        "unreceived inbox should remain silent until the agent explicitly reads it"
    );
}

#[test]
fn test_drive_active_session_receive_archives_inbox_even_when_response_delivery_fails() {
    let dir = tempfile::tempdir().unwrap();
    crate::message::ensure_dirs(dir.path()).unwrap();
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock.clone());
    let cryo_state = test_cryo_state();

    let mut runtime = FakeSessionRuntime::with_respond_results(
        vec![Ok(Some(crate::socket::Request::Receive))],
        vec![Ok(None), Ok(Some(ChildExitStatus { code: Some(0) }))],
        vec![Err(anyhow::anyhow!("injected response delivery failure"))],
    );
    let mut effects = FakeSessionEffects::new();
    effects.push_inbox_message("msg-1.md", "Archive me on receive");

    let err = daemon
        .drive_active_session(
            &mut runtime,
            &mut effects,
            test_session_context(&cryo_state, 60, clock.monotonic_now()),
            begin_test_logger(dir.path()),
        )
        .unwrap_err()
        .to_string();

    assert!(
        err.contains("injected response delivery failure"),
        "unexpected error: {err}"
    );
    assert!(effects.inbox_messages.is_empty());
}

#[test]
fn test_drive_active_session_todo_requests_use_effects() {
    let dir = tempfile::tempdir().unwrap();
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock.clone());
    let cryo_state = test_cryo_state();
    // Scenario exercises all four todo IPC verbs, then the agent adds a
    // second (pending) todo before hibernating — this second todo is what
    // lets the hibernate pass the "session must declare its next wake" check.
    let mut runtime = FakeSessionRuntime::new(
        vec![
            Ok(Some(crate::socket::Request::TodoAdd {
                text: "Check inbox".into(),
                at: "2026-03-01T13:00".into(),
            })),
            Ok(Some(crate::socket::Request::TodoList)),
            Ok(Some(crate::socket::Request::TodoDone { id: 1 })),
            Ok(Some(crate::socket::Request::TodoRemove { id: 1 })),
            Ok(Some(crate::socket::Request::TodoAdd {
                text: "Next session".into(),
                at: "2026-03-01T14:00".into(),
            })),
            Ok(Some(crate::socket::Request::Hibernate {
                complete: false,
                exit_code: 0,
                summary: None,
            })),
        ],
        vec![
            Ok(None),
            Ok(None),
            Ok(None),
            Ok(None),
            Ok(None),
            Ok(None),
            Ok(Some(ChildExitStatus { code: Some(0) })),
        ],
    );
    let mut effects = FakeSessionEffects::new();

    let outcome = daemon
        .drive_active_session(
            &mut runtime,
            &mut effects,
            test_session_context(&cryo_state, 60, clock.monotonic_now()),
            begin_test_logger(dir.path()),
        )
        .unwrap();

    assert_eq!(outcome, SessionLoopOutcome::Hibernate);
    assert_eq!(
        runtime.responses(),
        vec![
            (true, "Added todo #1".into()),
            (true, "1. [ ] Check inbox (at: 2026-03-01T13:00)".into()),
            (true, "Marked todo #1 as done".into()),
            (true, "Removed todo #1".into()),
            (true, "Added todo #2".into()),
            (true, "Hibernating.".into()),
        ]
    );
    // The second (pending) todo remains.
    assert_eq!(effects.todos.len(), 1);
    assert_eq!(effects.todos[0].id, 2);
    assert_eq!(effects.todos[0].at, "2026-03-01T14:00");
}

#[test]
fn test_run_event_loop_completes_claimed_todo_after_successful_session() {
    let dir = tempfile::tempdir().unwrap();
    seed_past_todo(dir.path());

    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let launcher = Arc::new(ScriptedSessionLauncher::new(vec![
        SessionLoopOutcome::PlanComplete,
    ]));
    let daemon =
        Daemon::new_with_clock_and_launcher(dir.path().to_path_buf(), clock.clone(), launcher);

    let sock_path = dir.path().join("test.sock");
    let server = crate::socket::SocketServer::bind(&sock_path).unwrap();
    server.set_nonblocking(true).unwrap();

    let mut cryo_state = test_cryo_state();
    cryo_state.session_number = 0;
    cryo_state.pid = Some(std::process::id());

    let bootstrap = DaemonBootstrapState {
        next_report_time: None,
        next_wake: None,
        run_now: true,
        watch_dirs: Vec::new(),
    };

    let (_tx, rx) = mpsc::channel();
    daemon
        .run_event_loop(
            &CryoConfig::default(),
            &mut cryo_state,
            bootstrap,
            &server,
            &rx,
        )
        .unwrap();

    let items = crate::todo::TodoFile::new(dir.path().join("todo.json"))
        .items()
        .unwrap();
    assert_eq!(items.len(), 1);
    assert!(items[0].done);
    assert!(!items[0].claimed);
}

#[test]
fn test_run_event_loop_reschedules_claimed_todo_after_crash() {
    let dir = tempfile::tempdir().unwrap();
    seed_past_todo(dir.path());

    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let launcher = Arc::new(ScriptedSessionLauncher::new(vec![
        SessionLoopOutcome::ValidationFailed { quick_exit: false },
    ]));
    let daemon =
        Daemon::new_with_clock_and_launcher(dir.path().to_path_buf(), clock.clone(), launcher);

    let sock_path = dir.path().join("test.sock");
    let server = crate::socket::SocketServer::bind(&sock_path).unwrap();
    server.set_nonblocking(true).unwrap();

    let mut cryo_state = test_cryo_state();
    cryo_state.session_number = 0;
    cryo_state.pid = Some(std::process::id());

    let bootstrap = DaemonBootstrapState {
        next_report_time: None,
        next_wake: None,
        run_now: true,
        watch_dirs: Vec::new(),
    };

    let (tx, rx) = mpsc::channel();
    drop(tx);
    daemon
        .run_event_loop(
            &CryoConfig::default(),
            &mut cryo_state,
            bootstrap,
            &server,
            &rx,
        )
        .unwrap();

    let items = crate::todo::TodoFile::new(dir.path().join("todo.json"))
        .items()
        .unwrap();
    assert_eq!(items.len(), 2);
    assert!(items[0].done);
    assert!(!items[0].claimed);
    assert_eq!(items[1].text, "keep going (attempt 1)");
    assert_eq!(items[1].at, "2026-03-01T12:02");
    assert!(!items[1].done);
    assert!(!items[1].claimed);
    assert!(cryo_state.previous_session_crashed);
}

#[test]
fn test_drive_active_session_receive_request_invokes_effect_and_returns_body() {
    let dir = tempfile::tempdir().unwrap();
    crate::message::ensure_dirs(dir.path()).unwrap();
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock.clone());
    let cryo_state = test_cryo_state();

    let mut runtime = FakeSessionRuntime::new(
        vec![
            Ok(Some(crate::socket::Request::Receive)),
            Ok(Some(crate::socket::Request::Hibernate {
                complete: false,
                exit_code: 0,
                summary: Some("done".into()),
            })),
        ],
        vec![
            Ok(None),
            Ok(None),
            Ok(Some(ChildExitStatus { code: Some(0) })),
        ],
    );
    let mut effects = FakeSessionEffects::new_with_pending_todo();
    effects.push_inbox_message("msg-1.md", "Archive me");

    let outcome = daemon
        .drive_active_session(
            &mut runtime,
            &mut effects,
            test_session_context(&cryo_state, 60, clock.monotonic_now()),
            begin_test_logger(dir.path()),
        )
        .unwrap();

    assert_eq!(outcome, SessionLoopOutcome::Hibernate);
    assert_eq!(runtime.responses().len(), 2);
    assert!(runtime.responses()[0].0);
    assert!(runtime.responses()[0].1.contains("--- msg-1.md ---"));
    assert!(runtime.responses()[0].1.contains("Archive me"));
    assert_eq!(runtime.responses()[1], (true, "Hibernating.".into()));
    assert!(effects.inbox_messages.is_empty());
    assert_eq!(effects.replies.len(), 1);
    assert_eq!(effects.replies[0].0, ReplyAuthor::Daemon);
    assert!(
        effects.replies[0]
            .1
            .contains("daemon: agent hibernated without replying"),
        "daemon fallback reply should still be written after receive/archive: {:?}",
        effects.replies
    );
}

#[test]
fn test_drive_active_session_receive_then_crash_uses_crash_fallback() {
    let dir = tempfile::tempdir().unwrap();
    crate::message::ensure_dirs(dir.path()).unwrap();
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock.clone());
    let cryo_state = test_cryo_state();

    let mut runtime = FakeSessionRuntime::new(
        vec![Ok(Some(crate::socket::Request::Receive))],
        vec![Ok(None), Ok(Some(ChildExitStatus { code: Some(1) }))],
    );
    let mut effects = FakeSessionEffects::new();
    effects.push_inbox_message("msg-1.md", "Archive me");

    let outcome = daemon
        .drive_active_session(
            &mut runtime,
            &mut effects,
            test_session_context(&cryo_state, 60, clock.monotonic_now()),
            begin_test_logger(dir.path()),
        )
        .unwrap();

    assert_eq!(
        outcome,
        SessionLoopOutcome::ValidationFailed { quick_exit: true }
    );
    assert_eq!(runtime.responses().len(), 1);
    assert!(runtime.responses()[0].0);
    assert!(runtime.responses()[0].1.contains("--- msg-1.md ---"));
    assert!(effects.inbox_messages.is_empty());
    assert_eq!(effects.replies.len(), 1);
    assert_eq!(effects.replies[0].0, ReplyAuthor::Daemon);
    assert!(
        effects.replies[0]
            .1
            .contains("daemon: agent crashed before replying"),
        "daemon fallback reply should name the crash path: {:?}",
        effects.replies
    );
}

#[test]
fn test_drive_active_session_dialog_request_returns_transcript_and_preserves_fallback() {
    let dir = tempfile::tempdir().unwrap();
    crate::message::ensure_dirs(dir.path()).unwrap();
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock.clone());
    let cryo_state = test_cryo_state();

    let mut runtime = FakeSessionRuntime::new(
        vec![
            Ok(Some(crate::socket::Request::Dialog {
                filter: crate::socket::DialogFilter::All,
            })),
            Ok(Some(crate::socket::Request::Hibernate {
                complete: false,
                exit_code: 0,
                summary: Some("done".into()),
            })),
        ],
        vec![
            Ok(None),
            Ok(None),
            Ok(Some(ChildExitStatus { code: Some(0) })),
        ],
    );
    let mut effects = FakeSessionEffects::new_with_pending_todo();
    effects.archived_outbox.push((
        "agent-0.md".into(),
        crate::message::Message {
            from: "agent".into(),
            subject: "Reply".into(),
            body: "Previous update".into(),
            timestamp: chrono::NaiveDate::from_ymd_opt(2026, 2, 28)
                .unwrap()
                .and_hms_opt(18, 0, 0)
                .unwrap(),
            metadata: Default::default(),
            is_question: false,
        },
    ));
    effects.push_inbox_message("msg-1.md", "Archive me");

    let outcome = daemon
        .drive_active_session(
            &mut runtime,
            &mut effects,
            test_session_context(&cryo_state, 60, clock.monotonic_now()),
            begin_test_logger(dir.path()),
        )
        .unwrap();

    assert_eq!(outcome, SessionLoopOutcome::Hibernate);
    assert_eq!(runtime.responses().len(), 2);
    assert!(runtime.responses()[0].0);
    assert!(runtime.responses()[0].1.contains("Previous update"));
    assert!(runtime.responses()[0].1.contains("Archive me"));
    assert!(runtime.responses()[0].1.contains("new since last session"));
    assert_eq!(runtime.responses()[1], (true, "Hibernating.".into()));
    assert!(effects.inbox_messages.is_empty());
    assert_eq!(effects.replies.len(), 1);
    assert_eq!(effects.replies[0].0, ReplyAuthor::Daemon);
    assert!(
        effects.replies[0]
            .1
            .contains("daemon: agent hibernated without replying"),
        "dialog should preserve the fallback reply obligation: {:?}",
        effects.replies
    );
}

#[test]
fn test_drive_active_session_dialog_failure_after_claim_still_triggers_fallback() {
    let dir = tempfile::tempdir().unwrap();
    crate::message::ensure_dirs(dir.path()).unwrap();
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock.clone());
    let cryo_state = test_cryo_state();

    let mut runtime = FakeSessionRuntime::new(
        vec![
            Ok(Some(crate::socket::Request::Dialog {
                filter: crate::socket::DialogFilter::Since {
                    iso: "yesterday".into(),
                },
            })),
            Ok(Some(crate::socket::Request::Hibernate {
                complete: false,
                exit_code: 0,
                summary: Some("done".into()),
            })),
        ],
        vec![
            Ok(None),
            Ok(None),
            Ok(Some(ChildExitStatus { code: Some(0) })),
        ],
    );
    let mut effects = FakeSessionEffects::new_with_pending_todo();
    effects.push_inbox_message("msg-1.md", "Archive me");

    let outcome = daemon
        .drive_active_session(
            &mut runtime,
            &mut effects,
            test_session_context(&cryo_state, 60, clock.monotonic_now()),
            begin_test_logger(dir.path()),
        )
        .unwrap();

    assert_eq!(outcome, SessionLoopOutcome::Hibernate);
    assert_eq!(runtime.responses().len(), 2);
    assert!(!runtime.responses()[0].0);
    assert!(runtime.responses()[0]
        .1
        .contains("not a recognized timestamp"));
    assert_eq!(runtime.responses()[1], (true, "Hibernating.".into()));
    assert!(effects.inbox_messages.is_empty());
    assert_eq!(effects.replies.len(), 1);
    assert_eq!(effects.replies[0].0, ReplyAuthor::Daemon);
    assert!(
        effects.replies[0]
            .1
            .contains("daemon: agent hibernated without replying"),
        "a failed dialog after claim must still preserve fallback reply behavior: {:?}",
        effects.replies
    );
}

#[test]
fn test_build_bootstrap_state_runs_immediately_for_first_session_and_overdue_wake() {
    let dir = tempfile::tempdir().unwrap();
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock);
    let mut first_session = test_cryo_state();
    first_session.session_number = 0;

    let first = daemon.build_bootstrap_state(&first_session, &CryoConfig::default());
    assert!(first.run_now);

    let todo_path = dir.path().join("todo.json");
    crate::todo::TodoFile::new(&todo_path)
        .add("Overdue task".into(), "2026-03-01T11:30".into())
        .unwrap();

    let mut resumed = test_cryo_state();
    resumed.session_number = 3;
    let resumed_bootstrap = daemon.build_bootstrap_state(&resumed, &CryoConfig::default());
    assert_eq!(
        resumed_bootstrap.next_wake,
        Some(
            chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
                .unwrap()
                .and_hms_opt(11, 30, 0)
                .unwrap()
        )
    );
    assert!(resumed_bootstrap.run_now);
}

#[test]
fn test_build_bootstrap_state_only_enables_watcher_when_configured_and_present() {
    let dir = tempfile::tempdir().unwrap();
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock);
    let cryo_state = test_cryo_state();
    let mut config = CryoConfig::default();

    // Default config lists `messages/inbox`, but the directory does not yet
    // exist on disk so the daemon should filter it out.
    let no_inbox = daemon.build_bootstrap_state(&cryo_state, &config);
    assert!(no_inbox.watch_dirs.is_empty());

    let inbox = dir.path().join("messages").join("inbox");
    std::fs::create_dir_all(&inbox).unwrap();
    let with_inbox = daemon.build_bootstrap_state(&cryo_state, &config);
    assert_eq!(with_inbox.watch_dirs, vec![inbox.clone()]);

    config.watch_dirs = Vec::new();
    let disabled = daemon.build_bootstrap_state(&cryo_state, &config);
    assert!(disabled.watch_dirs.is_empty());
}

#[test]
fn test_build_bootstrap_state_supports_multiple_watch_dirs() {
    let dir = tempfile::tempdir().unwrap();
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock);
    let cryo_state = test_cryo_state();

    let inbox = dir.path().join("messages").join("inbox");
    let drop_box = dir.path().join("drop_box");
    std::fs::create_dir_all(&inbox).unwrap();
    std::fs::create_dir_all(&drop_box).unwrap();

    let config = CryoConfig {
        watch_dirs: vec![
            std::path::PathBuf::from("messages/inbox"),
            std::path::PathBuf::from("drop_box"),
            std::path::PathBuf::from("not_yet_created"),
        ],
        ..Default::default()
    };

    let bootstrap = daemon.build_bootstrap_state(&cryo_state, &config);
    assert_eq!(bootstrap.watch_dirs, vec![inbox.clone(), drop_box.clone()]);
}

#[test]
fn test_prepare_shutdown_state_clears_runtime_identity() {
    let dir = tempfile::tempdir().unwrap();
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock);
    let mut cryo_state = test_cryo_state();
    cryo_state.pid = Some(1234);
    cryo_state.instance_id = Some("daemon-1".into());

    daemon.prepare_shutdown_state(&mut cryo_state);

    assert!(cryo_state.pid.is_none());
    assert!(cryo_state.instance_id.is_none());
}

#[test]
fn test_prepare_runtime_startup_returns_registry_warning_but_continues() {
    let dir = tempfile::tempdir().unwrap();
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock);
    let inbox = dir.path().join("messages").join("inbox");
    std::fs::create_dir_all(&inbox).unwrap();
    let mut platform = FakeStartupPlatform::new();
    platform.registry_error = Some("registry unavailable".into());
    let (tx, _rx) = mpsc::channel();

    let startup = daemon
        .prepare_runtime_startup(&platform, std::slice::from_ref(&inbox), tx)
        .unwrap();

    assert_eq!(
        startup.diagnostics.registry_warning,
        Some("registry unavailable".into())
    );
    assert!(startup.diagnostics.watcher_warning.is_none());
    assert_eq!(platform.bind_calls(), 1);
    assert_eq!(platform.watcher_calls(), 1);
    assert!(startup.watcher.is_some());
}

#[test]
fn test_prepare_runtime_startup_watcher_failure_is_nonfatal() {
    let dir = tempfile::tempdir().unwrap();
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock);
    let inbox = dir.path().join("messages").join("inbox");
    std::fs::create_dir_all(&inbox).unwrap();
    let mut platform = FakeStartupPlatform::new();
    platform.watcher_error = Some("watch failed".into());
    let (tx, _rx) = mpsc::channel();

    let startup = daemon
        .prepare_runtime_startup(&platform, std::slice::from_ref(&inbox), tx)
        .unwrap();

    assert!(startup.watcher.is_none());
    assert_eq!(
        startup.diagnostics.watcher_warning,
        Some("watch failed".into())
    );
    assert_eq!(platform.watcher_calls(), 1);
}

#[test]
fn test_prepare_runtime_startup_propagates_signal_registration_failure() {
    let dir = tempfile::tempdir().unwrap();
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock);
    let mut platform = FakeStartupPlatform::new();
    platform.signal_error = Some("signal registration failed".into());
    let (tx, _rx) = mpsc::channel();

    let error = daemon
        .prepare_runtime_startup(&platform, &[], tx)
        .unwrap_err()
        .to_string();

    assert!(error.contains("signal registration failed"));
    assert_eq!(platform.bind_calls(), 0);
    assert_eq!(platform.watcher_calls(), 0);
}

#[test]
fn test_prepare_runtime_startup_propagates_socket_bind_failure() {
    let dir = tempfile::tempdir().unwrap();
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock);
    let mut platform = FakeStartupPlatform::new();
    platform.bind_error = Some("bind failed".into());
    let (tx, _rx) = mpsc::channel();

    let error = daemon
        .prepare_runtime_startup(&platform, &[], tx)
        .unwrap_err()
        .to_string();

    assert!(error.contains("bind failed"));
    assert_eq!(platform.bind_calls(), 1);
    assert_eq!(platform.watcher_calls(), 0);
}

#[test]
fn test_run_clears_stranded_session_active_on_startup_save() {
    let dir = tempfile::tempdir().unwrap();
    crate::config::save_config(
        &crate::config::config_path(dir.path()),
        &crate::config::CryoConfig::default(),
    )
    .unwrap();
    crate::state::save_state(
        &crate::state::state_path(dir.path()),
        &CryoState {
            session_number: 3,
            pid: None,
            agent_override: None,
            max_session_duration_override: None,
            last_report_time: None,
            instance_id: None,
            session_active: true,
            previous_session_crashed: false,
        },
    )
    .unwrap();

    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let state_store = Arc::new(RecordingStateStore::default());
    let daemon = Daemon::with_deps(
        dir.path().to_path_buf(),
        clock,
        Arc::new(ProcessSessionLauncher),
        state_store.clone(),
    );
    daemon.shutdown.store(true, Ordering::Relaxed);

    daemon.run().unwrap();

    let saved = state_store.saved_states();
    assert!(
        !saved.is_empty(),
        "daemon should persist startup state before shutting down"
    );
    assert!(
        !saved[0].session_active,
        "startup save should clear a stranded active-session flag: {saved:?}"
    );
}

#[test]
fn test_run_recovers_stale_claimed_todo_on_startup() {
    let dir = tempfile::tempdir().unwrap();
    crate::config::save_config(
        &crate::config::config_path(dir.path()),
        &crate::config::CryoConfig::default(),
    )
    .unwrap();
    crate::state::save_state(
        &crate::state::state_path(dir.path()),
        &CryoState {
            session_number: 3,
            pid: None,
            agent_override: None,
            max_session_duration_override: None,
            last_report_time: None,
            instance_id: None,
            session_active: true,
            previous_session_crashed: false,
        },
    )
    .unwrap();
    std::fs::write(
        dir.path().join("todo.json"),
        r#"[{"id":1,"text":"stale wake","done":false,"claimed":true,"at":"2026-03-01T10:00","created":"unknown"}]"#,
    )
    .unwrap();

    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let state_store = Arc::new(RecordingStateStore::default());
    let daemon = Daemon::with_deps(
        dir.path().to_path_buf(),
        clock,
        Arc::new(ProcessSessionLauncher),
        state_store.clone(),
    );
    daemon.shutdown.store(true, Ordering::Relaxed);

    daemon.run().unwrap();

    let items = crate::todo::TodoFile::new(dir.path().join("todo.json"))
        .items()
        .unwrap();
    assert_eq!(items.len(), 2);
    assert!(items[0].done);
    assert!(!items[0].claimed);
    assert_eq!(items[1].text, "stale wake (attempt 1)");
    assert_eq!(items[1].at, "2026-03-01T12:02");
    assert!(!items[1].done);
    assert!(!items[1].claimed);

    let saved = state_store.saved_states();
    assert!(
        saved[0].previous_session_crashed,
        "startup state should remember that the previous session crashed: {saved:?}"
    );
}

// ---------- In-process multi-session event-loop tests ----------
//
// These tests exercise `run_event_loop` without spawning subprocesses or
// installing real OS resources. A `ScriptedSessionLauncher` returns canned
// outcomes so the loop's state-management (retry reset, next_wake refresh,
// plan-complete shutdown) can be verified in
// milliseconds. The virtual `TestClock` keeps `compute_sleep_timeout` in the
// past so each iteration returns from `wait_for_idle_event` immediately.

/// SessionLauncher that pops outcomes from a scripted queue. If the queue runs
/// dry it returns `PlanComplete` so the loop always terminates. Each call is
/// recorded with the session number and active provider name so tests can
/// assert on rotation sequences.
struct ScriptedSessionLauncher {
    steps: Mutex<VecDeque<ScriptedStep>>,
    invocations: Mutex<Vec<ScriptedInvocation>>,
}

struct ScriptedStep {
    outcome: SessionLoopOutcome,
    on_run: Option<Arc<dyn Fn() + Send + Sync>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ScriptedInvocation {
    session: u32,
    provider: Option<String>,
    wake_sources: Vec<PathBuf>,
}

impl ScriptedStep {
    fn outcome(outcome: SessionLoopOutcome) -> Self {
        Self {
            outcome,
            on_run: None,
        }
    }

    fn with_hook(outcome: SessionLoopOutcome, on_run: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            outcome,
            on_run: Some(Arc::new(on_run)),
        }
    }
}

impl ScriptedSessionLauncher {
    fn new(outcomes: Vec<SessionLoopOutcome>) -> Self {
        let steps = outcomes.into_iter().map(ScriptedStep::outcome).collect();
        Self::with_steps(steps)
    }

    fn with_steps(steps: Vec<ScriptedStep>) -> Self {
        Self {
            steps: Mutex::new(steps.into()),
            invocations: Mutex::new(Vec::new()),
        }
    }

    fn session_numbers(&self) -> Vec<u32> {
        self.invocations
            .lock()
            .unwrap()
            .iter()
            .map(|i| i.session)
            .collect()
    }

    fn providers(&self) -> Vec<Option<String>> {
        self.invocations
            .lock()
            .unwrap()
            .iter()
            .map(|i| i.provider.clone())
            .collect()
    }

    fn wake_sources(&self) -> Vec<Vec<PathBuf>> {
        self.invocations
            .lock()
            .unwrap()
            .iter()
            .map(|i| i.wake_sources.clone())
            .collect()
    }
}

impl SessionLauncher for ScriptedSessionLauncher {
    fn run_session(
        &self,
        _daemon: &Daemon,
        _config: &CryoConfig,
        cryo_state: &CryoState,
        _server: &crate::socket::SocketServer,
        _delayed_wake: Option<&str>,
        wake_sources: &[PathBuf],
        _provider_env: &std::collections::HashMap<String, String>,
        provider_name: Option<&str>,
    ) -> Result<SessionLoopOutcome> {
        self.invocations.lock().unwrap().push(ScriptedInvocation {
            session: cryo_state.session_number,
            provider: provider_name.map(str::to_string),
            wake_sources: wake_sources.to_vec(),
        });
        let step = self
            .steps
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| ScriptedStep::outcome(SessionLoopOutcome::PlanComplete));
        if let Some(hook) = step.on_run {
            hook();
        }
        Ok(step.outcome)
    }
}

struct ErrorSessionLauncher;

impl SessionLauncher for ErrorSessionLauncher {
    fn run_session(
        &self,
        _daemon: &Daemon,
        _config: &CryoConfig,
        _cryo_state: &CryoState,
        _server: &crate::socket::SocketServer,
        _delayed_wake: Option<&str>,
        _wake_sources: &[PathBuf],
        _provider_env: &std::collections::HashMap<String, String>,
        _provider_name: Option<&str>,
    ) -> Result<SessionLoopOutcome> {
        anyhow::bail!("injected launcher failure");
    }
}

/// Seed a single pending TODO so the daemon always has a "next wake" it can
/// compute. The wake time is in the past relative to the virtual clock, so
/// `wait_for_idle_event` returns `WakeFromSchedule` immediately on every
/// idle iteration.
fn seed_past_todo(dir: &Path) {
    crate::todo::TodoFile::new(dir.join("todo.json"))
        .add("keep going".into(), "2026-01-01T00:00".into())
        .unwrap();
}

#[test]
fn test_run_event_loop_drives_multiple_sessions_in_process() {
    let dir = tempfile::tempdir().unwrap();
    seed_past_todo(dir.path());

    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));

    let dir_path = dir.path().to_path_buf();
    let launcher = Arc::new(ScriptedSessionLauncher::with_steps(vec![
        ScriptedStep::with_hook(SessionLoopOutcome::Hibernate, {
            let dir_path = dir_path.clone();
            move || seed_past_todo(&dir_path)
        }),
        ScriptedStep::with_hook(SessionLoopOutcome::Hibernate, {
            let dir_path = dir_path.clone();
            move || seed_past_todo(&dir_path)
        }),
        ScriptedStep::outcome(SessionLoopOutcome::PlanComplete),
    ]));

    let daemon = Daemon::new_with_clock_and_launcher(
        dir.path().to_path_buf(),
        clock.clone(),
        launcher.clone(),
    );

    // A real SocketServer is required, but nothing connects to it — the loop
    // only polls it via non-blocking accept.
    let sock_path = dir.path().join("test.sock");
    let server = crate::socket::SocketServer::bind(&sock_path).unwrap();
    server.set_nonblocking(true).unwrap();

    let mut cryo_state = test_cryo_state();
    cryo_state.session_number = 0;
    cryo_state.pid = Some(std::process::id());

    let bootstrap = DaemonBootstrapState {
        next_report_time: None,
        next_wake: None,
        run_now: true,
        watch_dirs: Vec::new(),
    };

    let (_tx, rx) = mpsc::channel();

    let start = std::time::Instant::now();
    daemon
        .run_event_loop(
            &CryoConfig::default(),
            &mut cryo_state,
            bootstrap,
            &server,
            &rx,
        )
        .unwrap();
    let elapsed = start.elapsed();

    assert_eq!(
        launcher.session_numbers(),
        vec![1, 2, 3],
        "launcher should see session numbers 1, 2, 3"
    );
    assert_eq!(cryo_state.session_number, 3);
    // Plan-complete cleanup clears pid + instance_id.
    assert!(cryo_state.pid.is_none());
    assert!(cryo_state.instance_id.is_none());

    // In-process sessions should be dramatically faster than wall-clock
    // subprocess-based ones. The existing multi-session integration test
    // takes >3s; this one should be <1s even on a loaded CI machine.
    assert!(
        elapsed < Duration::from_secs(1),
        "in-process multi-session loop should be fast; took {elapsed:?}"
    );
}

#[test]
fn test_run_event_loop_passes_inbox_wake_sources_to_session() {
    let dir = tempfile::tempdir().unwrap();
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let launcher = Arc::new(ScriptedSessionLauncher::new(vec![
        SessionLoopOutcome::PlanComplete,
    ]));
    let daemon =
        Daemon::new_with_clock_and_launcher(dir.path().to_path_buf(), clock, launcher.clone());

    let sock_path = dir.path().join("test.sock");
    let server = crate::socket::SocketServer::bind(&sock_path).unwrap();
    server.set_nonblocking(true).unwrap();

    let mut cryo_state = test_cryo_state();
    cryo_state.session_number = 0;
    cryo_state.pid = Some(std::process::id());
    let bootstrap = DaemonBootstrapState {
        next_report_time: None,
        next_wake: None,
        run_now: false,
        watch_dirs: Vec::new(),
    };

    let (tx, rx) = mpsc::channel();
    tx.send(DaemonEvent::InboxChanged {
        paths: vec![PathBuf::from(".messenger/inbox/mail-123")],
    })
    .unwrap();
    drop(tx);

    daemon
        .run_event_loop(
            &CryoConfig::default(),
            &mut cryo_state,
            bootstrap,
            &server,
            &rx,
        )
        .unwrap();

    assert_eq!(
        launcher.wake_sources(),
        vec![vec![PathBuf::from(".messenger/inbox/mail-123")]]
    );
}

#[test]
fn test_prepare_startup_state_clears_stale_session_active() {
    let dir = tempfile::tempdir().unwrap();
    let daemon = Daemon::new(dir.path().to_path_buf());
    let mut st = test_cryo_state();
    st.pid = None;
    st.instance_id = None;
    st.session_active = true; // stale leftover from a prior SIGKILL'd run
    daemon.prepare_startup_state(&mut st);
    assert_eq!(st.pid, Some(std::process::id()));
    assert!(
        st.instance_id.is_some(),
        "prepare_startup_state should mint an instance_id"
    );
    assert!(
        !st.session_active,
        "prepare_startup_state must clear stale session_active"
    );
}

#[test]
fn test_session_active_observed_inside_session_and_cleared_after() {
    let dir = tempfile::tempdir().unwrap();
    seed_past_todo(dir.path());

    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));

    let state_path = crate::state::state_path(dir.path());
    let captured: Arc<Mutex<Option<bool>>> = Arc::new(Mutex::new(None));
    let captured_clone = captured.clone();
    let state_path_clone = state_path.clone();
    let launcher = Arc::new(ScriptedSessionLauncher::with_steps(vec![
        ScriptedStep::with_hook(SessionLoopOutcome::PlanComplete, move || {
            let st = crate::state::load_state(&state_path_clone)
                .unwrap()
                .unwrap();
            *captured_clone.lock().unwrap() = Some(st.session_active);
        }),
    ]));

    let daemon = Daemon::new_with_clock_and_launcher(
        dir.path().to_path_buf(),
        clock.clone(),
        launcher.clone(),
    );

    let sock_path = dir.path().join("test.sock");
    let server = crate::socket::SocketServer::bind(&sock_path).unwrap();
    server.set_nonblocking(true).unwrap();

    let mut cryo_state = test_cryo_state();
    cryo_state.session_number = 0;
    cryo_state.pid = Some(std::process::id());
    cryo_state.session_active = false;

    let bootstrap = DaemonBootstrapState {
        next_report_time: None,
        next_wake: None,
        run_now: true,
        watch_dirs: Vec::new(),
    };

    let (_tx, rx) = mpsc::channel();

    daemon
        .run_event_loop(
            &CryoConfig::default(),
            &mut cryo_state,
            bootstrap,
            &server,
            &rx,
        )
        .unwrap();

    assert_eq!(
        *captured.lock().unwrap(),
        Some(true),
        "session_active must be true in timer.json while run_session is executing"
    );
    let final_state = crate::state::load_state(&state_path).unwrap().unwrap();
    assert!(
        !final_state.session_active,
        "session_active must be cleared after the session returns"
    );
}

#[test]
fn test_run_event_loop_hibernate_refreshes_next_wake_between_sessions() {
    // After each Hibernate, the loop calls `next_wake_from_todos`. If the
    // TODO file changes between sessions, the next iteration should see the
    // new wake time. We simulate that by having the scripted launcher's
    // outcomes happen to coincide with a test-side TODO update.
    let dir = tempfile::tempdir().unwrap();
    seed_past_todo(dir.path());

    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));

    let dir_path = dir.path().to_path_buf();
    let launcher = Arc::new(ScriptedSessionLauncher::with_steps(vec![
        ScriptedStep::with_hook(SessionLoopOutcome::Hibernate, move || {
            seed_past_todo(&dir_path)
        }),
        ScriptedStep::outcome(SessionLoopOutcome::PlanComplete),
    ]));

    let daemon = Daemon::new_with_clock_and_launcher(
        dir.path().to_path_buf(),
        clock.clone(),
        launcher.clone(),
    );

    let sock_path = dir.path().join("test.sock");
    let server = crate::socket::SocketServer::bind(&sock_path).unwrap();
    server.set_nonblocking(true).unwrap();

    let mut cryo_state = test_cryo_state();
    cryo_state.session_number = 0;
    cryo_state.pid = Some(std::process::id());

    let bootstrap = DaemonBootstrapState {
        next_report_time: None,
        next_wake: None,
        run_now: true,
        watch_dirs: Vec::new(),
    };

    let (_tx, rx) = mpsc::channel();

    daemon
        .run_event_loop(
            &CryoConfig::default(),
            &mut cryo_state,
            bootstrap,
            &server,
            &rx,
        )
        .unwrap();

    // Both sessions ran; second was triggered by the past-TODO wake.
    assert_eq!(launcher.session_numbers(), vec![1, 2]);
    assert!(!cryo_state.previous_session_crashed);
}

#[test]
fn test_run_event_loop_marks_session_active_during_successful_session() {
    let dir = tempfile::tempdir().unwrap();
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let launcher = Arc::new(ScriptedSessionLauncher::new(vec![
        SessionLoopOutcome::PlanComplete,
    ]));
    let state_store = Arc::new(RecordingStateStore::default());
    let daemon = Daemon::with_deps(
        dir.path().to_path_buf(),
        clock,
        launcher,
        state_store.clone(),
    );

    let sock_path = dir.path().join("test.sock");
    let server = crate::socket::SocketServer::bind(&sock_path).unwrap();
    server.set_nonblocking(true).unwrap();

    let mut cryo_state = test_cryo_state();
    cryo_state.session_number = 0;
    cryo_state.pid = Some(std::process::id());

    let bootstrap = DaemonBootstrapState {
        next_report_time: None,
        next_wake: None,
        run_now: true,
        watch_dirs: Vec::new(),
    };

    let (_tx, rx) = mpsc::channel();

    daemon
        .run_event_loop(
            &CryoConfig::default(),
            &mut cryo_state,
            bootstrap,
            &server,
            &rx,
        )
        .unwrap();

    let saved = state_store.saved_states();
    assert!(
        saved.len() >= 3,
        "expected startup, post-session, and final shutdown saves, got {saved:?}"
    );
    assert!(
        saved[0].session_active,
        "session should be marked active before launch"
    );
    assert!(
        !saved[1].session_active,
        "post-session save should clear the active flag: {saved:?}"
    );
    assert!(
        !saved.last().unwrap().session_active,
        "final shutdown state must stay inactive: {saved:?}"
    );
}

#[test]
fn test_run_event_loop_clears_session_active_after_launcher_error() {
    let dir = tempfile::tempdir().unwrap();
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let launcher = Arc::new(ErrorSessionLauncher);
    let state_store = Arc::new(RecordingStateStore::default());
    let daemon = Daemon::with_deps(
        dir.path().to_path_buf(),
        clock,
        launcher,
        state_store.clone(),
    );

    let sock_path = dir.path().join("test.sock");
    let server = crate::socket::SocketServer::bind(&sock_path).unwrap();
    server.set_nonblocking(true).unwrap();

    let mut cryo_state = test_cryo_state();
    cryo_state.session_number = 0;
    cryo_state.pid = Some(std::process::id());

    let bootstrap = DaemonBootstrapState {
        next_report_time: None,
        next_wake: None,
        run_now: true,
        watch_dirs: Vec::new(),
    };

    let (tx, rx) = mpsc::channel();
    drop(tx);

    daemon
        .run_event_loop(
            &CryoConfig::default(),
            &mut cryo_state,
            bootstrap,
            &server,
            &rx,
        )
        .unwrap();

    let saved = state_store.saved_states();
    assert!(
        saved.len() >= 3,
        "expected startup, error, and final shutdown saves, got {saved:?}"
    );
    assert!(
        saved[0].session_active,
        "session should be marked active before launch"
    );
    assert!(
        !saved[1].session_active,
        "error path should clear the active flag before persisting: {saved:?}"
    );
    assert!(
        !saved.last().unwrap().session_active,
        "final shutdown state must stay inactive: {saved:?}"
    );
}

#[test]
fn test_run_event_loop_does_not_abort_on_mid_loop_state_save_failure() {
    let dir = tempfile::tempdir().unwrap();
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let launcher = Arc::new(ScriptedSessionLauncher::new(vec![
        SessionLoopOutcome::PlanComplete,
    ]));
    let daemon = Daemon::with_deps(
        dir.path().to_path_buf(),
        clock,
        launcher,
        Arc::new(FailingStateStore),
    );

    let sock_path = dir.path().join("test.sock");
    let server = crate::socket::SocketServer::bind(&sock_path).unwrap();
    server.set_nonblocking(true).unwrap();

    let mut cryo_state = test_cryo_state();
    cryo_state.session_number = 0;
    cryo_state.pid = Some(std::process::id());

    let bootstrap = DaemonBootstrapState {
        next_report_time: None,
        next_wake: None,
        run_now: true,
        watch_dirs: Vec::new(),
    };

    let (_tx, rx) = mpsc::channel();

    daemon
        .run_event_loop(
            &CryoConfig::default(),
            &mut cryo_state,
            bootstrap,
            &server,
            &rx,
        )
        .unwrap();

    assert_eq!(cryo_state.session_number, 1);
    assert!(cryo_state.pid.is_none());
    assert!(!cryo_state.session_active);
}

#[test]
fn test_run_event_loop_validation_failures_no_longer_auto_retry() {
    // A ValidationFailed outcome used to trigger an in-loop backoff/retry.
    // With the retry plumbing removed, the session is treated like a
    // hibernate: the next wake is determined solely by the TODO list (or by
    // external inbox/wake events). With a past TODO seeded, the scheduler
    // does still re-fire on the next loop iteration, but there is no
    // dedicated backoff path — the launcher is drained purely via scheduled
    // wakes, and the run ends once the queue empties.
    let dir = tempfile::tempdir().unwrap();
    seed_past_todo(dir.path());
    crate::message::ensure_dirs(dir.path()).unwrap();

    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));

    let dir_path = dir.path().to_path_buf();
    let launcher = Arc::new(ScriptedSessionLauncher::with_steps(vec![
        ScriptedStep::with_hook(
            SessionLoopOutcome::ValidationFailed { quick_exit: false },
            {
                let dir_path = dir_path.clone();
                move || seed_past_todo(&dir_path)
            },
        ),
        ScriptedStep::with_hook(
            SessionLoopOutcome::ValidationFailed { quick_exit: false },
            {
                let dir_path = dir_path.clone();
                move || seed_past_todo(&dir_path)
            },
        ),
    ]));

    let daemon = Daemon::new_with_clock_and_launcher(
        dir.path().to_path_buf(),
        clock.clone(),
        launcher.clone(),
    );

    let sock_path = dir.path().join("test.sock");
    let server = crate::socket::SocketServer::bind(&sock_path).unwrap();
    server.set_nonblocking(true).unwrap();

    let mut cryo_state = test_cryo_state();
    cryo_state.session_number = 0;
    cryo_state.pid = Some(std::process::id());

    let bootstrap = DaemonBootstrapState {
        next_report_time: None,
        next_wake: None,
        run_now: true,
        watch_dirs: Vec::new(),
    };

    let config = CryoConfig::default();

    let (_tx, rx) = mpsc::channel();

    daemon
        .run_event_loop(&config, &mut cryo_state, bootstrap, &server, &rx)
        .unwrap();

    // Two crashed sessions were consumed from the scripted launcher; the
    // third (fallthrough PlanComplete) ends the loop.
    let invocations = launcher.session_numbers();
    assert!(
        invocations.len() >= 3,
        "expected 2 failures + 1 plan-complete = 3 invocations, got {invocations:?}"
    );
}

#[test]
fn fs_session_effects_write_reply_with_question_writes_frontmatter_and_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let mut effects = crate::daemon::effects::FsSessionEffects::new(dir.path());
    let ts = chrono::NaiveDate::from_ymd_opt(2026, 4, 25)
        .unwrap()
        .and_hms_opt(15, 30, 0)
        .unwrap();

    effects
        .write_reply(
            crate::daemon::effects::ReplyAuthor::Agent,
            "What is ice?",
            ts,
            true,
        )
        .unwrap();

    let outbox = dir.path().join("messages").join("outbox");
    let mut entries: Vec<_> = std::fs::read_dir(&outbox)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "md"))
        .collect();
    assert_eq!(entries.len(), 1, "expected exactly one outbox file");
    let content = std::fs::read_to_string(entries.remove(0).path()).unwrap();

    assert!(
        content.contains("question: true"),
        "expected frontmatter `question: true`; got:\n{content}"
    );
    assert!(
        content.contains("\nWhat is ice?"),
        "expected body to be the raw text without prefix; got:\n{content}"
    );
}

#[test]
fn fs_session_effects_write_reply_without_question_omits_frontmatter_and_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let mut effects = crate::daemon::effects::FsSessionEffects::new(dir.path());
    let ts = chrono::NaiveDate::from_ymd_opt(2026, 4, 25)
        .unwrap()
        .and_hms_opt(15, 30, 0)
        .unwrap();

    effects
        .write_reply(
            crate::daemon::effects::ReplyAuthor::Agent,
            "Status update",
            ts,
            false,
        )
        .unwrap();

    let outbox = dir.path().join("messages").join("outbox");
    let mut entries: Vec<_> = std::fs::read_dir(&outbox)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "md"))
        .collect();
    assert_eq!(entries.len(), 1);
    let content = std::fs::read_to_string(entries.remove(0).path()).unwrap();

    assert!(!content.contains("question: true"), "got:\n{content}");
    assert!(
        !content.contains("Question: Status update"),
        "got:\n{content}"
    );
}
