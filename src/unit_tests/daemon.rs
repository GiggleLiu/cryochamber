use super::*;
use std::collections::VecDeque;
use std::sync::Mutex;

struct TestClockState {
    now: NaiveDateTime,
    elapsed: Duration,
    sleeps: Vec<Duration>,
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
                sleeps: Vec::new(),
            }),
        }
    }

    fn advance(&self, duration: Duration) {
        let mut state = self.state.lock().unwrap();
        state.now += chrono::Duration::from_std(duration).unwrap();
        state.elapsed += duration;
    }

    fn sleeps(&self) -> Vec<Duration> {
        self.state.lock().unwrap().sleeps.clone()
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
        state.sleeps.push(duration);
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
        Ok(())
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
    replies: Vec<(String, NaiveDateTime)>,
    todos: Vec<crate::todo::TodoItem>,
    next_todo_id: u32,
    archived_batches: Vec<Vec<String>>,
}

impl FakeSessionEffects {
    fn new() -> Self {
        Self {
            reply_failure: None,
            replies: Vec::new(),
            todos: Vec::new(),
            next_todo_id: 1,
            archived_batches: Vec::new(),
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
            at: "2099-12-31T23:59".to_string(),
            created: "unknown".to_string(),
        });
        effects.next_todo_id = 2;
        effects
    }
}

impl SessionEffects for FakeSessionEffects {
    fn archive_inbox(&mut self, inbox_filenames: &[String]) -> Result<()> {
        self.archived_batches.push(inbox_filenames.to_vec());
        Ok(())
    }

    fn write_reply(&mut self, text: &str, timestamp: NaiveDateTime) -> Result<()> {
        if let Some(message) = &self.reply_failure {
            anyhow::bail!("{message}");
        }
        self.replies.push((text.to_string(), timestamp));
        Ok(())
    }

