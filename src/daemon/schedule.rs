use chrono::NaiveDateTime;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::config::{CryoConfig, RotateOn};

use super::SessionLoopOutcome;

/// Format for parsing TODO `at` timestamps (minute precision, no seconds).
pub(super) const WAKE_TIME_FMT: &str = "%Y-%m-%dT%H:%M";

/// Tracks which provider the daemon is currently using and lets callers
/// advance through the configured provider pool. Agent failures no longer
/// drive auto-retries, so there is no attempt counter or backoff — the
/// scheduler just waits for the next TODO or inbox wake.
#[derive(Debug)]
pub struct RetryState {
    pub provider_index: usize,
    provider_count: usize,
}

impl RetryState {
    pub fn new(provider_count: usize) -> Self {
        Self {
            provider_index: 0,
            provider_count,
        }
    }

    pub fn reset(&mut self) {
        self.provider_index = 0;
    }

    /// Advance to the next provider. Returns true if we wrapped back to index 0
    /// (all providers have been tried in this cycle).
    pub fn rotate_provider(&mut self) -> bool {
        if self.provider_count <= 1 {
            return true; // can't rotate with 0 or 1 provider
        }
        self.provider_index = (self.provider_index + 1) % self.provider_count;
        self.provider_index == 0 // wrapped
    }
}

/// Pure: given the configured rotate-on policy and the provider pool, decide
/// whether a failed session should trigger provider rotation.
pub(super) fn should_rotate_provider(
    rotate_on: &RotateOn,
    quick_exit: bool,
    provider_count: usize,
) -> bool {
    if provider_count < 2 {
        return false;
    }
    match rotate_on {
        RotateOn::QuickExit => quick_exit,
        RotateOn::AnyFailure => true,
        RotateOn::Never => false,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SessionRunResult<'a> {
    Outcome(&'a SessionLoopOutcome),
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ProviderRotationReason {
    QuickExit,
    Failure,
}

impl ProviderRotationReason {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::QuickExit => "quick-exit",
            Self::Failure => "failure",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum NextStep {
    PlanComplete,
    Hibernate {
        next_wake: Option<NaiveDateTime>,
    },
    RotateProvider {
        next_wake: Option<NaiveDateTime>,
        next_provider_index: usize,
        wrapped: bool,
        reason: ProviderRotationReason,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DaemonBootstrapState {
    pub(super) next_report_time: Option<NaiveDateTime>,
    pub(super) next_wake: Option<NaiveDateTime>,
    pub(super) run_now: bool,
    pub(super) watch_inbox_path: Option<PathBuf>,
}

pub(super) fn decide_next_step(
    session_result: SessionRunResult<'_>,
    config: &CryoConfig,
    retry: &RetryState,
    next_wake: Option<NaiveDateTime>,
) -> NextStep {
    match session_result {
        SessionRunResult::Outcome(SessionLoopOutcome::PlanComplete) => NextStep::PlanComplete,
        SessionRunResult::Outcome(SessionLoopOutcome::Hibernate) => {
            NextStep::Hibernate { next_wake }
        }
        SessionRunResult::Outcome(SessionLoopOutcome::ValidationFailed { quick_exit }) => {
            if should_rotate_provider(&config.rotate_on, *quick_exit, config.providers.len()) {
                let provider_count = config.providers.len();
                let next_provider_index = (retry.provider_index + 1) % provider_count;
                return NextStep::RotateProvider {
                    next_wake,
                    next_provider_index,
                    wrapped: next_provider_index == 0,
                    reason: if *quick_exit {
                        ProviderRotationReason::QuickExit
                    } else {
                        ProviderRotationReason::Failure
                    },
                };
            }
            // Agent failed to hibernate cleanly (or returned --exit N) and
            // rotation is disabled for this failure mode: don't retry.
            // Wait for the next TODO / inbox event just like a normal hibernate.
            NextStep::Hibernate { next_wake }
        }
        // Internal error spawning or driving the session. Persist and
        // wait for the next scheduled wake instead of hammering a retry.
        SessionRunResult::Error => NextStep::Hibernate { next_wake },
    }
}

/// Compute how long to sleep given optional wake and report deadlines.
pub(super) fn compute_sleep_timeout(
    wake_deadline: Option<NaiveDateTime>,
    report_deadline: Option<NaiveDateTime>,
    now: NaiveDateTime,
) -> Duration {
    let to_duration =
        |dt: NaiveDateTime| -> Duration { (dt - now).to_std().unwrap_or(Duration::ZERO) };
    match (
        wake_deadline.map(&to_duration),
        report_deadline.map(&to_duration),
    ) {
        (Some(w), Some(r)) => w.min(r),
        (Some(w), None) => w,
        (None, Some(r)) => r,
        (None, None) => Duration::from_secs(3600),
    }
}

/// Compute the next wake time from the TODO list.
/// Iterates all pending TODOs, parses each `at` field, and returns the earliest
/// valid timestamp. Invalid or unparseable entries are skipped with a warning.
pub(super) fn next_wake_from_todos(dir: &Path) -> Option<NaiveDateTime> {
    crate::todo::TodoFile::new(dir.join("todo.json"))
        .next_valid_wake()
        .ok()
        .flatten()
}

/// Check if the scheduled wake time is significantly in the past (machine suspend).
/// Returns `Some(delay_description)` if delayed by more than 5 minutes.
pub(super) fn detect_delayed_wake(scheduled: NaiveDateTime, now: NaiveDateTime) -> Option<String> {
    let delay = now - scheduled;
    if delay > chrono::Duration::minutes(5) {
        let delay_str = if delay.num_hours() > 0 {
            format!("{}h {}m", delay.num_hours(), delay.num_minutes() % 60)
        } else {
            format!("{}m", delay.num_minutes())
        };
        Some(delay_str)
    } else {
        None
    }
}

pub(super) fn delayed_wake_notice(
    is_inbox_wake: bool,
    next_wake: Option<NaiveDateTime>,
    now: NaiveDateTime,
) -> Option<String> {
    match (is_inbox_wake, next_wake) {
        (true, _) | (_, None) => None,
        (false, Some(wake)) => detect_delayed_wake(wake, now).map(|delay_str| {
            format!(
                "DELAYED WAKE: This session was scheduled for {} but is running {} late \
                 (the host machine was likely suspended or powered off). \
                 Check whether time-sensitive tasks need adjustment.",
                wake.format(WAKE_TIME_FMT),
                delay_str,
            )
        }),
    }
}
