// src/daemon.rs
//! Persistent daemon that owns the session lifecycle.
//!
//! Replaces one-shot `cryo wake` with a long-running process that:
//! - Sleeps until scheduled wake time
//! - Watches messages/inbox/ for reactive wake
//! - Enforces session timeout
//! - Retries crashed agents with exponential backoff

use std::time::Duration;

/// Events the daemon responds to.
#[derive(Debug, PartialEq)]
pub enum DaemonEvent {
    /// Scheduled wake time arrived.
    ScheduledWake,
    /// New file appeared in messages/inbox/.
    InboxChanged,
    /// SIGTERM or SIGINT received.
    Shutdown,
}

/// Tracks retry state with exponential backoff.
#[derive(Debug)]
pub struct RetryState {
    pub attempt: u32,
    pub max_retries: u32,
}

impl RetryState {
    pub fn new(max_retries: u32) -> Self {
        Self {
            attempt: 0,
            max_retries,
        }
    }

    /// Calculate backoff duration for current attempt.
    /// Returns None if max retries exceeded.
    pub fn next_backoff(&self) -> Option<Duration> {
        if self.attempt >= self.max_retries {
            return None;
        }
        // 5s, 15s, 60s, 60s, 60s, ...
        let secs = match self.attempt {
            0 => 5,
            1 => 15,
            _ => 60,
        };
        Some(Duration::from_secs(secs))
    }

    pub fn record_failure(&mut self) {
        self.attempt += 1;
    }

    pub fn reset(&mut self) {
        self.attempt = 0;
    }

    pub fn exhausted(&self) -> bool {
        self.attempt >= self.max_retries
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backoff_sequence() {
        let mut state = RetryState::new(3);
        assert_eq!(state.next_backoff(), Some(Duration::from_secs(5)));

        state.record_failure();
        assert_eq!(state.attempt, 1);
        assert_eq!(state.next_backoff(), Some(Duration::from_secs(15)));

        state.record_failure();
        assert_eq!(state.attempt, 2);
        assert_eq!(state.next_backoff(), Some(Duration::from_secs(60)));

        state.record_failure();
        assert_eq!(state.attempt, 3);
        assert_eq!(state.next_backoff(), None);
        assert!(state.exhausted());
    }

    #[test]
    fn test_backoff_reset() {
        let mut state = RetryState::new(3);
        state.record_failure();
        state.record_failure();
        assert_eq!(state.attempt, 2);

        state.reset();
        assert_eq!(state.attempt, 0);
        assert!(!state.exhausted());
    }

    #[test]
    fn test_backoff_zero_retries() {
        let state = RetryState::new(0);
        assert_eq!(state.next_backoff(), None);
        assert!(state.exhausted());
    }
}
