use super::*;
use std::io::Write;

#[test]
fn pid_path_points_into_dir() {
    let p = sync_pid_path(std::path::Path::new("/tmp/cryo-x"));
    assert_eq!(p, std::path::Path::new("/tmp/cryo-x/cryo-gh-sync.pid"));
}

#[test]
fn read_missing_pid_returns_none() {
    let dir = tempfile::tempdir().unwrap();
    assert!(read_sync_pid(dir.path()).is_none());
}

#[test]
fn read_present_pid_returns_value() {
    let dir = tempfile::tempdir().unwrap();
    let mut f = std::fs::File::create(sync_pid_path(dir.path())).unwrap();
    f.write_all(b"12345\n").unwrap();
    assert_eq!(read_sync_pid(dir.path()), Some(12345));
}

#[test]
fn read_invalid_pid_returns_none() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(sync_pid_path(dir.path()), "not-a-number").unwrap();
    assert!(read_sync_pid(dir.path()).is_none());
}

#[test]
fn running_is_false_when_no_pid_file() {
    let dir = tempfile::tempdir().unwrap();
    assert!(!is_sync_running(dir.path()));
}

#[test]
fn running_is_false_for_dead_pid() {
    let dir = tempfile::tempdir().unwrap();
    let child = std::process::Command::new("true").spawn().unwrap();
    let dead_pid = child.id();
    let _ = child.wait_with_output();
    std::fs::write(sync_pid_path(dir.path()), dead_pid.to_string()).unwrap();
    assert!(!is_sync_running(dir.path()));
}
