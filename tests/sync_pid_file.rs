//! Integration test: cryo-gh sync-daemon writes a pid file on startup and
//! removes it on clean shutdown.
//!
//! We give the daemon a fake gh-sync.json so it starts but can't talk to
//! GitHub. The pull/push errors it logs are expected noise.

use std::time::Duration;

fn target_bin(name: &str) -> std::path::PathBuf {
    let mut p = std::env::current_exe().unwrap();
    // tests live at target/debug/deps/<test>; binaries at target/debug/<name>
    p.pop(); // deps
    p.pop(); // debug
    p.push(name);
    p
}

#[test]
#[cfg(unix)]
fn cryo_gh_sync_daemon_manages_pid_file() {
    let dir = tempfile::tempdir().unwrap();
    let workdir = dir.path().to_path_buf();

    // Minimal gh-sync.json so the daemon does not abort.
    std::fs::write(
        workdir.join("gh-sync.json"),
        r#"{"repo":"fake/fake","discussion_number":1,"discussion_node_id":"fake"}"#,
    )
    .unwrap();
    std::fs::create_dir_all(workdir.join("messages").join("outbox")).unwrap();

    let bin = target_bin("cryo-gh");
    assert!(
        bin.exists(),
        "build cryo-gh first: cargo build --bin cryo-gh"
    );

    let mut child = std::process::Command::new(&bin)
        .current_dir(&workdir)
        .arg("sync-daemon")
        .arg("--interval")
        .arg("60")
        .env("CRYO_NO_SERVICE", "1")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn cryo-gh sync-daemon");

    // Wait up to 5 seconds for the pid file to appear.
    let pid_path = workdir.join("cryo-gh-sync.pid");
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if pid_path.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(pid_path.exists(), "pid file should have been created");

    let pid_contents = std::fs::read_to_string(&pid_path).unwrap();
    assert_eq!(pid_contents.trim().parse::<u32>().unwrap(), child.id());

    // SIGTERM the daemon and wait for it.
    unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
    let _ = child.wait();

    // Allow brief time for cleanup after loop exits.
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        if !pid_path.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        !pid_path.exists(),
        "pid file should be removed after SIGTERM"
    );
}
