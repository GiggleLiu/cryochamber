use super::*;

#[test]
fn start_chamber_rejects_missing_cryo_toml() {
    let dir = tempfile::tempdir().unwrap();
    let err = start_chamber(dir.path()).unwrap_err();
    assert!(err.to_string().contains("no cryo.toml"));
}

#[test]
fn start_chamber_rejects_missing_plan_md() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = crate::config::CryoConfig::default();
    crate::config::save_config(&crate::config::config_path(dir.path()), &cfg).unwrap();
    let err = start_chamber(dir.path()).unwrap_err();
    assert!(err.to_string().contains("plan.md"));
}

#[test]
fn stop_chamber_is_idempotent_on_nothing_running() {
    let dir = tempfile::tempdir().unwrap();
    stop_chamber(dir.path()).unwrap();
}

#[test]
fn wait_for_live_daemon_times_out_when_no_daemon_registers() {
    let dir = tempfile::tempdir().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(300);
    let err = wait_for_live_daemon_until(dir.path(), deadline).unwrap_err();
    assert!(err.to_string().contains("Daemon did not start"));
}

#[test]
fn wait_for_live_daemon_times_out_with_unlocked_state() {
    let dir = tempfile::tempdir().unwrap();
    let st = crate::state::CryoState {
        session_number: 0,
        pid: None,
        retry_count: 0,
        agent_override: None,
        max_session_duration_override: None,
        last_report_time: None,
        provider_index: None,
        instance_id: None,
        session_active: false,
        previous_session_crashed: false,
    };
    crate::state::save_state(&crate::state::state_path(dir.path()), &st).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(300);
    let err = wait_for_live_daemon_until(dir.path(), deadline).unwrap_err();
    assert!(err.to_string().contains("Daemon did not start"));
}

#[test]
fn archive_logs_moves_existing_logs_and_skips_missing() {
    let dir = tempfile::tempdir().unwrap();
    let cryo_log = dir.path().join("cryo.log");
    std::fs::write(&cryo_log, b"old session data").unwrap();
    // cryo-agent.log intentionally absent

    let archive = archive_logs(dir.path()).unwrap();
    assert!(archive.starts_with(dir.path().join("history")));
    assert!(archive.join("cryo.log").exists());
    assert!(!archive.join("cryo-agent.log").exists());
    assert!(!cryo_log.exists(), "original cryo.log should be moved");
    assert_eq!(
        std::fs::read_to_string(archive.join("cryo.log")).unwrap(),
        "old session data"
    );
}

#[test]
fn archive_logs_creates_history_dir_when_no_logs_present() {
    let dir = tempfile::tempdir().unwrap();
    let archive = archive_logs(dir.path()).unwrap();
    assert!(archive.is_dir());
    assert!(dir.path().join("history").is_dir());
}

#[test]
fn archive_runtime_moves_todo_notes_and_messages() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("cryo.log"), "log").unwrap();
    std::fs::write(dir.path().join("cryo-agent.log"), "agent log").unwrap();
    std::fs::write(dir.path().join("todo.json"), "[]").unwrap();
    std::fs::write(dir.path().join("NOTES.md"), "notes").unwrap();
    std::fs::write(dir.path().join("timer.json"), "{}").unwrap();
    crate::message::ensure_dirs(dir.path()).unwrap();
    std::fs::write(
        dir.path().join("messages").join("inbox").join("hello.md"),
        "message",
    )
    .unwrap();

    let archive = archive_runtime(dir.path()).unwrap();

    for name in [
        "cryo.log",
        "cryo-agent.log",
        "todo.json",
        "NOTES.md",
        "timer.json",
        "messages/inbox/hello.md",
    ] {
        assert!(
            archive.join(name).exists(),
            "{name} should be archived under {}",
            archive.display()
        );
        assert!(
            !dir.path().join(name).exists(),
            "{name} should be removed from the chamber root"
        );
    }
}

#[test]
fn reset_chamber_leaves_chamber_stopped_with_fresh_messages_dir() {
    // Reset must not auto-start the daemon (previously confusing UX: the
    // operator pressed reset and was left staring at only a Stop button).
    // Reset must also re-create `messages/` so a still-running sync daemon
    // (e.g. cryo-zulip) keeps delivering into the live directory instead of
    // the archived one.
    let dir = tempfile::tempdir().unwrap();
    let cfg = crate::config::CryoConfig::default();
    crate::config::save_config(&crate::config::config_path(dir.path()), &cfg).unwrap();
    std::fs::write(dir.path().join("plan.md"), "plan").unwrap();
    std::fs::write(dir.path().join("cryo.log"), "old log").unwrap();
    std::fs::write(dir.path().join("timer.json"), "{\"session_number\":5}").unwrap();
    crate::message::ensure_dirs(dir.path()).unwrap();

    let archive = reset_chamber(dir.path()).unwrap();

    assert!(archive.join("cryo.log").exists(), "logs should be archived");
    assert!(
        archive.join("timer.json").exists(),
        "timer.json should be archived so session counter starts fresh"
    );
    assert!(
        !dir.path().join("timer.json").exists(),
        "reset should leave chamber stopped (no timer.json until next start)"
    );
    assert!(
        dir.path().join("messages").join("inbox").is_dir(),
        "reset should re-create messages/inbox for in-flight sync delivery"
    );
    assert!(
        dir.path().join("messages").join("outbox").is_dir(),
        "reset should re-create messages/outbox for in-flight sync delivery"
    );
}

#[test]
fn resolve_cryo_exe_prefers_sibling_of_current_exe() {
    // `current_exe()` here is the test binary (under target/debug/deps/...).
    // The fix only kicks in when `cryo` exists next to the running binary.
    // We can't easily inject that without changing the function signature,
    // so this test pins the contract: if the resolver returns Ok, the path
    // either ends in `cryo` or is the cryo binary on PATH. After running
    // `cargo build`, target/debug/cryo exists, so the sibling path of the
    // test binary (target/debug/deps/) won't have a sibling cryo —
    // resolution will fall through to `which cryo`. Both outcomes are
    // acceptable; what we want to guarantee is the resolver never returns
    // a path ending in `cryohub` (the original bug).
    if let Ok(p) = resolve_cryo_exe() {
        let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
        assert!(
            !name.starts_with("cryohub"),
            "resolver returned cryohub binary: {}",
            p.display()
        );
        assert!(
            name == "cryo" || name == "cryo.exe",
            "resolver returned unexpected binary: {}",
            p.display()
        );
    }
    // If resolve_cryo_exe errors (no cryo on disk), that's fine for the
    // test — we only assert the regression: never return cryohub.
}

#[test]
fn resolve_cryo_exe_from_test_binary_prefers_target_debug_cryo() {
    let dir = tempfile::tempdir().unwrap();
    let debug = dir.path().join("target").join("debug");
    let deps = debug.join("deps");
    std::fs::create_dir_all(&deps).unwrap();
    let test_bin = deps.join("hub_multi_chamber-abc123");
    std::fs::write(&test_bin, "test binary").unwrap();
    let cryo = debug.join("cryo");
    std::fs::write(&cryo, "cryo binary").unwrap();

    let resolved = resolve_cryo_exe_from(&test_bin, || None).unwrap();
    assert_eq!(resolved, cryo);
}