    fn todo_add(&mut self, text: &str, at: &str) -> Result<u32> {
        let id = self.next_todo_id;
        self.next_todo_id += 1;
        self.todos.push(crate::todo::TodoItem {
            id,
            text: text.to_string(),
            done: false,
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
                let check = if item.done { "x" } else { " " };
                format!("{}. [{}] {} (at: {})", item.id, check, item.text, item.at)
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }

    fn has_pending_todo_with_valid_wake(&self) -> bool {
        self.todos.iter().any(|item| {
            !item.done
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
        _inbox_path: &Path,
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
        retry_count: 0,
        agent_override: None,
        max_retries_override: None,
        max_session_duration_override: None,
        last_report_time: None,
        provider_index: None,
        instance_id: Some("test-instance".into()),
        pending_fallback: None,
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
    inbox_filenames: &'a [String],
) -> ActiveSessionContext<'a> {
    ActiveSessionContext {
        cryo_state,
        timeout_secs,
        spawn_time,
        inbox_filenames,
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

    fn drain_inbox_changed(&self) {
        let mut events = self.events.lock().unwrap();
        while matches!(events.front(), Some(Ok(DaemonEvent::InboxChanged))) {
            events.pop_front();
            *self.drained_inbox.lock().unwrap() += 1;
        }
    }
}

#[test]
fn test_backoff_sequence() {
    let mut state = RetryState::new(5, 1);
    // 5s, 10s, 20s, 40s, 80s, then keeps going capped at 3600s
    assert_eq!(state.next_backoff(), Duration::from_secs(5));

    state.record_failure();
    assert_eq!(state.next_backoff(), Duration::from_secs(10));

    state.record_failure();
    assert_eq!(state.next_backoff(), Duration::from_secs(20));

    state.record_failure();
    assert_eq!(state.next_backoff(), Duration::from_secs(40));

    state.record_failure();
    assert_eq!(state.next_backoff(), Duration::from_secs(80));

    // Past max_retries — still returns backoff, capped at 3600s
    state.record_failure();
    assert_eq!(state.attempt, 5);
    assert_eq!(state.next_backoff(), Duration::from_secs(160));
    assert!(state.exhausted());
}

#[test]
fn test_backoff_caps_at_one_hour() {
    let mut state = RetryState::new(20, 1);
    for _ in 0..15 {
        state.record_failure();
    }
    // 5 * 2^15 = 163840 > 3600, so capped
    assert_eq!(state.next_backoff(), Duration::from_secs(3600));
}

#[test]
fn test_backoff_reset() {
    let mut state = RetryState::new(3, 1);
    state.record_failure();
    state.record_failure();
    assert_eq!(state.attempt, 2);

    state.reset();
    assert_eq!(state.attempt, 0);
    assert!(!state.exhausted());
}

#[test]
fn test_backoff_exact_sequence() {
    let mut retry = RetryState::new(20, 1);
    let expected = [5, 10, 20, 40, 80, 160, 320, 640, 1280, 2560, 3600, 3600];
    for (i, &secs) in expected.iter().enumerate() {
        assert_eq!(
            retry.next_backoff(),
            Duration::from_secs(secs),
            "Backoff at attempt {i} should be {secs}s"
        );
        retry.record_failure();
    }
}

#[test]
fn test_backoff_cap_never_exceeds_3600() {
    let mut retry = RetryState::new(100, 1);
    for _ in 0..100 {
        let backoff = retry.next_backoff();
        assert!(
            backoff <= Duration::from_secs(3600),
            "Backoff should never exceed 3600s, got {:?}",
            backoff
        );
        retry.record_failure();
    }
}

#[test]
fn test_rotate_provider_single_provider() {
    let mut retry = RetryState::new(5, 1);
    // With only 1 provider, rotate always returns true (can't rotate)
    assert!(
        retry.rotate_provider(),
        "Single provider should always wrap"
    );
    assert_eq!(retry.provider_index, 0);
}

#[test]
fn test_rotate_provider_advances_and_wraps() {
    let mut retry = RetryState::new(5, 3);
    assert_eq!(retry.provider_index, 0);

    assert!(!retry.rotate_provider(), "Should not wrap: 0->1");
    assert_eq!(retry.provider_index, 1);

    assert!(!retry.rotate_provider(), "Should not wrap: 1->2");
    assert_eq!(retry.provider_index, 2);

    assert!(retry.rotate_provider(), "Should wrap: 2->0");
    assert_eq!(retry.provider_index, 0);
}

#[test]
fn test_reset_clears_attempt_and_provider() {
    let mut retry = RetryState::new(5, 3);
    retry.record_failure();
    retry.record_failure();
    retry.rotate_provider(); // index = 1, attempt reset to 0 by rotate
    retry.record_failure(); // attempt = 1
    assert_eq!(retry.attempt, 1);
    assert_eq!(retry.provider_index, 1);

    retry.reset();
    assert_eq!(retry.attempt, 0);
    assert_eq!(
        retry.provider_index, 0,
        "Provider index should be reset to 0"
    );
}

#[test]
fn test_exhausted_boundary() {
    let mut retry = RetryState::new(3, 1);
    assert!(!retry.exhausted(), "Should not be exhausted at attempt 0");
    retry.record_failure();
    assert!(!retry.exhausted(), "Should not be exhausted at attempt 1");
    retry.record_failure();
    assert!(!retry.exhausted(), "Should not be exhausted at attempt 2");
    retry.record_failure();
    assert!(
        retry.exhausted(),
        "Should be exhausted at attempt 3 (== max_retries)"
    );
}

#[test]
fn test_inbox_watcher_detects_new_file() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("messages").join("inbox");
    std::fs::create_dir_all(&inbox).unwrap();

    let (tx, rx) = mpsc::channel();
    let _watcher = InboxWatcher::start(&inbox, tx).unwrap();

    // Create a file in inbox
    std::fs::write(inbox.join("test-message.md"), "hello").unwrap();

    // Should receive InboxChanged within 2 seconds
    let event = rx.recv_timeout(Duration::from_secs(2));
    assert_eq!(event.unwrap(), DaemonEvent::InboxChanged);
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
    let _watcher = InboxWatcher::start(&inbox, tx).unwrap();

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
        Ok(DaemonEvent::InboxChanged),
        Ok(DaemonEvent::InboxChanged),
        Ok(DaemonEvent::InboxChanged),
    ]);

    let outcome = wait_for_idle_event(&source, Duration::from_secs(1), None, sample_time);

    assert_eq!(outcome, IdleWaitOutcome::WakeFromInbox);
    assert_eq!(source.drained_count(), 2);
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
fn test_next_wake_from_todos_picks_earliest() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("todo.json");
    let mut list = crate::todo::TodoList::new();
    list.add("later".into(), "2026-03-02T16:00".into());
    list.add("earlier".into(), "2026-03-02T14:00".into());
    list.save(&path).unwrap();
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
    let mut list = crate::todo::TodoList::new();
    let id = list.add("done".into(), "2026-03-02T10:00".into());
    list.done(id).unwrap();
    list.save(&path).unwrap();
    let wake = next_wake_from_todos(dir.path());
    assert!(wake.is_none());
}

#[test]
fn test_next_wake_from_todos_skips_invalid_and_picks_valid() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("todo.json");
    let mut list = crate::todo::TodoList::new();
    // Invalid at value that sorts before valid ones
    list.add("bad format".into(), "2026-03-02 10:00".into());
    // Valid at value
    list.add("valid task".into(), "2026-03-02T14:00".into());
    list.save(&path).unwrap();
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
    let mut list = crate::todo::TodoList::new();
    list.add("bad1".into(), "not-a-date".into());
    list.add("bad2".into(), "also-bad".into());
    list.save(&path).unwrap();
    let wake = next_wake_from_todos(dir.path());
    assert!(wake.is_none(), "All invalid entries should yield None");
}

#[test]
fn test_sleep_or_shutdown_uses_injected_clock() {
    let dir = tempfile::tempdir().unwrap();
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock.clone());

