use super::*;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::sync::Mutex;

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn make_stub(dir: &Path, name: &str, exit_code: i32, stdout: &str) -> PathBuf {
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
fn summarize_all_empty_for_unconfigured_dir() {
    let dir = tempfile::tempdir().unwrap();
    assert!(summarize_all(dir.path()).is_empty());
}

#[test]
fn summarize_all_returns_configured_backends() {
    let dir = tempfile::tempdir().unwrap();
    let state = crate::zulip_sync::ZulipSyncState {
        site: "https://z.example.com".into(),
        stream: "notes".into(),
        stream_id: 7,
        self_email: "bot@z.example.com".into(),
        topic: None,
        last_message_id: None,
        last_pushed_session: Some(3),
    };
    crate::zulip_sync::save_sync_state(&dir.path().join("zulip-sync.json"), &state).unwrap();

    let summaries = summarize_all(dir.path());
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].backend, SyncBackend::Zulip);
    assert_eq!(
        summaries[0].target,
        "https://z.example.com · notes / cryochamber"
    );
    assert_eq!(summaries[0].last_pushed_session, Some(3));
    assert!(!summaries[0].running);
}

#[test]
fn start_invokes_sync_subcommand_via_env_override() {
    let _guard = ENV_LOCK.lock().unwrap();
    let bin = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let stub = make_stub(bin.path(), "cryo-zulip-stub", 0, "ok");
    std::env::set_var("CRYO_ZULIP_CLI", &stub);
    let res = start(SyncBackend::Zulip, work.path());
    std::env::remove_var("CRYO_ZULIP_CLI");
    assert!(res.is_ok(), "{res:?}");
}

#[test]
fn stop_propagates_non_zero_exit_as_error() {
    let _guard = ENV_LOCK.lock().unwrap();
    let bin = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let stub = make_stub(bin.path(), "cryo-zulip-stub", 7, "boom");
    std::env::set_var("CRYO_ZULIP_CLI", &stub);
    let res = stop(SyncBackend::Zulip, work.path());
    std::env::remove_var("CRYO_ZULIP_CLI");
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
    // No pid file means not running. Expect `false` to match right away.
    let start = Instant::now();
    let ok = wait_for_state(
        SyncBackend::Zulip,
        work.path(),
        false,
        Duration::from_secs(2),
    );
    assert!(ok);
    assert!(
        start.elapsed() < Duration::from_millis(200),
        "fast-path should not sleep when already matching"
    );
}

#[test]
fn wait_for_state_observes_transition_before_deadline() {
    let work = tempfile::tempdir().unwrap();
    let pid_path = crate::zulip_sync::sync_pid_path(work.path());
    let pid_path_writer = pid_path.clone();

    // Drop a live pid file after we start waiting. Our own PID is alive by
    // definition, so `is_sync_running` will flip to true.
    let writer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        std::fs::write(&pid_path_writer, std::process::id().to_string()).unwrap();
    });

    let ok = wait_for_state(
        SyncBackend::Zulip,
        work.path(),
        true,
        Duration::from_secs(2),
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
        Duration::from_millis(250),
    );
    assert!(!ok, "no pid file will ever appear -- must time out");
}
