use super::*;
use std::collections::VecDeque;
use std::fs;
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
fn wake_prompt_uses_bounded_todo_display() {
    // The per-session prompt must use the prompt-bounded TODO rendering, not
    // the unbounded full list: done items are never deleted, so the full list
    // grows with chamber age and is re-injected on every single wake.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let session_src = std::fs::read_to_string(root.join("src/daemon/session.rs")).unwrap();
    assert!(
        session_src.contains("display_for_prompt()"),
        "run_session should build the prompt TODO section via TodoFile::display_for_prompt"
    );
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
    parked: AtomicBool,
    /// Scripted results for `reclaim_parked_if_disconnected`, one per poll
    /// tick while parked; empty queue means "client still alive".
    parked_disconnects: Mutex<VecDeque<bool>>,
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
            parked: AtomicBool::new(false),
            parked_disconnects: Mutex::new(VecDeque::new()),
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
            parked: AtomicBool::new(false),
            parked_disconnects: Mutex::new(VecDeque::new()),
        }
    }

    fn script_parked_disconnects(&self, values: Vec<bool>) {
        *self.parked_disconnects.lock().unwrap() = values.into();
    }

    fn responses(&self) -> Vec<(bool, String)> {
        self.responses.lock().unwrap().clone()
    }

    fn terminated(&self) -> bool {
        self.terminated.load(Ordering::Relaxed)
    }

    fn parked(&self) -> bool {
        self.parked.load(Ordering::Relaxed)
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

    fn park(&mut self) -> Result<()> {
        anyhow::ensure!(!self.parked.load(Ordering::Relaxed), "already parked");
        self.parked.store(true, Ordering::Relaxed);
        Ok(())
    }

    fn respond_parked(&mut self, ok: bool, message: String) -> Result<()> {
        anyhow::ensure!(self.parked.load(Ordering::Relaxed), "nothing parked");
        self.parked.store(false, Ordering::Relaxed);
        self.responses.lock().unwrap().push((ok, message));
        self.respond_results
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Ok(()))
    }

    fn reclaim_parked_if_disconnected(&mut self) -> bool {
        let disconnected = self
            .parked_disconnects
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(false);
        if disconnected {
            assert!(
                self.parked.load(Ordering::Relaxed),
                "reclaim with nothing parked"
            );
            self.parked.store(false, Ordering::Relaxed);
        }
        disconnected
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
    claim_failure: Option<String>,
    replies: Vec<(ReplyAuthor, String, NaiveDateTime, bool)>,
    inbox_messages: Vec<(String, crate::message::Message)>,
    archived_inbox: Vec<(String, crate::message::Message)>,
    archived_outbox: Vec<(String, crate::message::Message)>,
    todos: Vec<crate::todo::TodoItem>,
    next_todo_id: u32,
    scripted_claims: VecDeque<Vec<(String, crate::message::Message)>>,
}

impl FakeSessionEffects {
    fn new() -> Self {
        Self {
            reply_failure: None,
            claim_failure: None,
            replies: Vec::new(),
            inbox_messages: Vec::new(),
            archived_inbox: Vec::new(),
            archived_outbox: Vec::new(),
            todos: Vec::new(),
            next_todo_id: 1,
            scripted_claims: VecDeque::new(),
        }
    }

    /// Make the next (and only the next) `claim_inbox_batch` call fail, to
    /// exercise the `ReceiveRequestOutcome { ok: false, .. }` path.
    fn with_claim_failure(message: &str) -> Self {
        let mut effects = Self::new();
        effects.claim_failure = Some(message.to_string());
        effects
    }

    /// Script future `claim_inbox_batch` results: each call pops one entry
    /// (empty vec = "inbox still empty"). Falls back to draining
    /// `inbox_messages` when the script is exhausted.
    fn push_scripted_claim(&mut self, batch: Vec<(String, crate::message::Message)>) {
        self.scripted_claims.push_back(batch);
    }

