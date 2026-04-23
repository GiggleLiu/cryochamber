// tests/daemon_tests.rs
use cryochamber::daemon::RetryState;

#[test]
fn test_retry_state_provider_rotation_advances() {
    let mut retry = RetryState::new(3); // 3 providers
    assert_eq!(retry.provider_index, 0);

    let wrapped = retry.rotate_provider();
    assert!(!wrapped);
    assert_eq!(retry.provider_index, 1);

    let wrapped = retry.rotate_provider();
    assert!(!wrapped);
    assert_eq!(retry.provider_index, 2);
}

#[test]
fn test_retry_state_provider_rotation_wraps() {
    let mut retry = RetryState::new(3);

    retry.rotate_provider(); // 0 -> 1
    retry.rotate_provider(); // 1 -> 2
    let wrapped = retry.rotate_provider(); // 2 -> 0
    assert_eq!(retry.provider_index, 0);
    assert!(wrapped); // signals cycle complete
}

#[test]
fn test_retry_state_reset_clears_provider_index() {
    let mut retry = RetryState::new(3);
    retry.rotate_provider();
    retry.rotate_provider();
    assert_eq!(retry.provider_index, 2);

    retry.reset();
    assert_eq!(retry.provider_index, 0);
}

#[test]
fn test_retry_state_single_provider_no_rotation() {
    let mut retry = RetryState::new(1);
    let wrapped = retry.rotate_provider();
    assert_eq!(retry.provider_index, 0);
    assert!(wrapped); // immediately wraps with 1 provider
}

#[test]
fn test_retry_state_no_providers() {
    let mut retry = RetryState::new(0);
    let wrapped = retry.rotate_provider();
    assert_eq!(retry.provider_index, 0);
    assert!(wrapped);
}
