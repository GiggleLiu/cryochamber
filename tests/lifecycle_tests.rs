use cryochamber::config::{self, CryoConfig};
use cryochamber::lifecycle::{prepare_start, StartOptions};

fn init_chamber(dir: &std::path::Path) {
    config::save_config(&config::config_path(dir), &CryoConfig::default()).unwrap();
    std::fs::write(dir.join("plan.md"), "test plan").unwrap();
}

fn test_state() -> cryochamber::state::CryoState {
    cryochamber::state::CryoState {
        session_number: 7,
        pid: Some(999_999),
        agent_override: Some("codex".to_string()),
        max_session_duration_override: Some(120),
        last_report_time: Some("2026-04-22T10:00:00".to_string()),
        provider_index: Some(2),
        instance_id: Some("instance-1".to_string()),
        session_active: false,
        previous_session_crashed: true,
    }
}

#[test]
fn prepare_start_rejects_missing_config() {
    let dir = tempfile::tempdir().unwrap();

    let err = prepare_start(dir.path(), StartOptions::default()).unwrap_err();

    assert!(err.to_string().contains("cryo.toml"));
}

#[test]
fn prepare_start_uses_cli_overrides_in_state_and_effective_agent() {
    let dir = tempfile::tempdir().unwrap();
    init_chamber(dir.path());

    let prepared = prepare_start(
        dir.path(),
        StartOptions {
            agent_override: Some("mock".to_string()),
            max_session_duration_override: Some(120),
        },
    )
    .unwrap();

    assert_eq!(prepared.effective_agent, "mock");
    assert_eq!(prepared.state.session_number, 0);
    assert_eq!(prepared.state.pid, None);
    assert_eq!(prepared.state.agent_override.as_deref(), Some("mock"));
    assert_eq!(prepared.state.max_session_duration_override, Some(120));
}

#[test]
fn prepare_start_rejects_locked_state() {
    let dir = tempfile::tempdir().unwrap();
    init_chamber(dir.path());
    let state = cryochamber::state::CryoState {
        session_number: 7,
        pid: Some(std::process::id()),
        agent_override: None,
        max_session_duration_override: None,
        last_report_time: None,
        provider_index: None,
        instance_id: None,
        session_active: false,
        previous_session_crashed: false,
    };
    cryochamber::state::save_state(&cryochamber::state::state_path(dir.path()), &state).unwrap();

    let err = prepare_start(dir.path(), StartOptions::default()).unwrap_err();

    assert!(err.to_string().contains("already running"));
}

#[test]
fn stop_chamber_clears_pid_and_preserves_runtime_state() {
    let dir = tempfile::tempdir().unwrap();
    init_chamber(dir.path());
    let state_path = cryochamber::state::state_path(dir.path());
    cryochamber::state::save_state(&state_path, &test_state()).unwrap();

    cryochamber::lifecycle::stop_chamber(dir.path()).unwrap();

    let stopped = cryochamber::state::load_state(&state_path)
        .unwrap()
        .expect("timer.json should be preserved by stop");
    assert_eq!(stopped.pid, None);
    assert_eq!(stopped.session_number, 7);
    assert_eq!(stopped.agent_override.as_deref(), Some("codex"));
    assert_eq!(stopped.max_session_duration_override, Some(120));
    assert_eq!(
        stopped.last_report_time.as_deref(),
        Some("2026-04-22T10:00:00")
    );
    assert_eq!(stopped.provider_index, Some(2));
    assert_eq!(stopped.instance_id.as_deref(), Some("instance-1"));
    assert!(stopped.previous_session_crashed);
}

#[test]
fn archive_runtime_moves_resettable_files_but_preserves_sync_config() {
    let dir = tempfile::tempdir().unwrap();
    init_chamber(dir.path());
    std::fs::write(dir.path().join("cryo.log"), "log").unwrap();
    std::fs::write(dir.path().join("cryo-agent.log"), "agent log").unwrap();
    std::fs::write(dir.path().join("todo.json"), "[]").unwrap();
    std::fs::write(dir.path().join("NOTES.md"), "notes").unwrap();
    cryochamber::state::save_state(&cryochamber::state::state_path(dir.path()), &test_state())
        .unwrap();
    cryochamber::message::ensure_dirs(dir.path()).unwrap();
    std::fs::write(
        dir.path().join("messages").join("inbox").join("hello.md"),
        "message",
    )
    .unwrap();
    std::fs::write(dir.path().join("gh-sync.json"), "{}").unwrap();
    std::fs::write(dir.path().join("zulip-sync.json"), "{}").unwrap();

    let archive = cryochamber::lifecycle::archive_runtime(dir.path()).unwrap();

    for name in [
        "cryo.log",
        "cryo-agent.log",
        "todo.json",
        "NOTES.md",
        "timer.json",
        "messages/inbox/hello.md",
    ] {
        assert!(archive.join(name).exists(), "{name} should be archived");
        assert!(
            !dir.path().join(name).exists(),
            "{name} should be moved out of the chamber root"
        );
    }
    assert!(dir.path().join("gh-sync.json").exists());
    assert!(dir.path().join("zulip-sync.json").exists());
}

#[test]
fn reset_chamber_archives_runtime_and_recreates_message_dirs() {
    let dir = tempfile::tempdir().unwrap();
    init_chamber(dir.path());
    std::fs::write(dir.path().join("cryo.log"), "log").unwrap();
    cryochamber::state::save_state(&cryochamber::state::state_path(dir.path()), &test_state())
        .unwrap();
    cryochamber::message::ensure_dirs(dir.path()).unwrap();

    let archive = cryochamber::lifecycle::reset_chamber(dir.path()).unwrap();

    assert!(archive.join("cryo.log").exists());
    assert!(archive.join("timer.json").exists());
    assert!(!dir.path().join("timer.json").exists());
    assert!(dir.path().join("messages").join("inbox").is_dir());
    assert!(dir.path().join("messages").join("outbox").is_dir());
}