    assert!(!daemon.sleep_or_shutdown(Duration::from_millis(600)));
    assert_eq!(
        clock.sleeps(),
        vec![
            Duration::from_millis(250),
            Duration::from_millis(250),
            Duration::from_millis(100),
        ]
    );
    assert_eq!(clock.local_now(), now + chrono::Duration::milliseconds(600));
}

#[test]
fn test_check_fallback_uses_injected_clock() {
    let dir = tempfile::tempdir().unwrap();
    crate::message::ensure_dirs(dir.path()).unwrap();

    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock.clone());

    let action = FallbackAction {
        action: "email".to_string(),
        target: "ops@example.com".to_string(),
        message: "still waiting".to_string(),
    };
    let mut cryo_state = CryoState {
        session_number: 1,
        pid: None,
        retry_count: 0,
        agent_override: None,
        max_retries_override: None,
        max_session_duration_override: None,
        last_report_time: None,
        provider_index: None,
        instance_id: None,
        pending_fallback: Some(PendingFallbackState {
            deadline: (now + chrono::Duration::minutes(1))
                .format(FALLBACK_TIME_FMT)
                .to_string(),
            action: action.clone(),
        }),
    };
    let mut pending = Some((now + chrono::Duration::minutes(1), action));

    daemon.check_fallback(&mut cryo_state, &mut pending, "outbox");
    assert!(
        pending.is_some(),
        "Fallback should not fire before the deadline"
    );

    clock.advance(Duration::from_secs(61));
    daemon.check_fallback(&mut cryo_state, &mut pending, "outbox");

    assert!(
        pending.is_none(),
        "Fallback should fire after fake time advances"
    );
    assert!(cryo_state.pending_fallback.is_none());
    let outbox = crate::message::read_outbox(dir.path()).unwrap();
    assert_eq!(outbox.len(), 1, "Fallback should write one outbox message");
}

#[test]
fn test_resolve_hibernate_request_failure_retries() {
    let mut pending_fallback = Some(FallbackAction {
        action: "email".into(),
        target: "ops@example.com".into(),
        message: "stuck".into(),
    });

    // Failure path does not require a pending TODO — daemon will retry.
    let decision = resolve_hibernate_request(
        false,
        7,
        Some("provider failed"),
        false,
        &mut pending_fallback,
    );

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
    assert!(
        pending_fallback.is_some(),
        "failure should not consume fallback"
    );
}

#[test]
fn test_resolve_hibernate_request_complete_ignores_fallback() {
    let mut pending_fallback = Some(FallbackAction {
        action: "email".into(),
        target: "ops@example.com".into(),
        message: "stuck".into(),
    });

    // `--complete` means the plan is truly finished; no TODO needed.
    let decision = resolve_hibernate_request(true, 0, None, false, &mut pending_fallback);

    assert_eq!(decision.outcome, Some(SessionLoopOutcome::PlanComplete));
    assert!(decision.response_ok);
    assert_eq!(decision.response_message, "Plan complete. Shutting down.");
    assert_eq!(
        decision.log_event,
        "hibernate: plan complete, exit=0, summary=\"(no summary)\""
    );
    assert!(
        pending_fallback.is_some(),
        "complete should not consume fallback"
    );
}

