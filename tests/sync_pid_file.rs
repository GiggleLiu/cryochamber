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

/// RAII wrapper: on drop, SIGKILL + reap the child so an assertion panic
/// earlier in the test never leaves a zombie process behind. Silences
/// clippy::zombie_processes and keeps the suite safe when assertions fail.
struct ChildGuard(Option<std::process::Child>);

impl ChildGuard {
    fn take(&mut self) -> std::process::Child {
        self.0.take().expect("child already taken")
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[test]
#[cfg(unix)]
// Clippy's static analysis can't track the ChildGuard drop + try_wait reap
// path; in practice every exit route either reaps via `try_wait(Ok(Some))`,
// calls `child.wait()`, or falls through to the guard's Drop.
#[allow(clippy::zombie_processes)]
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

    let mut guard = ChildGuard(Some(
        std::process::Command::new(&bin)
            .current_dir(&workdir)
            .arg("sync-daemon")
            .arg("--interval")
            .arg("60")
            .env("CRYO_NO_SERVICE", "1")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn cryo-gh sync-daemon"),
    ));
    let child_id = guard.0.as_ref().unwrap().id();

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
    assert_eq!(pid_contents.trim().parse::<u32>().unwrap(), child_id);

    // Take ownership of the child so the normal-path wait semantics are
    // explicit; the guard's drop is still a safety net if we panic.
    let mut child = guard.take();

    // SIGTERM the daemon and wait for it with a bounded timeout so a stuck
    // child cannot hang the test suite. Assert `kill` returned 0 (ESRCH means
    // the child already exited, which is acceptable).
    let pid = child.id() as i32;
    let rc = unsafe { libc::kill(pid, libc::SIGTERM) };
    if rc != 0 {
        let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
        let _ = child.wait();
        assert_eq!(errno, libc::ESRCH, "kill failed with errno={errno}");
    }

    // Poll try_wait with a 5s timeout; fall back to SIGKILL if it misbehaves.
    let wait_deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut exited = false;
    while std::time::Instant::now() < wait_deadline {
        match child.try_wait() {
            Ok(Some(_)) => {
                exited = true;
                break;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("try_wait failed: {e}");
            }
        }
    }
    if !exited {
        let _ = unsafe { libc::kill(pid, libc::SIGKILL) };
        let _ = child.wait();
        panic!("daemon did not exit within 5s after SIGTERM");
    }

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
