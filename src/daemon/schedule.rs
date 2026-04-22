use anyhow::{Context, Result};
use chrono::NaiveDateTime;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::config::{CryoConfig, RotateOn};
use crate::fallback::FallbackAction;
use crate::state::{CryoState, PendingFallbackState};

use super::SessionLoopOutcome;

/// Format for parsing TODO `at` timestamps (minute precision, no seconds).
pub(super) const WAKE_TIME_FMT: &str = "%Y-%m-%dT%H:%M";
pub(super) const FALLBACK_TIME_FMT: &str = "%Y-%m-%dT%H:%M:%S";

/// Tracks retry state with exponential backoff.
#[derive(Debug)]
pub struct RetryState {
    pub attempt: u32,
    pub max_retries: u32,
    pub provider_index: usize,
    provider_count: usize,
}

impl RetryState {
    pub fn new(max_retries: u32, provider_count: usize) -> Self {
        Self {
            attempt: 0,
            max_retries,
            provider_index: 0,
            provider_count,
        }
    }

    /// Calculate backoff duration for current attempt.
    /// Doubles each time: 5s, 10s, 20s, ..., capped at 3600s (1 hour).
    /// Always returns a duration (retries indefinitely with backoff).
    pub fn next_backoff(&self) -> Duration {
        let secs = 5u64.checked_shl(self.attempt).unwrap_or(3600).min(3600);
        Duration::from_secs(secs)
    }

    pub fn record_failure(&mut self) {
        self.attempt += 1;
    }

    pub fn reset(&mut self) {
        self.attempt = 0;
        self.provider_index = 0;
    }

    pub fn exhausted(&self) -> bool {
        self.attempt >= self.max_retries
    }

    /// Advance to the next provider. Returns true if we wrapped back to index 0
    /// (all providers have been tried in this cycle). Resets retry attempt counter.
    pub fn rotate_provider(&mut self) -> bool {
        if self.provider_count <= 1 {
            return true; // can't rotate with 0 or 1 provider
        }
        self.provider_index = (self.provider_index + 1) % self.provider_count;
        self.attempt = 0;
        self.provider_index == 0 // wrapped
    }
}

/// Pure: given the next scheduled wake and (optionally) a session-registered
/// fallback action, produce the `(deadline, action)` to arm. We arm the
/// fallback one hour after the scheduled wake so a missed wake fires the
/// alert rather than silently dropping it.
pub(super) fn scheduled_fallback_for(
    next_wake: Option<NaiveDateTime>,
    fallback: Option<FallbackAction>,
) -> Option<(NaiveDateTime, FallbackAction)> {
    next_wake.and_then(|w| fallback.map(|f| (w + chrono::Duration::hours(1), f)))
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
pub(super) struct RetryPlan {
    pub(super) backoff: Duration,
    pub(super) send_alert: bool,
}

impl RetryPlan {
    pub(super) fn for_state(retry: &RetryState) -> Self {
        Self {
            backoff: retry.next_backoff(),
            send_alert: retry.attempt.saturating_add(1) == retry.max_retries,
        }
    }
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
        scheduled_fallback: Option<(NaiveDateTime, FallbackAction)>,
    },
    RotateProvider {
        next_wake: Option<NaiveDateTime>,
        next_provider_index: usize,
        wrapped: bool,
        reason: ProviderRotationReason,
    },
    Retry {
        next_wake: Option<NaiveDateTime>,
        plan: RetryPlan,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DaemonBootstrapState {
    pub(super) next_report_time: Option<NaiveDateTime>,
    pub(super) next_wake: Option<NaiveDateTime>,
    pub(super) run_now: bool,
    pub(super) pending_fallback: Option<(NaiveDateTime, FallbackAction)>,
    pub(super) watch_inbox_path: Option<PathBuf>,
    pub(super) cleared_invalid_pending_fallback: bool,
}

pub(super) fn decide_next_step(
    session_result: SessionRunResult<'_>,
    config: &CryoConfig,
    retry: &RetryState,
    next_wake: Option<NaiveDateTime>,
) -> NextStep {
    match session_result {
        SessionRunResult::Outcome(SessionLoopOutcome::PlanComplete) => NextStep::PlanComplete,
        SessionRunResult::Outcome(SessionLoopOutcome::Hibernate { fallback }) => {
            NextStep::Hibernate {
                next_wake,
                scheduled_fallback: scheduled_fallback_for(next_wake, fallback.clone()),
            }
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
            NextStep::Retry {
                next_wake,
                plan: RetryPlan::for_state(retry),
            }
        }
        SessionRunResult::Error => NextStep::Retry {
            next_wake,
            plan: RetryPlan::for_state(retry),
        },
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
    let path = dir.join("todo.json");
    let list = crate::todo::TodoList::load(&path).ok()?;
    list.items()
        .iter()
        .filter(|i| !i.done && !i.at.is_empty())
        .filter_map(|i| {
            let parsed = NaiveDateTime::parse_from_str(&i.at, WAKE_TIME_FMT);
            if parsed.is_err() {
                eprintln!(
                    "Daemon: Skipping TODO #{} with invalid at value: {:?}",
                    i.id, i.at
                );
            }
            parsed.ok()
        })
        .min()
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

pub(super) fn pending_fallback_to_state(
    pending: Option<&(NaiveDateTime, FallbackAction)>,
) -> Option<PendingFallbackState> {
    pending.map(|(deadline, action)| PendingFallbackState {
        deadline: deadline.format(FALLBACK_TIME_FMT).to_string(),
        action: action.clone(),
    })
}

pub(super) fn pending_fallback_from_state(
    state: &CryoState,
) -> Result<Option<(NaiveDateTime, FallbackAction)>> {
    let Some(pending) = state.pending_fallback.as_ref() else {
        return Ok(None);
    };
    let deadline = NaiveDateTime::parse_from_str(&pending.deadline, FALLBACK_TIME_FMT)
        .with_context(|| format!("Invalid pending fallback deadline: {}", pending.deadline))?;
    Ok(Some((deadline, pending.action.clone())))
}