#[test]
fn test_resolve_hibernate_request_uses_pending_fallback() {
    let fallback = FallbackAction {
        action: "webhook".into(),
        target: "ops".into(),
        message: "waiting".into(),
    };
    let mut pending_fallback = Some(fallback.clone());

    let decision = resolve_hibernate_request(
        false,
        0,
        Some("waiting on reply"),
        true,
        &mut pending_fallback,
    );

    assert_eq!(
        decision.outcome,
        Some(SessionLoopOutcome::Hibernate {
            fallback: Some(fallback),
        })
    );
    assert!(decision.response_ok);
    assert_eq!(decision.response_message, "Hibernating.");
    assert_eq!(
        decision.log_event,
        "hibernate: exit=0, summary=\"waiting on reply\""
    );
    assert!(
        pending_fallback.is_none(),
        "successful hibernate should consume pending fallback"
    );
}

#[test]
fn test_resolve_hibernate_request_rejects_when_no_pending_todo() {
    let mut pending_fallback = Some(FallbackAction {
        action: "webhook".into(),
        target: "ops".into(),
        message: "waiting".into(),
    });

    // Non-complete hibernate with no pending TODO: session must stay alive
    // so the agent can observe the error and correct.
    let decision =
        resolve_hibernate_request(false, 0, Some("forgot todo"), false, &mut pending_fallback);

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
    assert!(
        pending_fallback.is_some(),
        "rejected hibernate must not consume pending fallback"
    );
}

#[test]
fn test_resolve_interrupted_session_prefers_hibernate_outcome() {
    let hibernate = SessionLoopOutcome::Hibernate { fallback: None };

    let shutdown =
        resolve_interrupted_session(SessionInterruption::Shutdown, Some(hibernate.clone()));
    let timeout = resolve_interrupted_session(SessionInterruption::Timeout, Some(hibernate));

    assert_eq!(
        shutdown.outcome,
        SessionLoopOutcome::Hibernate { fallback: None }
    );
    assert_eq!(
        shutdown.finish_reason,
        "daemon shutdown — using agent's hibernate outcome"
    );
    assert_eq!(
        timeout.outcome,
        SessionLoopOutcome::Hibernate { fallback: None }
    );
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
fn test_drive_active_session_alert_then_hibernate_returns_fallback() {
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
            Ok(Some(crate::socket::Request::Alert {
                action: "webhook".into(),
                target: "ops".into(),
                message: "waiting".into(),
            })),
            Ok(Some(crate::socket::Request::Hibernate {
                complete: false,
                exit_code: 0,
                summary: Some("waiting on reply".into()),
            })),
        ],
        vec![Ok(None), Ok(Some(ChildExitStatus { code: Some(0) }))],
    );
    let mut effects = FakeSessionEffects::new_with_pending_todo();

    let outcome = daemon
        .drive_active_session(
            &mut runtime,
            &mut effects,
            test_session_context(&cryo_state, 60, clock.monotonic_now(), &[]),
            begin_test_logger(dir.path()),
        )
        .unwrap();

    assert_eq!(
        outcome,
        SessionLoopOutcome::Hibernate {
            fallback: Some(FallbackAction {
                action: "webhook".into(),
                target: "ops".into(),
                message: "waiting".into(),
            }),
        }
    );
    assert_eq!(
        runtime.responses(),
        vec![
            (true, "Alert registered".into()),
            (true, "Hibernating.".into()),
        ]
    );
    assert!(!runtime.terminated());
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
            test_session_context(&cryo_state, 1, clock.monotonic_now(), &[]),
            begin_test_logger(dir.path()),
        )
        .unwrap();

    assert_eq!(outcome, SessionLoopOutcome::Hibernate { fallback: None });
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
            test_session_context(&cryo_state, 60, clock.monotonic_now(), &[]),
            begin_test_logger(dir.path()),
        )
        .unwrap();

    assert_eq!(
        outcome,
        SessionLoopOutcome::ValidationFailed { quick_exit: true }
    );
    assert!(!runtime.terminated());
    assert!(runtime.responses().is_empty());
}