    fn make_inbox_message(filename: &str, body: &str) -> (String, crate::message::Message) {
        (
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
        )
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
        if let Some(message) = self.claim_failure.take() {
            anyhow::bail!("{message}");
        }
        let claimed = match self.scripted_claims.pop_front() {
            Some(batch) => batch,
            None => std::mem::take(&mut self.inbox_messages),
        };
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

    fn register_signal_handlers(&self, _shutdown: &Arc<AtomicBool>) -> Result<()> {
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
        wait_timeout_secs: crate::config::DEFAULT_WAIT_TIMEOUT_SECS,
        spawn_time,
        retry_remaining: false,
    }
}

/// Like `test_session_context`, but lets the caller control the chamber's
/// default `receive --wait` timeout instead of hard-coding
/// `DEFAULT_WAIT_TIMEOUT_SECS`.
fn test_session_context_with_wait(
    cryo_state: &CryoState,
    timeout_secs: u64,
    spawn_time: Instant,
    wait_timeout_secs: u64,
) -> ActiveSessionContext<'_> {
    ActiveSessionContext {
        cryo_state,
        timeout_secs,
        wait_timeout_secs,
        spawn_time,
        retry_remaining: false,
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
            assert!(!paths.is_empty());
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
fn test_compute_sleep_timeout_wake_only() {
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let wake = now + chrono::Duration::seconds(120);
    let timeout = compute_sleep_timeout(Some(wake), now);
    assert_eq!(timeout, Duration::from_secs(120));
}

#[test]
fn test_compute_sleep_timeout_past_wake_is_zero() {
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let wake = now - chrono::Duration::seconds(45);
    let timeout = compute_sleep_timeout(Some(wake), now);
    assert_eq!(timeout, Duration::ZERO);
}

#[test]
fn test_compute_sleep_timeout_neither() {
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let timeout = compute_sleep_timeout(None, now);
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
            paths: vec![PathBuf::from("mailbox/inbox/mail-123")],
        }),
    ]);

    let outcome = wait_for_idle_event(&source, Duration::from_secs(1), None, sample_time);

    assert_eq!(
        outcome,
        IdleWaitOutcome::WakeFromInbox {
            paths: vec![
                PathBuf::from("messages/inbox/admin.md"),
                PathBuf::from("mailbox/inbox/mail-123"),
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
    // Failure path does not require a pending TODO. Claimed TODO retry is
    // handled by the outer session-finalization path, not by this response.
    let decision = resolve_hibernate_request(false, 7, Some("provider failed"), false);

    assert_eq!(
        decision.outcome,
        Some(SessionLoopOutcome::ValidationFailed {
            quick_exit: false,
            retryable: false,
        })
    );
    assert!(decision.response_ok);
    assert_eq!(decision.response_message, "Failure recorded.");
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
        chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap(),
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
    let outcome = SessionLoopOutcome::ValidationFailed {
        quick_exit: false,
        retryable: false,
    };

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
        SessionLoopOutcome::ValidationFailed {
            quick_exit: false,
            retryable: false,
        },
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
    assert!(SessionLoopOutcome::ValidationFailed {
        quick_exit: false,
        retryable: false
    }
    .is_crash());
    assert!(SessionLoopOutcome::ValidationFailed {
        quick_exit: true,
        retryable: true
    }
    .is_crash());
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
        SessionLoopOutcome::ValidationFailed {
            quick_exit: false,
            retryable: false,
        }
    );
    assert_eq!(shutdown.finish_reason, "daemon shutdown — agent terminated");
    assert_eq!(
        timeout.outcome,
        SessionLoopOutcome::ValidationFailed {
            quick_exit: false,
            retryable: false,
        }
    );
    assert_eq!(timeout.finish_reason, "session timeout — agent killed");
}

#[test]
fn test_resolve_child_exit_after_hibernate_returns_outcome() {
    let decision = resolve_child_exit(
        Some(SessionLoopOutcome::PlanComplete),
        Duration::from_secs(1),
        Some(0),
    );

    assert_eq!(decision.outcome, SessionLoopOutcome::PlanComplete);
    assert_eq!(decision.finish_reason, "session complete");
    assert!(!decision.quick_exit);
    assert!(!decision.retryable);
}

#[test]
fn test_resolve_child_exit_without_hibernate_marks_quick_exit() {
    let quick = resolve_child_exit(None, Duration::from_secs(2), Some(0));
    let slow = resolve_child_exit(None, Duration::from_secs(8), Some(0));

    assert_eq!(
        quick.outcome,
        SessionLoopOutcome::ValidationFailed {
            quick_exit: true,
            retryable: true
        }
    );
    assert_eq!(quick.finish_reason, "agent exited without hibernate");
    assert!(quick.quick_exit);
    assert!(quick.retryable);

    assert_eq!(
        slow.outcome,
        SessionLoopOutcome::ValidationFailed {
            quick_exit: false,
            retryable: false
        }
    );
    assert_eq!(slow.finish_reason, "agent exited without hibernate");
    assert!(!slow.quick_exit);
    assert!(!slow.retryable);
}

#[test]
fn test_resolve_child_exit_nonzero_exit_is_retryable_after_quick_exit_window() {
    let decision = resolve_child_exit(None, Duration::from_secs(8), Some(1));

    assert_eq!(
        decision.outcome,
        SessionLoopOutcome::ValidationFailed {
            quick_exit: false,
            retryable: true
        }
    );
    assert!(!decision.quick_exit);
    assert!(decision.retryable);
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
        SessionLoopOutcome::ValidationFailed {
            quick_exit: true,
            retryable: true,
        }
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
        SessionLoopOutcome::ValidationFailed {
            quick_exit: false,
            retryable: false,
        },
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
        SessionLoopOutcome::ValidationFailed {
            quick_exit: true,
            retryable: false,
        }
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
fn test_drive_active_session_send_then_crash_is_not_retryable() {
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
        vec![Ok(Some(crate::socket::Request::Send {
            question: false,
            text: "Working on it".into(),
        }))],
        vec![Ok(None), Ok(Some(ChildExitStatus { code: Some(1) }))],
    );
    let mut effects = FakeSessionEffects::new();

    let outcome = daemon
        .drive_active_session(
            &mut runtime,
            &mut effects,
            ActiveSessionContext {
                retry_remaining: true,
                ..test_session_context(&cryo_state, 60, clock.monotonic_now())
            },
            begin_test_logger(dir.path()),
        )
        .unwrap();

    assert_eq!(
        outcome,
        SessionLoopOutcome::ValidationFailed {
            quick_exit: true,
            retryable: false,
        }
    );
    assert_eq!(effects.replies.len(), 1);
    assert_eq!(effects.replies[0].0, ReplyAuthor::Agent);
    assert_eq!(effects.replies[0].1, "Working on it");
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
    previous_session_crashed: bool,
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

    fn previous_session_crashed_flags(&self) -> Vec<bool> {
        self.invocations
            .lock()
            .unwrap()
            .iter()
            .map(|i| i.previous_session_crashed)
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
        _retry_remaining: bool,
    ) -> Result<SessionLoopOutcome> {
        self.invocations.lock().unwrap().push(ScriptedInvocation {
            session: cryo_state.session_number,
            previous_session_crashed: cryo_state.previous_session_crashed,
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
        _retry_remaining: bool,
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

fn seed_todo_at(dir: &Path, text: &str, at: &str) {
    crate::todo::TodoFile::new(dir.join("todo.json"))
        .add(text.into(), at.into())
        .unwrap();
}

struct ShutdownAfterRetryableCrashLauncher {
    invocations: Mutex<u32>,
}

impl ShutdownAfterRetryableCrashLauncher {
    fn new() -> Self {
        Self {
            invocations: Mutex::new(0),
        }
    }

    fn invocations(&self) -> u32 {
        *self.invocations.lock().unwrap()
    }
}

impl SessionLauncher for ShutdownAfterRetryableCrashLauncher {
    #[allow(clippy::too_many_arguments)]
    fn run_session(
        &self,
        daemon: &Daemon,
        config: &CryoConfig,
        cryo_state: &CryoState,
        _server: &crate::socket::SocketServer,
        _delayed_wake: Option<&str>,
        _wake_sources: &[PathBuf],
        _provider_env: &std::collections::HashMap<String, String>,
        _provider_name: Option<&str>,
        retry_remaining: bool,
    ) -> Result<SessionLoopOutcome> {
        *self.invocations.lock().unwrap() += 1;
        let mut logger = crate::log::EventLogger::begin(
            &daemon.log_path,
            cryo_state.session_number,
            "test task",
            "mock-agent",
            &[],
        )?;
        logger.log_event("agent started (pid 123)")?;

        let mut runtime =
            FakeSessionRuntime::new(vec![], vec![Ok(Some(ChildExitStatus { code: Some(1) }))]);
        let mut effects = effects::FsSessionEffects::new(&daemon.dir);
        let outcome = daemon.drive_active_session(
            &mut runtime,
            &mut effects,
            ActiveSessionContext {
                cryo_state,
                timeout_secs: config.max_session_duration,
                wait_timeout_secs: config
                    .wait_timeout
                    .unwrap_or(crate::config::DEFAULT_WAIT_TIMEOUT_SECS),
                spawn_time: daemon.clock.monotonic_now(),
                retry_remaining,
            },
            logger,
        );
        daemon.shutdown.store(true, Ordering::Relaxed);
        outcome
    }
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
        next_wake: None,
        run_now: false,
        watch_dirs: Vec::new(),
    };

    let (tx, rx) = mpsc::channel();
    tx.send(DaemonEvent::InboxChanged {
        paths: vec![PathBuf::from("mailbox/inbox/mail-123")],
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
        vec![vec![PathBuf::from("mailbox/inbox/mail-123")]]
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
fn test_run_event_loop_non_retryable_validation_failures_follow_schedule() {
    // Non-retryable ValidationFailed outcomes are treated like hibernates:
    // the next wake is determined solely by the TODO list (or by external
    // inbox/wake events). With a past TODO seeded, the scheduler still
    // re-fires on the next loop iteration, but there is no retry backoff path.
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
            SessionLoopOutcome::ValidationFailed {
                quick_exit: false,
                retryable: false,
            },
            {
                let dir_path = dir_path.clone();
                move || seed_past_todo(&dir_path)
            },
        ),
        ScriptedStep::with_hook(
            SessionLoopOutcome::ValidationFailed {
                quick_exit: false,
                retryable: false,
            },
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
fn test_run_event_loop_retries_retryable_failure_with_unread_inbox() {
    let dir = tempfile::tempdir().unwrap();
    crate::message::ensure_dirs(dir.path()).unwrap();
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    crate::message::write_message(
        dir.path(),
        "inbox",
        &crate::message::Message {
            from: "user".into(),
            subject: "retry".into(),
            body: "please try this".into(),
            timestamp: now,
            metadata: Default::default(),
            is_question: false,
        },
    )
    .unwrap();

    let clock = Arc::new(TestClock::new(now));
    let launcher = Arc::new(ScriptedSessionLauncher::new(vec![
        SessionLoopOutcome::ValidationFailed {
            quick_exit: true,
            retryable: true,
        },
        SessionLoopOutcome::PlanComplete,
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
        next_wake: None,
        run_now: false,
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

    assert_eq!(
        launcher.session_numbers(),
        vec![1, 2],
        "retryable startup failure should be retried before the unread inbox is left idle"
    );
    assert_eq!(
        launcher.previous_session_crashed_flags(),
        vec![false, false],
        "in-daemon retries must not show the previous-session-crashed notice before retries are exhausted"
    );
    assert_eq!(
        clock.local_now(),
        now + chrono::Duration::seconds(1),
        "first retry should wait for the first backoff gap"
    );
}

#[test]
fn test_run_event_loop_does_not_claim_new_todos_during_retry_backoff() {
    let dir = tempfile::tempdir().unwrap();
    seed_todo_at(dir.path(), "initial wake", "2026-03-01T11:59:00");
    seed_todo_at(dir.path(), "due during retry gap", "2026-03-01T12:00:01");

    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let launcher = Arc::new(ScriptedSessionLauncher::new(vec![
        SessionLoopOutcome::ValidationFailed {
            quick_exit: true,
            retryable: true,
        },
        SessionLoopOutcome::PlanComplete,
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

    assert_eq!(launcher.session_numbers(), vec![1, 2]);
    let items = crate::todo::TodoFile::new(dir.path().join("todo.json"))
        .items()
        .unwrap();
    let initial = items
        .iter()
        .find(|item| item.text == "initial wake")
        .unwrap();
    let became_due = items
        .iter()
        .find(|item| item.text == "due during retry gap")
        .unwrap();
    assert!(initial.done);
    assert!(!initial.claimed);
    assert!(!became_due.done);
    assert!(!became_due.claimed);
}

#[test]
fn test_run_event_loop_writes_deferred_fallback_when_shutdown_interrupts_retry_backoff() {
    let dir = tempfile::tempdir().unwrap();
    crate::message::ensure_dirs(dir.path()).unwrap();

    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let launcher = Arc::new(ShutdownAfterRetryableCrashLauncher::new());
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

    assert_eq!(launcher.invocations(), 1);
    let outbox = crate::message::read_outbox(dir.path()).unwrap();
    assert_eq!(outbox.len(), 1, "deferred fallback should be made visible");
    assert!(
        outbox[0]
            .1
            .body
            .contains("daemon: agent crashed before sending"),
        "{outbox:?}"
    );
}

#[test]
fn test_agent_retry_backoff_increases_exponentially() {
    let gaps: Vec<_> = (1..=MAX_AGENT_RETRIES).map(agent_retry_backoff).collect();

    assert_eq!(
        gaps,
        vec![
            Duration::from_secs(1),
            Duration::from_secs(2),
            Duration::from_secs(4),
            Duration::from_secs(8),
            Duration::from_secs(16),
            Duration::from_secs(32),
            Duration::from_secs(64),
            Duration::from_secs(128),
            Duration::from_secs(256),
            Duration::from_secs(512),
        ]
    );
}

#[test]
fn test_run_event_loop_stops_after_ten_retryable_failures() {
    let dir = tempfile::tempdir().unwrap();
    crate::message::ensure_dirs(dir.path()).unwrap();
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    crate::message::write_message(
        dir.path(),
        "inbox",
        &crate::message::Message {
            from: "user".into(),
            subject: "retry".into(),
            body: "please try this".into(),
            timestamp: now,
            metadata: Default::default(),
            is_question: false,
        },
    )
    .unwrap();

    let clock = Arc::new(TestClock::new(now));
    let mut outcomes = vec![
        SessionLoopOutcome::ValidationFailed {
            quick_exit: true,
            retryable: true,
        };
        MAX_AGENT_RETRIES + 1
    ];
    outcomes.push(SessionLoopOutcome::PlanComplete);
    let launcher = Arc::new(ScriptedSessionLauncher::new(outcomes));
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
        next_wake: None,
        run_now: false,
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

    let expected_attempts = MAX_AGENT_RETRIES + 1;
    assert_eq!(launcher.session_numbers().len(), expected_attempts);
    assert_eq!(
        clock.local_now(),
        now + chrono::Duration::from_std(
            (1..=MAX_AGENT_RETRIES)
                .map(agent_retry_backoff)
                .sum::<Duration>()
        )
        .unwrap()
    );
    assert!(
        cryo_state.previous_session_crashed,
        "the final exhausted retryable failure should be recorded as a crash"
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

#[test]
fn test_run_event_loop_wakes_on_preexisting_inbox_at_startup() {
    // Regression for the bug where inbox messages delivered while the daemon
    // was down (after `cryo restart`, a service auto-restart, or a
    // crash-restart) sat unread forever: the notify watcher only reports files
    // created after it starts, and `run_now` was set only for the first session
    // or a past-due TODO. A resumed daemon (session_number > 0, no pending
    // TODO, run_now = false) with a pre-existing inbox file must still run a
    // session immediately (invariant 2).
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("messages").join("inbox");
    std::fs::create_dir_all(&inbox).unwrap();
    std::fs::write(inbox.join("human-1.md"), "from: human\n\nAnswer me").unwrap();

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
    cryo_state.session_number = 4; // resumed daemon, not the first session
    cryo_state.pid = Some(std::process::id());

    let bootstrap = DaemonBootstrapState {
        next_wake: None,
        run_now: false,
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
        launcher.session_numbers(),
        vec![5],
        "a pre-existing inbox file must drive one session even with run_now=false and no pending TODO"
    );
}

#[test]
fn test_run_treats_locked_but_unresponsive_pid_as_stale_on_startup() {
    // A live PID (our own) that `is_locked` reports as alive but whose daemon
    // is not answering on the socket is the reboot / PID-reuse case. Startup
    // must NOT bail ("Another daemon is already running"); it must treat the
    // lock as stale, take over, and mint a fresh identity.
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
            pid: Some(std::process::id()),
            agent_override: None,
            max_session_duration_override: None,
            instance_id: Some("stale".into()),
            session_active: false,
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

    // With the old `is_locked`-only guard this returned Err; the liveness probe
    // lets it proceed.
    daemon.run().unwrap();

    let saved = state_store.saved_states();
    assert!(
        !saved.is_empty(),
        "startup should proceed past the stale-PID guard and persist state"
    );
}

/// Drive one idle-mode IPC round-trip: a client thread sends `request` while
/// the main thread accepts it and dispatches through `handle_idle_request`.
/// Returns the client's response plus the inbox filenames left behind, so
/// tests can assert idle commands neither succeed nor archive.
fn idle_request_round_trip(
    request: crate::socket::Request,
) -> (crate::socket::Response, Vec<String>) {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("messages").join("inbox");
    std::fs::create_dir_all(&inbox).unwrap();
    std::fs::write(inbox.join("human-1.md"), "from: human\n\nHello").unwrap();

    let sock = crate::socket::socket_path(dir.path());
    std::fs::create_dir_all(sock.parent().unwrap()).unwrap();
    let mut st = test_cryo_state();
    st.instance_id = Some("idle-instance".into());
    crate::state::save_state(&crate::state::state_path(dir.path()), &st).unwrap();

    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock);

    let server = crate::socket::SocketServer::bind(&sock).unwrap();

    let dir_for_client = dir.path().to_path_buf();
    let handle =
        std::thread::spawn(move || crate::daemon_client::send_request(&dir_for_client, &request));

    let (req, responder) = server
        .accept_one(Some("idle-instance"))
        .unwrap()
        .expect("request should pass the instance check");
    daemon.handle_idle_request(req, responder).unwrap();

    let response = handle.join().unwrap().unwrap();
    let remaining = crate::message::list_inbox(dir.path()).unwrap();
    (response, remaining)
}

#[test]
fn test_idle_receive_refuses_without_archiving_inbox() {
    // Idle `Receive` has no `SessionInboxState` and thus no reply obligation, so
    // claiming + archiving the batch would terminally consume it with nobody on
    // the hook to answer (invariant 2). It must refuse and leave the inbox.
    let (response, remaining) = idle_request_round_trip(crate::socket::Request::Receive);

    assert!(!response.ok, "idle receive must be refused: {response:?}");
    assert!(
        response.message.contains("No active session"),
        "refusal should explain why: {}",
        response.message
    );
    assert_eq!(
        remaining,
        vec!["human-1.md".to_string()],
        "idle receive must leave the inbox untouched (not archive it)"
    );
}

#[test]
fn test_idle_dialog_refuses_without_archiving_inbox() {
    // Idle `Dialog` used to claim + archive the inbox via `handle_dialog_request`
    // with no fallback reply. It must now refuse and leave the inbox in place.
    let (response, remaining) = idle_request_round_trip(crate::socket::Request::Dialog {
        filter: crate::socket::DialogFilter::All,
    });

    assert!(!response.ok, "idle dialog must be refused: {response:?}");
    assert!(
        response.message.contains("No active session"),
        "refusal should explain why: {}",
        response.message
    );
    assert_eq!(
        remaining,
        vec!["human-1.md".to_string()],
        "idle dialog must leave the inbox untouched (not archive it)"
    );
}

#[test]
fn fake_runtime_park_then_respond_parked_records_response() {
    let mut runtime = FakeSessionRuntime::new(vec![], vec![]);
    assert!(!runtime.parked());
    runtime.park().unwrap();
    assert!(runtime.parked());
    assert!(runtime.park().is_err(), "double park must be rejected");
    runtime.respond_parked(true, "delivered".into()).unwrap();
    assert!(!runtime.parked());
    assert_eq!(runtime.responses(), vec![(true, "delivered".into())]);
    assert!(
        runtime.respond_parked(true, "again".into()).is_err(),
        "respond_parked without a parked wait must be rejected"
    );
}

fn receive_wait_request(
    timeout_secs: Option<u64>,
) -> anyhow::Result<Option<crate::socket::Request>> {
    Ok(Some(crate::socket::Request::ReceiveWait { timeout_secs }))
}

#[test]
fn test_receive_wait_with_pending_inbox_claims_immediately() {
    // ReceiveWait with a non-empty inbox behaves exactly like Receive: no park.
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
            receive_wait_request(None),
            Ok(Some(crate::socket::Request::Send {
                text: "ack".into(),
                question: false,
            })),
            Ok(Some(crate::socket::Request::Hibernate {
                complete: true,
                exit_code: 0,
                summary: None,
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
    effects.push_inbox_message("q1.md", "hello");

    let outcome = daemon
        .drive_active_session(
            &mut runtime,
            &mut effects,
            test_session_context(&cryo_state, 60, clock.monotonic_now()),
            begin_test_logger(dir.path()),
        )
        .unwrap();

    assert_eq!(outcome, SessionLoopOutcome::PlanComplete);
    assert!(!runtime.parked());
    let responses = runtime.responses();
    assert!(
        responses[0].0 && responses[0].1.contains("hello"),
        "{responses:?}"
    );
}

#[test]
fn test_receive_wait_parks_then_delivers_new_message_in_same_session() {
    let dir = tempfile::tempdir().unwrap();
    crate::message::ensure_dirs(dir.path()).unwrap();
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock.clone());
    let cryo_state = test_cryo_state();
    // Tick 1: ReceiveWait (inbox empty -> park). Tick 2: poll finds nothing.
    // Tick 3: poll claims the new batch -> respond_parked. Then Send + Hibernate.
    let mut runtime = FakeSessionRuntime::new(
        vec![
            receive_wait_request(None),
            Ok(None),
            Ok(None),
            Ok(Some(crate::socket::Request::Send {
                text: "round 2 ack".into(),
                question: false,
            })),
            Ok(Some(crate::socket::Request::Hibernate {
                complete: true,
                exit_code: 0,
                summary: None,
            })),
        ],
        (0..6)
            .map(|_| Ok(None))
            .chain([Ok(Some(ChildExitStatus { code: Some(0) }))])
            .collect(),
    );
    let mut effects = FakeSessionEffects::new();
    effects.push_scripted_claim(vec![]); // ReceiveWait's own claim: empty -> park
    effects.push_scripted_claim(vec![]); // first poll tick: still empty
    effects.push_scripted_claim(vec![FakeSessionEffects::make_inbox_message(
        "r2.md",
        "second round",
    )]);

    let outcome = daemon
        .drive_active_session(
            &mut runtime,
            &mut effects,
            test_session_context(&cryo_state, 60, clock.monotonic_now()),
            begin_test_logger(dir.path()),
        )
        .unwrap();

    assert_eq!(outcome, SessionLoopOutcome::PlanComplete);
    let responses = runtime.responses();
    // Response 0 is the parked delivery, then "Message sent", then hibernate ack.
    assert!(
        responses[0].0 && responses[0].1.contains("second round"),
        "{responses:?}"
    );
    assert!(!runtime.parked());
    let log = fs::read_to_string(dir.path().join("cryo.log")).unwrap();
    assert!(log.contains("wait: parked"), "{log}");
    assert!(
        log.contains("wait: delivered 1 message(s) [r2.md]"),
        "{log}"
    );
}

#[test]
fn test_receive_wait_times_out_and_tells_agent_to_hibernate() {
    let dir = tempfile::tempdir().unwrap();
    crate::message::ensure_dirs(dir.path()).unwrap();
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock.clone());
    let cryo_state = test_cryo_state();
    // 1s wait timeout = 10 ticks of 100ms. Give enough empty accepts, then hibernate.
    let mut requests = vec![receive_wait_request(Some(1))];
    requests.extend((0..12).map(|_| Ok(None)));
    requests.push(Ok(Some(crate::socket::Request::Hibernate {
        complete: true,
        exit_code: 0,
        summary: None,
    })));
    let mut waits: Vec<std::io::Result<Option<ChildExitStatus>>> =
        (0..14).map(|_| Ok(None)).collect();
    waits.push(Ok(Some(ChildExitStatus { code: Some(0) })));
    let mut runtime = FakeSessionRuntime::new(requests, waits);
    let mut effects = FakeSessionEffects::new();

    let outcome = daemon
        .drive_active_session(
            &mut runtime,
            &mut effects,
            test_session_context(&cryo_state, 3600, clock.monotonic_now()),
            begin_test_logger(dir.path()),
        )
        .unwrap();

    assert_eq!(outcome, SessionLoopOutcome::PlanComplete);
    let responses = runtime.responses();
    assert!(
        responses[0].0 && responses[0].1.contains("No new messages"),
        "{responses:?}"
    );
    let log = fs::read_to_string(dir.path().join("cryo.log")).unwrap();
    assert!(log.contains("wait: timed out"), "{log}");
}

#[test]
fn test_session_deadline_suspended_while_parked_and_reset_on_delivery() {
    // Session timeout is 1s; the wait parks for ~2s; the session must survive.
    let dir = tempfile::tempdir().unwrap();
    crate::message::ensure_dirs(dir.path()).unwrap();
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock.clone());
    let cryo_state = test_cryo_state();
    // 20 empty poll ticks (2s virtual) past the 1s session deadline, then delivery.
    let mut requests = vec![receive_wait_request(Some(3600))];
    requests.extend((0..21).map(|_| Ok(None)));
    requests.push(Ok(Some(crate::socket::Request::Send {
        text: "late ack".into(),
        question: false,
    })));
    requests.push(Ok(Some(crate::socket::Request::Hibernate {
        complete: true,
        exit_code: 0,
        summary: None,
    })));
    let mut waits: Vec<std::io::Result<Option<ChildExitStatus>>> =
        (0..24).map(|_| Ok(None)).collect();
    waits.push(Ok(Some(ChildExitStatus { code: Some(0) })));
    let mut runtime = FakeSessionRuntime::new(requests, waits);
    let mut effects = FakeSessionEffects::new();
    effects.push_scripted_claim(vec![]); // park
    for _ in 0..20 {
        effects.push_scripted_claim(vec![]); // parked polls while deadline passes
    }
    effects.push_scripted_claim(vec![FakeSessionEffects::make_inbox_message(
        "late.md",
        "still there?",
    )]);

    let outcome = daemon
        .drive_active_session(
            &mut runtime,
            &mut effects,
            test_session_context(&cryo_state, 1, clock.monotonic_now()),
            begin_test_logger(dir.path()),
        )
        .unwrap();

    assert_eq!(
        outcome,
        SessionLoopOutcome::PlanComplete,
        "session must not be timeout-killed while parked"
    );
    assert!(!runtime.terminated());
}

#[test]
fn test_receive_wait_refused_while_already_parked() {
    // A second `receive --wait` arriving while one is already parked for
    // this session must be refused outright, not silently double-park.
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
            receive_wait_request(None), // parks (inbox is empty)
            receive_wait_request(None), // refused: already parked
            Ok(Some(crate::socket::Request::Hibernate {
                complete: true,
                exit_code: 0,
                summary: None,
            })),
        ],
        vec![
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

    assert_eq!(outcome, SessionLoopOutcome::PlanComplete);
    // `park()` doesn't itself call `respond()`, so response 0 is the
    // refusal of the second (rejected) ReceiveWait; the eventual
    // `respond_parked` release for the first (still-parked) wait comes
    // later, at child exit.
    let responses = runtime.responses();
    assert!(
        !responses[0].0 && responses[0].1.contains("already parked"),
        "{responses:?}"
    );
    assert!(!runtime.parked());
}

#[test]
fn test_receive_wait_refused_when_inbox_claim_fails() {
    // If the underlying inbox read itself fails, ReceiveWait must surface the
    // failure and refuse, not park on top of a broken claim.
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
        vec![receive_wait_request(None)],
        vec![Ok(None), Ok(Some(ChildExitStatus { code: Some(0) }))],
    );
    let mut effects = FakeSessionEffects::with_claim_failure("disk on fire");

    daemon
        .drive_active_session(
            &mut runtime,
            &mut effects,
            test_session_context(&cryo_state, 60, clock.monotonic_now()),
            begin_test_logger(dir.path()),
        )
        .unwrap();

    let responses = runtime.responses();
    assert!(
        !responses[0].0 && responses[0].1.contains("disk on fire"),
        "{responses:?}"
    );
    assert!(!runtime.parked(), "a failed claim must not leave a park");
}

#[test]
fn test_receive_wait_refused_with_unresolved_claimed_batch() {
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
            Ok(Some(crate::socket::Request::Receive)), // claims q1.md
            receive_wait_request(None),                // refused: batch unresolved
            Ok(Some(crate::socket::Request::Send {
                text: "ack".into(),
                question: false,
            })),
            Ok(Some(crate::socket::Request::Hibernate {
                complete: true,
                exit_code: 0,
                summary: None,
            })),
        ],
        vec![
            Ok(None),
            Ok(None),
            Ok(None),
            Ok(None),
            Ok(Some(ChildExitStatus { code: Some(0) })),
        ],
    );
    let mut effects = FakeSessionEffects::new();
    effects.push_inbox_message("q1.md", "hello");

    daemon
        .drive_active_session(
            &mut runtime,
            &mut effects,
            test_session_context(&cryo_state, 60, clock.monotonic_now()),
            begin_test_logger(dir.path()),
        )
        .unwrap();

    let responses = runtime.responses();
    assert!(!responses[1].0, "{responses:?}");
    assert!(
        responses[1]
            .1
            .contains("send a message for the current inbox batch"),
        "{responses:?}"
    );
    assert!(!runtime.parked());
}

#[test]
fn test_agent_exit_while_parked_releases_wait() {
    // The agent process dies while its receive --wait is parked. The daemon
    // must release the parked responder (best-effort) before finalizing.
    let dir = tempfile::tempdir().unwrap();
    crate::message::ensure_dirs(dir.path()).unwrap();
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock.clone());
    let cryo_state = test_cryo_state();
    // Tick 1: ReceiveWait parks (empty claim). Tick 2: child has exited.
    let mut runtime = FakeSessionRuntime::new(
        vec![receive_wait_request(None)],
        vec![Ok(None), Ok(Some(ChildExitStatus { code: Some(1) }))],
    );
    let mut effects = FakeSessionEffects::new();
    effects.push_scripted_claim(vec![]); // park

    let outcome = daemon
        .drive_active_session(
            &mut runtime,
            &mut effects,
            test_session_context(&cryo_state, 60, clock.monotonic_now()),
            begin_test_logger(dir.path()),
        )
        .unwrap();

    assert!(matches!(
        outcome,
        SessionLoopOutcome::ValidationFailed { .. }
    ));
    assert!(
        !runtime.parked(),
        "child exit must release the parked responder"
    );
    let responses = runtime.responses();
    assert!(
        responses
            .iter()
            .any(|(_, m)| m.contains("Session is ending")),
        "{responses:?}"
    );
    let log = fs::read_to_string(dir.path().join("cryo.log")).unwrap();
    assert!(log.contains("wait: interrupted"), "{log}");
}

#[test]
fn test_try_wait_error_while_parked_releases_wait() {
    // `try_wait` itself errors (e.g. an OS-level failure polling the child)
    // while a receive --wait is parked. The daemon must still release the
    // parked responder before propagating the error and finalizing the
    // session, exactly like the clean agent-exit case.
    let dir = tempfile::tempdir().unwrap();
    crate::message::ensure_dirs(dir.path()).unwrap();
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock.clone());
    let cryo_state = test_cryo_state();
    // Tick 1: ReceiveWait parks (empty claim). Tick 2: try_wait errors.
    let mut runtime = FakeSessionRuntime::new(
        vec![receive_wait_request(None)],
        vec![
            Ok(None),
            Err(std::io::Error::other("simulated try_wait failure")),
        ],
    );
    let mut effects = FakeSessionEffects::new();
    effects.push_scripted_claim(vec![]); // park

    let outcome = daemon.drive_active_session(
        &mut runtime,
        &mut effects,
        test_session_context(&cryo_state, 60, clock.monotonic_now()),
        begin_test_logger(dir.path()),
    );

    let err = outcome.expect_err("try_wait error must propagate");
    assert!(err.to_string().contains("simulated try_wait failure"));
    assert!(
        !runtime.parked(),
        "try_wait error must release the parked responder"
    );
    let responses = runtime.responses();
    assert!(
        responses
            .iter()
            .any(|(_, m)| m.contains("Session is ending")),
        "{responses:?}"
    );
    let log = fs::read_to_string(dir.path().join("cryo.log")).unwrap();
    assert!(log.contains("wait: interrupted"), "{log}");
    assert!(log.contains("error checking agent"), "{log}");
}

#[test]
fn test_receive_wait_uses_chamber_default_timeout_when_request_omits_one() {
    // The request itself omits `--timeout`; only the chamber's configured
    // `wait_timeout_secs` (here 1s, via `test_session_context_with_wait`)
    // should govern how long the parked wait lasts. If the daemon fell back
    // to some other default instead, the wait would still be parked after
    // the 1s-equivalent tick budget below and the session would need to be
    // ended a different way.
    let dir = tempfile::tempdir().unwrap();
    crate::message::ensure_dirs(dir.path()).unwrap();
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock.clone());
    let cryo_state = test_cryo_state();
    // 1s chamber default = 10 ticks of 100ms. Give enough empty accepts, then hibernate.
    let mut requests = vec![receive_wait_request(None)];
    requests.extend((0..12).map(|_| Ok(None)));
    requests.push(Ok(Some(crate::socket::Request::Hibernate {
        complete: true,
        exit_code: 0,
        summary: None,
    })));
    let mut waits: Vec<std::io::Result<Option<ChildExitStatus>>> =
        (0..14).map(|_| Ok(None)).collect();
    waits.push(Ok(Some(ChildExitStatus { code: Some(0) })));
    let mut runtime = FakeSessionRuntime::new(requests, waits);
    let mut effects = FakeSessionEffects::new();

    let outcome = daemon
        .drive_active_session(
            &mut runtime,
            &mut effects,
            test_session_context_with_wait(&cryo_state, 3600, clock.monotonic_now(), 1),
            begin_test_logger(dir.path()),
        )
        .unwrap();

    assert_eq!(outcome, SessionLoopOutcome::PlanComplete);
    let responses = runtime.responses();
    assert!(
        responses[0].0 && responses[0].1.contains("No new messages"),
        "{responses:?}"
    );
}

#[test]
fn test_parked_delivery_finalizes_session_even_when_respond_parked_fails() {
    // Regression for the finding that a failed `respond_parked` on the
    // delivery path (e.g. the blocked `cryo-agent receive --wait` client is
    // already gone: EPIPE/EOF) must not skip session finalization. The batch
    // is already recorded in `inbox_state` by the time `respond_parked` is
    // attempted, so a dead client must not strand it unanswered — the next
    // tick's child-exit detection must still run `finalize_human_replies`
    // and write the daemon fallback reply (chamber invariant 2).
    let dir = tempfile::tempdir().unwrap();
    crate::message::ensure_dirs(dir.path()).unwrap();
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock.clone());
    let cryo_state = test_cryo_state();
    // Iteration 1: ReceiveWait parks (its own claim is empty), and the same
    // iteration's trailing poll also finds nothing yet. Iteration 2: the
    // poll claims a new batch and attempts `respond_parked`, which fails.
    // Iteration 3: the child has exited, driving finalization.
    let mut runtime = FakeSessionRuntime::with_respond_results(
        vec![receive_wait_request(None)],
        [Ok(None), Ok(None)]
            .into_iter()
            .chain([Ok(Some(ChildExitStatus { code: Some(1) }))])
            .collect(),
        std::iter::once(Err(anyhow::anyhow!(
            "simulated respond_parked failure (EPIPE)"
        )))
        .collect(),
    );
    let mut effects = FakeSessionEffects::new();
    effects.push_scripted_claim(vec![]); // ReceiveWait's own claim: empty -> park
    effects.push_scripted_claim(vec![]); // same-iteration poll: still empty
    effects.push_scripted_claim(vec![FakeSessionEffects::make_inbox_message(
        "late.md",
        "anyone there?",
    )]); // next poll: delivers, respond_parked fails

    let outcome = daemon
        .drive_active_session(
            &mut runtime,
            &mut effects,
            test_session_context(&cryo_state, 60, clock.monotonic_now()),
            begin_test_logger(dir.path()),
        )
        .expect("a failed respond_parked must not propagate as an error");

    assert!(matches!(
        outcome,
        SessionLoopOutcome::ValidationFailed { .. }
    ));
    assert!(
        effects
            .replies
            .iter()
            .any(|(author, _, _, _)| *author == ReplyAuthor::Daemon),
        "finalize_human_replies must still write the daemon fallback reply: {:?}",
        effects.replies
    );
    let log = fs::read_to_string(dir.path().join("cryo.log")).unwrap();
    assert!(
        log.contains("wait: parked respond failed after delivery"),
        "{log}"
    );
}

// --- should_ignore_inbox_wake (Finding 2: stale watcher events after an
// interactive conversation must not spawn a spurious session) ---

#[test]
fn test_should_ignore_inbox_wake_empty_paths_never_ignored() {
    // An empty path list carries no evidence about which files arrived, so
    // it must always wake regardless of inbox contents (fail open).
    let inbox_dir = Path::new("/chamber/messages/inbox");
    assert!(!should_ignore_inbox_wake(&[], inbox_dir, true));
    assert!(!should_ignore_inbox_wake(&[], inbox_dir, false));
}

#[test]
fn test_should_ignore_inbox_wake_all_inbox_paths_and_empty_inbox_is_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let inbox_dir = dir.path().join("messages").join("inbox");
    fs::create_dir_all(&inbox_dir).unwrap();
    let stale = inbox_dir.join("archived-away.md");
    assert!(should_ignore_inbox_wake(&[stale], &inbox_dir, true));
}

#[test]
fn test_should_ignore_inbox_wake_all_inbox_paths_but_nonempty_inbox_wakes() {
    let dir = tempfile::tempdir().unwrap();
    let inbox_dir = dir.path().join("messages").join("inbox");
    fs::create_dir_all(&inbox_dir).unwrap();
    let path = inbox_dir.join("new.md");
    assert!(!should_ignore_inbox_wake(&[path], &inbox_dir, false));
}

#[test]
fn test_should_ignore_inbox_wake_mixed_paths_wakes_even_if_inbox_empty() {
    let dir = tempfile::tempdir().unwrap();
    let inbox_dir = dir.path().join("messages").join("inbox");
    fs::create_dir_all(&inbox_dir).unwrap();
    let other_dir = dir.path().join("elsewhere");
    fs::create_dir_all(&other_dir).unwrap();
    let inside = inbox_dir.join("archived-away.md");
    let outside = other_dir.join("watched-file.md");
    assert!(!should_ignore_inbox_wake(
        &[inside, outside],
        &inbox_dir,
        true
    ));
}

#[test]
fn test_should_ignore_inbox_wake_stale_path_under_symlinked_dir_is_ignored() {
    // CI regression: on macOS the tempdir lives under /var → /private/var, so
    // an existing inbox dir canonicalizes to the real path while a stale
    // (already-archived) file path cannot be canonicalized and used to fall
    // back to the symlinked raw path — making containment fail and the stale
    // wake spawn a spurious session. The parent-resolving fallback must keep
    // the two sides comparable.
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("real");
    fs::create_dir_all(real.join("messages").join("inbox")).unwrap();
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let inbox_via_link = link.join("messages").join("inbox");
    let stale = inbox_via_link.join("archived-away.md");
    assert!(should_ignore_inbox_wake(&[stale], &inbox_via_link, true));
}

// --- SessionWaitState.timed_out (Finding 3: post-timeout re-wait must not
// bypass max_session_duration) ---

#[test]
fn test_receive_wait_refused_after_previous_wait_timed_out() {
    // ReceiveWait(Some(1)) parks and times out (~12 empty 100ms ticks cover
    // the 1s deadline), then a second ReceiveWait(None) must be refused
    // rather than parking again, and the agent must be able to hibernate
    // cleanly afterward.
    let dir = tempfile::tempdir().unwrap();
    crate::message::ensure_dirs(dir.path()).unwrap();
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock.clone());
    let cryo_state = test_cryo_state();

    let mut requests = vec![receive_wait_request(Some(1))];
    requests.extend((0..12).map(|_| Ok(None)));
    requests.push(receive_wait_request(None));
    requests.push(Ok(Some(crate::socket::Request::Hibernate {
        complete: true,
        exit_code: 0,
        summary: None,
    })));
    let mut waits: Vec<std::io::Result<Option<ChildExitStatus>>> =
        (0..14).map(|_| Ok(None)).collect();
    waits.push(Ok(Some(ChildExitStatus { code: Some(0) })));
    let mut runtime = FakeSessionRuntime::new(requests, waits);
    let mut effects = FakeSessionEffects::new();

    let outcome = daemon
        .drive_active_session(
            &mut runtime,
            &mut effects,
            test_session_context(&cryo_state, 3600, clock.monotonic_now()),
            begin_test_logger(dir.path()),
        )
        .unwrap();

    assert_eq!(outcome, SessionLoopOutcome::PlanComplete);
    let responses = runtime.responses();
    let refusal = responses
        .iter()
        .find(|(_, msg)| msg.contains("already timed out"))
        .expect("second ReceiveWait must be refused with an already-timed-out message");
    assert!(!refusal.0, "refusal must be ok=false: {responses:?}");
}

#[test]
fn test_send_log_line_trims_trailing_newline_from_stdin_body() {
    let dir = tempfile::tempdir().unwrap();
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock.clone());
    let cryo_state = test_cryo_state();
    // `send --stdin` bodies end with the heredoc's final newline; the logged
    // event must not render it as a trailing ⏎ inside the quotes.
    let mut runtime = FakeSessionRuntime::new(
        vec![
            Ok(Some(crate::socket::Request::Send {
                question: false,
                text: "Got it — reminder saved.\n".into(),
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
            Ok(None),
            Ok(Some(ChildExitStatus { code: Some(0) })),
        ],
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
    let log = std::fs::read_to_string(dir.path().join("cryo.log")).unwrap();
    assert!(
        log.contains("send: \"Got it — reminder saved.\""),
        "send event should be logged without the trailing newline: {log}"
    );
    assert!(
        !log.contains('⏎'),
        "no ⏎ should appear for a trailing-only newline: {log}"
    );
}

#[test]
fn test_strip_ansi_removes_escape_sequences_and_control_chars() {
    assert_eq!(
        strip_ansi("\u{1b}[91m\u{1b}[1mError: \u{1b}[0mboom"),
        "Error: boom"
    );
    assert_eq!(strip_ansi("\u{1b}]0;title\u{7}text"), "text");
    assert_eq!(strip_ansi("a\tb\u{8}c"), "a bc");
    assert_eq!(strip_ansi("plain"), "plain");
}

#[test]
fn test_agent_log_tail_keeps_last_ten_nonempty_lines_capped() {
    let mut content = String::new();
    for i in 1..=15 {
        content.push_str(&format!("line {i}\n\n"));
    }
    content.push_str(&"x".repeat(300));
    content.push('\n');

    let tail = agent_log_tail(&content);
    assert_eq!(tail.len(), AGENT_LOG_TAIL_LINES);
    // Order preserved: oldest of the kept lines first, giant line last.
    assert_eq!(tail[0], "line 7");
    assert_eq!(tail[8], "line 15");
    let last = &tail[9];
    assert_eq!(last.chars().count(), AGENT_LOG_TAIL_LINE_CHARS + 1);
    assert!(last.ends_with('…'));
}

#[test]
fn test_crash_debug_suffix_empty_when_log_missing_or_blank() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(crash_debug_suffix(&dir.path().join("cryo-agent.log")), "");

    let blank = dir.path().join("blank.log");
    std::fs::write(&blank, "\n\n\u{1b}[0m\n").unwrap();
    assert_eq!(crash_debug_suffix(&blank), "");
}

#[test]
fn test_crash_debug_suffix_fences_sanitized_tail() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("cryo-agent.log");
    std::fs::write(
        &log,
        "\u{1b}[91mError:\u{1b}[0m permission rejected\ncode ``` fence\n",
    )
    .unwrap();

    let suffix = crash_debug_suffix(&log);
    assert!(suffix.starts_with("\n\nLast agent log output (`cryo-agent.log`):\n````\n"));
    assert!(suffix.contains("Error: permission rejected"));
    assert!(suffix.contains("code ``` fence"));
    assert!(suffix.ends_with("\n````"));
    assert!(!suffix.contains('\u{1b}'));
}

#[test]
fn test_crash_fallback_reply_includes_agent_log_tail() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("cryo-agent.log"),
        "starting up\n\u{1b}[93m! \u{1b}[0mpermission requested: external_directory (/tmp/*); auto-rejecting\n",
    )
    .unwrap();
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock.clone());
    let cryo_state = test_cryo_state();
    // Agent claims the batch, then exits without send/hibernate → crash
    // fallback must carry the log tail so the operator sees why.
    let mut runtime = FakeSessionRuntime::new(
        vec![Ok(Some(crate::socket::Request::Receive))],
        vec![Ok(None), Ok(Some(ChildExitStatus { code: Some(0) }))],
    );
    let mut effects = FakeSessionEffects::new();
    effects.push_inbox_message("human-1.md", "please review");

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
    assert!(matches!(
        outcome,
        SessionLoopOutcome::ValidationFailed { .. }
    ));

    let (author, text, _, _) = effects
        .replies
        .iter()
        .find(|(author, _, _, _)| *author == ReplyAuthor::Daemon)
        .expect("daemon fallback reply must be written");
    assert_eq!(*author, ReplyAuthor::Daemon);
    assert!(text.contains("daemon: agent crashed before replying"));
    assert!(text.contains("Last agent log output"));
    assert!(text.contains("permission requested: external_directory (/tmp/*); auto-rejecting"));
    assert!(
        !text.contains('\u{1b}'),
        "ANSI codes must be stripped: {text:?}"
    );
}

#[test]
fn test_clean_hibernate_fallback_has_no_log_dump() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("cryo-agent.log"), "some agent output\n").unwrap();
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock.clone());
    let cryo_state = test_cryo_state();
    // Agent hibernates cleanly but never sends → status fallback, no crash,
    // so no log dump belongs in the operator's mailbox.
    let mut runtime = FakeSessionRuntime::new(
        vec![Ok(Some(crate::socket::Request::Hibernate {
            complete: false,
            exit_code: 0,
            summary: Some("quiet".into()),
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

    let (_, text, _, _) = effects
        .replies
        .iter()
        .find(|(author, _, _, _)| *author == ReplyAuthor::Daemon)
        .expect("daemon status fallback must be written");
    assert!(text.contains("hibernated without sending"));
    assert!(
        !text.contains("Last agent log output"),
        "no log dump on clean hibernate: {text:?}"
    );
}

#[test]
fn test_parked_client_death_releases_wait_without_claiming_inbox() {
    // The cryo-agent `receive --wait` client can be killed (e.g. by the agent
    // runner's shell timeout) while parked. The daemon must notice, free the
    // parked slot, and — critically — stop claiming inbox batches for the
    // dead client: a message claimed here would be swallowed and answered by
    // a fallback reply instead of the agent.
    let dir = tempfile::tempdir().unwrap();
    crate::message::ensure_dirs(dir.path()).unwrap();
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock.clone());
    let cryo_state = test_cryo_state();
    // Tick 1: park. Tick 2: client found dead -> released. Then the agent
    // process itself (still alive) sends and hibernates normally.
    let mut runtime = FakeSessionRuntime::new(
        vec![
            receive_wait_request(None),
            Ok(None),
            Ok(Some(crate::socket::Request::Send {
                text: "wrapping up".into(),
                question: false,
            })),
            Ok(Some(crate::socket::Request::Hibernate {
                complete: true,
                exit_code: 0,
                summary: None,
            })),
        ],
        (0..5)
            .map(|_| Ok(None))
            .chain([Ok(Some(ChildExitStatus { code: Some(0) }))])
            .collect(),
    );
    runtime.script_parked_disconnects(vec![false, true]);
    let mut effects = FakeSessionEffects::new();
    effects.push_scripted_claim(vec![]); // ReceiveWait's own claim: empty -> park
    effects.push_scripted_claim(vec![]); // tick 1 poll: still empty, client alive
    effects.push_scripted_claim(vec![FakeSessionEffects::make_inbox_message(
        "late.md",
        "anyone there?",
    )]); // must NOT be consumed: the wait was released before this tick

    let outcome = daemon
        .drive_active_session(
            &mut runtime,
            &mut effects,
            test_session_context(&cryo_state, 60, clock.monotonic_now()),
            begin_test_logger(dir.path()),
        )
        .unwrap();

    assert_eq!(outcome, SessionLoopOutcome::PlanComplete);
    assert!(!runtime.parked());
    // The inbox batch scripted for after the disconnect stayed unclaimed.
    assert_eq!(
        effects.scripted_claims.len(),
        1,
        "batch must stay unclaimed"
    );
    let responses = runtime.responses();
    assert!(
        responses.iter().all(|(_, m)| !m.contains("anyone there?")),
        "nothing may be delivered to the dead client: {responses:?}"
    );
    let log = fs::read_to_string(dir.path().join("cryo.log")).unwrap();
    assert!(log.contains("wait: parked"), "{log}");
    assert!(log.contains("wait: client disconnected"), "{log}");
    assert!(!log.contains("wait: delivered"), "{log}");
}

#[test]
fn test_receive_wait_can_repark_after_parked_client_died() {
    // After a dead parked client is reclaimed, the agent must be able to run
    // `receive --wait` again in the same session (regression: the stale slot
    // used to refuse every later wait with "already parked").
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
            receive_wait_request(None), // parks
            Ok(None),                   // tick where the client dies
            receive_wait_request(None), // re-park must be allowed
            Ok(Some(crate::socket::Request::Send {
                text: "round 2 ack".into(),
                question: false,
            })),
            Ok(Some(crate::socket::Request::Hibernate {
                complete: true,
                exit_code: 0,
                summary: None,
            })),
        ],
        (0..6)
            .map(|_| Ok(None))
            .chain([Ok(Some(ChildExitStatus { code: Some(0) }))])
            .collect(),
    );
    runtime.script_parked_disconnects(vec![false, true]);
    let mut effects = FakeSessionEffects::new();
    effects.push_scripted_claim(vec![]); // wait #1 claim: empty -> park
    effects.push_scripted_claim(vec![]); // tick 1 poll: empty, client alive
    effects.push_scripted_claim(vec![]); // wait #2 claim: empty -> park again
    effects.push_scripted_claim(vec![FakeSessionEffects::make_inbox_message(
        "r2.md",
        "second round",
    )]); // delivered to the live second wait

    let outcome = daemon
        .drive_active_session(
            &mut runtime,
            &mut effects,
            test_session_context(&cryo_state, 60, clock.monotonic_now()),
            begin_test_logger(dir.path()),
        )
        .unwrap();

    assert_eq!(outcome, SessionLoopOutcome::PlanComplete);
    let responses = runtime.responses();
    assert!(
        !responses.iter().any(|(_, m)| m.contains("already parked")),
        "second wait must not be refused: {responses:?}"
    );
    assert!(
        responses[0].0 && responses[0].1.contains("second round"),
        "{responses:?}"
    );
    let log = fs::read_to_string(dir.path().join("cryo.log")).unwrap();
    assert_eq!(log.matches("wait: parked").count(), 2, "{log}");
    assert!(log.contains("wait: client disconnected"), "{log}");
    assert!(
        log.contains("wait: delivered 1 message(s) [r2.md]"),
        "{log}"
    );
}

#[test]
fn test_event_loop_bootstrap_workless_wake_still_runs_session() {
    // Only *scheduled* wakes are demand-driven. The bootstrap wake after
    // `cryo start` is explicit operator demand and must run even with no
    // due TODO and an empty inbox (the chamber's first orientation session).
    let dir = tempfile::tempdir().unwrap();
    crate::message::ensure_dirs(dir.path()).unwrap();
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let launcher = Arc::new(ScriptedSessionLauncher::new(vec![
        SessionLoopOutcome::Hibernate,
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
    // No TODOs, empty inbox: the initial session is a workless wake.
    let bootstrap = DaemonBootstrapState {
        next_wake: None,
        run_now: true,
        watch_dirs: Vec::new(),
    };
    let config: CryoConfig = toml::from_str(r#"agent = "opencode""#).unwrap();
    let (tx, rx) = mpsc::channel::<DaemonEvent>();
    drop(tx);

    daemon
        .run_event_loop(&config, &mut cryo_state, bootstrap, &server, &rx)
        .unwrap();

    assert_eq!(
        launcher.session_numbers(),
        vec![1],
        "the initial workless wake must still run a session"
    );
}

#[test]
fn test_stale_scheduled_wake_with_no_due_work_skips_session_and_resyncs() {
    // The April 2026 runaway signature: the in-memory `next_wake` says "due"
    // while todo.json disagrees. A scheduled wake must derive its decision
    // from disk — claim a due TODO or find inbox files — not from the cache;
    // with neither, no session may spawn.
    let dir = tempfile::tempdir().unwrap();
    crate::message::ensure_dirs(dir.path()).unwrap();
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let launcher = Arc::new(ScriptedSessionLauncher::new(vec![
        SessionLoopOutcome::Hibernate,
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
    // Disk truth: the only pending TODO is tomorrow. Cache: an hour overdue.
    seed_todo_at(dir.path(), "tomorrow's work", "2026-03-02T09:00");
    let stale = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(11, 0, 0)
        .unwrap();
    let bootstrap = DaemonBootstrapState {
        next_wake: Some(stale),
        run_now: false,
        watch_dirs: Vec::new(),
    };
    let config: CryoConfig = toml::from_str(r#"agent = "opencode""#).unwrap();
    let (tx, rx) = mpsc::channel::<DaemonEvent>();
    // The stale cache fires WakeFromSchedule immediately; after the daemon
    // resyncs, the loop idles until this shutdown event ends the test.
    let stopper = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(700));
        let _ = tx.send(DaemonEvent::Shutdown);
    });

    daemon
        .run_event_loop(&config, &mut cryo_state, bootstrap, &server, &rx)
        .unwrap();
    stopper.join().unwrap();

    assert!(
        launcher.session_numbers().is_empty(),
        "a scheduled wake with no due TODO and an empty inbox must not spawn a session: {:?}",
        launcher.session_numbers()
    );
}

#[test]
fn test_scheduled_wake_with_unreadable_todos_keeps_retrying_until_healed() {
    // Codex review finding: mapping a claim *error* to "0 claimed" made the
    // demand-driven skip resync against the same unreadable todo.json,
    // getting None and silently parking the chamber forever. A claim error
    // must instead keep the stale wake armed and retry on a paced cadence,
    // so the chamber self-heals the moment todo.json is readable again.
    let dir = tempfile::tempdir().unwrap();
    crate::message::ensure_dirs(dir.path()).unwrap();
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let launcher = Arc::new(ScriptedSessionLauncher::new(vec![
        SessionLoopOutcome::PlanComplete,
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
    // todo.json is corrupt at wake time; the cached wake is already due.
    std::fs::write(dir.path().join("todo.json"), "{ not json ]").unwrap();
    let stale = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(11, 0, 0)
        .unwrap();
    let bootstrap = DaemonBootstrapState {
        next_wake: Some(stale),
        run_now: false,
        watch_dirs: Vec::new(),
    };
    let config: CryoConfig = toml::from_str(r#"agent = "opencode""#).unwrap();
    let (tx, rx) = mpsc::channel::<DaemonEvent>();
    let todo_path = dir.path().join("todo.json");
    let healer = std::thread::spawn(move || {
        // Heal the file while the daemon is mid-retry; a due pending TODO
        // appears. (Far-past `at` so it stays due however far the virtual
        // clock has advanced across paced retries.) Atomic rename: the
        // daemon reads concurrently and must never observe a half-written
        // file.
        std::thread::sleep(Duration::from_millis(400));
        let tmp = todo_path.with_extension("json.tmp");
        std::fs::write(
            &tmp,
            r#"[{"id":1,"text":"recovered work","done":false,"claimed":false,"at":"2026-01-01T00:00","created":"2026-01-01T00:00:00"}]"#,
        )
        .unwrap();
        std::fs::rename(&tmp, &todo_path).unwrap();
        // Safety net: end the loop even if the fix regresses and no session
        // (with its PlanComplete exit) ever runs.
        std::thread::sleep(Duration::from_millis(1500));
        let _ = tx.send(DaemonEvent::Shutdown);
    });

    daemon
        .run_event_loop(&config, &mut cryo_state, bootstrap, &server, &rx)
        .unwrap();
    healer.join().unwrap();

    assert_eq!(
        launcher.session_numbers(),
        vec![1],
        "the chamber must recover and run the due TODO once todo.json heals"
    );
}

#[test]
fn test_parked_client_death_pauses_session_deadline_instead_of_resetting_it() {
    // Codex review finding: granting a fresh session budget on every
    // disconnect-reclaim would let an agent under a too-short shell timeout
    // extend one session forever (park -> shell kills client -> reclaim ->
    // fresh budget -> repeat). The documented semantics are "the clock
    // pauses while you wait": after a reclaim the agent resumes with the
    // budget it had when it parked.
    let dir = tempfile::tempdir().unwrap();
    crate::message::ensure_dirs(dir.path()).unwrap();
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock.clone());
    let cryo_state = test_cryo_state();
    // 1 s session budget = 10 ticks. Burn 9 ticks, park on tick 10 (0.1 s of
    // budget left), stay parked ~1.8 s, then the client dies. The agent then
    // idles; its Hibernate would arrive 5 ticks after the reclaim — but with
    // pause semantics only ~1 tick of budget remains, so the session must
    // time out first. (A reset would grant 10 fresh ticks and let the
    // hibernate land.)
    let mut requests: Vec<anyhow::Result<Option<crate::socket::Request>>> =
        (0..9).map(|_| Ok(None)).collect();
    requests.push(receive_wait_request(Some(3600)));
    requests.extend((0..22).map(|_| Ok(None)));
    requests.push(Ok(Some(crate::socket::Request::Hibernate {
        complete: true,
        exit_code: 0,
        summary: None,
    })));
    let mut waits: Vec<std::io::Result<Option<ChildExitStatus>>> =
        (0..33).map(|_| Ok(None)).collect();
    waits.push(Ok(Some(ChildExitStatus { code: Some(0) })));
    let mut runtime = FakeSessionRuntime::new(requests, waits);
    let mut disconnects = vec![false; 18];
    disconnects.push(true);
    runtime.script_parked_disconnects(disconnects);
    let mut effects = FakeSessionEffects::new();

    let outcome = daemon
        .drive_active_session(
            &mut runtime,
            &mut effects,
            test_session_context(&cryo_state, 1, clock.monotonic_now()),
            begin_test_logger(dir.path()),
        )
        .unwrap();

    assert_ne!(
        outcome,
        SessionLoopOutcome::PlanComplete,
        "the session must not gain a fresh budget from a dead wait client"
    );
    assert!(runtime.terminated(), "session should have been timed out");
    let log = fs::read_to_string(dir.path().join("cryo.log")).unwrap();
    assert!(log.contains("wait: client disconnected"), "{log}");
}

#[cfg(not(target_os = "macos"))]
#[test]
fn inbox_watcher_fires_on_atomic_tmp_rename() {
    // Atomic writers create `.{name}.tmp` and rename it into place; the
    // rename produces no Create event, only Modify(Name(...)). inotify
    // delivers both sides in real time, so pre-fix the only signal was
    // the tmp create, which lost the race against should_ignore_inbox_
    // wake's empty-inbox check. Post-fix the paired rename reports the
    // final path deterministically.
    //
    // Gated off macOS: FSEvents delivers events post-hoc (after the
    // rename has landed), so the pre-existing tmp create event already
    // wakes the daemon reliably there — and its coalescing of quick
    // create+rename sequences does not guarantee any event for the
    // final path within a test-sized window.
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    fs::create_dir(&inbox).unwrap();
    let (tx, rx) = mpsc::channel();
    let _watcher = InboxWatcher::start(std::slice::from_ref(&inbox), tx).unwrap();

    let final_path = inbox.join("2026-08-09T20-00-00_test_1.md");
    let tmp_path = inbox.join(".2026-08-09T20-00-00_test_1.md.tmp");
    fs::write(&tmp_path, "body").unwrap();
    fs::rename(&tmp_path, &final_path).unwrap();

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match rx.recv_timeout(std::time::Duration::from_millis(200)) {
            Ok(DaemonEvent::InboxChanged { paths }) => {
                // Pre-fix only the tmp create fired, which is the bug;
                // the paired rename must report the final path.
                if paths.iter().any(|p| p == &final_path) {
                    return; // the atomic write woke the daemon
                }
            }
            Ok(_) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => {
                assert!(
                    std::time::Instant::now() < deadline,
                    "no InboxChanged for the renamed-in file"
                );
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                panic!("watcher channel disconnected")
            }
        }
    }
}
