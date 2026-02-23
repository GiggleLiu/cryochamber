// tests/timer_mod_tests.rs
use cryochamber::timer::run_checked;
use std::process::Command;

#[test]
fn test_run_checked_success() {
    let mut cmd = Command::new("true");
    let result = run_checked(&mut cmd, "run true");
    assert!(result.is_ok());
}

#[test]
fn test_run_checked_failure() {
    let mut cmd = Command::new("false");
    let result = run_checked(&mut cmd, "run false");
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("run false"));
    assert!(err.contains("exited with"));
}

#[test]
fn test_run_checked_spawn_failure() {
    let mut cmd = Command::new("nonexistent_command_xyz_12345");
    let result = run_checked(&mut cmd, "spawn nonexistent");
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("spawn nonexistent"));
}