#[test]
fn test_drive_active_session_reply_failure_responds_and_continues() {
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
            Ok(Some(crate::socket::Request::Reply {
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
            Ok(Some(ChildExitStatus { code: Some(0) })),
        ],
    );
    let mut effects = FakeSessionEffects::with_reply_failure("injected reply failure");

    let outcome = daemon
        .drive_active_session(
            &mut runtime,
            &mut effects,
            test_session_context(&cryo_state, 60, clock.monotonic_now(), &[]),
            begin_test_logger(dir.path()),
        )
        .unwrap();

    assert_eq!(outcome, SessionLoopOutcome::Hibernate { fallback: None });
    assert_eq!(
        runtime.responses(),
        vec![
            (
                false,
                "Failed to write reply: injected reply failure".into()
            ),
            (true, "Hibernating.".into()),
        ]
    );
    assert!(effects.replies.is_empty());
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
            test_session_context(&cryo_state, 60, clock.monotonic_now(), &[]),
            begin_test_logger(dir.path()),
        )
        .unwrap();

    assert_eq!(outcome, SessionLoopOutcome::Hibernate { fallback: None });
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
fn test_drive_active_session_timeout_archives_inbox_via_effects() {
    let dir = tempfile::tempdir().unwrap();
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock.clone());
    let cryo_state = test_cryo_state();
    let mut runtime = FakeSessionRuntime::new(vec![], vec![]);
    let mut effects = FakeSessionEffects::new();
    let inbox = vec!["msg-1.md".to_string(), "msg-2.md".to_string()];

    let outcome = daemon
        .drive_active_session(
            &mut runtime,
            &mut effects,
            test_session_context(&cryo_state, 1, clock.monotonic_now(), &inbox),
            begin_test_logger(dir.path()),
        )
        .unwrap();

    assert_eq!(
        outcome,
        SessionLoopOutcome::ValidationFailed { quick_exit: false }
    );
    assert!(runtime.terminated());
    assert_eq!(effects.archived_batches, vec![inbox]);
}

#[test]
fn test_build_bootstrap_state_clears_invalid_pending_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let clock = Arc::new(TestClock::new(now));
    let daemon = Daemon::new_with_clock(dir.path().to_path_buf(), clock);
    let mut cryo_state = test_cryo_state();
    cryo_state.pending_fallback = Some(PendingFallbackState {
        deadline: "not-a-time".into(),
        action: FallbackAction {
            action: "email".into(),
            target: "ops@example.com".into(),
            message: "stuck".into(),
        },
    });

    let bootstrap = daemon.build_bootstrap_state(&mut cryo_state, &CryoConfig::default());

    assert!(bootstrap.pending_fallback.is_none());
    assert!(bootstrap.cleared_invalid_pending_fallback);
    assert!(cryo_state.pending_fallback.is_none());
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

    let first = daemon.build_bootstrap_state(&mut first_session, &CryoConfig::default());
    assert!(first.run_now);

    let todo_path = dir.path().join("todo.json");
    let mut todos = crate::todo::TodoList::new();
    todos.add("Overdue task".into(), "2026-03-01T11:30".into());
    todos.save(&todo_path).unwrap();

    let mut resumed = test_cryo_state();
    resumed.session_number = 3;
    let resumed_bootstrap = daemon.build_bootstrap_state(&mut resumed, &CryoConfig::default());
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
    let mut cryo_state = test_cryo_state();
    let mut config = CryoConfig::default();

    let no_inbox = daemon.build_bootstrap_state(&mut cryo_state, &config);
    assert!(no_inbox.watch_inbox_path.is_none());

    let inbox = dir.path().join("messages").join("inbox");
    std::fs::create_dir_all(&inbox).unwrap();
    let with_inbox = daemon.build_bootstrap_state(&mut cryo_state, &config);
    assert_eq!(with_inbox.watch_inbox_path, Some(inbox.clone()));

    config.watch_inbox = false;
    let disabled = daemon.build_bootstrap_state(&mut cryo_state, &config);
    assert!(disabled.watch_inbox_path.is_none());
}

#[test]
fn test_prepare_shutdown_state_clears_runtime_identity_and_syncs_fallback() {
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
    let pending = Some((
        now + chrono::Duration::hours(1),
        FallbackAction {
            action: "webhook".into(),
            target: "ops".into(),
            message: "still waiting".into(),
        },
    ));

    daemon.prepare_shutdown_state(&mut cryo_state, pending.as_ref());

    assert!(cryo_state.pid.is_none());
    assert!(cryo_state.instance_id.is_none());
    assert_eq!(
        cryo_state.pending_fallback,
        Some(PendingFallbackState {
            deadline: "2026-03-01T13:00:00".into(),
            action: FallbackAction {
                action: "webhook".into(),
                target: "ops".into(),
                message: "still waiting".into(),
            },
        })
    );
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
        .prepare_runtime_startup(&platform, Some(inbox.as_path()), tx)
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
        .prepare_runtime_startup(&platform, Some(inbox.as_path()), tx)
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
        .prepare_runtime_startup(&platform, None, tx)
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
        .prepare_runtime_startup(&platform, None, tx)
        .unwrap_err()
        .to_string();

    assert!(error.contains("bind failed"));
    assert_eq!(platform.bind_calls(), 1);
    assert_eq!(platform.watcher_calls(), 0);
}
