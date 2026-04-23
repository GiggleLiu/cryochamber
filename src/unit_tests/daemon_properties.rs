//! Property tests for the daemon's pure state-machine functions.
//!
//! Example-based tests live next to this file in `daemon.rs`. These randomized
//! tests cover the same functions from a different angle: they assert
//! *invariants* that should hold for all reachable inputs, not specific
//! input→output rows. The goal is to catch corner cases at the edges of the
//! state machine (backoff overflow, deadline-in-past, unusual provider counts,
//! etc.) that example tables miss.

use super::*;
use proptest::prelude::*;
use std::collections::VecDeque;
use std::sync::Mutex;

// ---------- Strategies ----------

fn hibernate_outcome_strategy() -> impl Strategy<Value = SessionLoopOutcome> {
    prop_oneof![
        Just(SessionLoopOutcome::PlanComplete),
        Just(SessionLoopOutcome::Hibernate),
        any::<bool>().prop_map(|q| SessionLoopOutcome::ValidationFailed { quick_exit: q }),
    ]
}

// Construct a NaiveDateTime offset by `secs` seconds from a fixed origin.
fn offset_from_origin(origin: NaiveDateTime, secs: i64) -> NaiveDateTime {
    origin + chrono::Duration::seconds(secs)
}

fn origin() -> NaiveDateTime {
    NaiveDateTime::parse_from_str("2026-01-01T00:00:00", "%Y-%m-%dT%H:%M:%S").unwrap()
}

// ---------- RetryState ----------

proptest! {
    /// The backoff doubling is capped: every attempt yields a duration in
    /// [5s, 3600s], and the sequence is monotonically non-decreasing until it
    /// pins at the cap.
    #[test]
    fn prop_retry_backoff_is_bounded(attempt in 0u32..128) {
        let mut state = RetryState::new(1);
        state.attempt = attempt;
        let d = state.next_backoff();
        prop_assert!(d >= Duration::from_secs(5));
        prop_assert!(d <= Duration::from_secs(3600));
    }

    /// Starting from zero, the backoff sequence is non-decreasing for any
    /// number of consecutive failures. This catches any future regression that
    /// lets the cap "snap back" or produces a spike.
    #[test]
    fn prop_retry_backoff_monotonic(n in 1u32..20) {
        let mut state = RetryState::new(1);
        let mut prev = state.next_backoff();
        for _ in 0..n {
            state.record_failure();
            let cur = state.next_backoff();
            prop_assert!(cur >= prev, "backoff went down: {prev:?} -> {cur:?}");
            prev = cur;
        }
    }

    /// `rotate_provider` cycles indices 0..count and reports `true` exactly on
    /// the wrap-around step. Each rotation resets the attempt counter.
    #[test]
    fn prop_retry_rotate_cycles(count in 2usize..10, rotations in 1u32..40) {
        let mut state = RetryState::new(count);
        state.attempt = 3;
        let mut wrapped_count = 0;
        for step in 1..=rotations {
            let wrapped = state.rotate_provider();
            prop_assert_eq!(state.attempt, 0, "rotate must reset attempt");
            let expected_index = (step as usize) % count;
            prop_assert_eq!(state.provider_index, expected_index);
            if wrapped {
                wrapped_count += 1;
                prop_assert_eq!(expected_index, 0, "wrap reported off a non-zero index");
            }
        }
        // Over `rotations` steps with `count` providers, we wrap exactly
        // `rotations / count` times.
        prop_assert_eq!(wrapped_count, rotations / count as u32);
    }

    /// With 0 or 1 provider, rotation always reports a wrap (no meaningful
    /// rotation is possible). The caller uses this to back off after trying
    /// everything once.
    #[test]
    fn prop_retry_rotate_single_provider_always_wraps(count in 0usize..=1) {
        let mut state = RetryState::new(count);
        prop_assert!(state.rotate_provider());
        prop_assert!(state.rotate_provider());
    }
}

// ---------- compute_sleep_timeout ----------

proptest! {
    /// The sleep is never longer than the nearest future deadline, and it is
    /// zero whenever any deadline is already in the past.
    #[test]
    fn prop_sleep_respects_deadlines(
        wake_offset in prop::option::of(-3600i64..=7200),
        report_offset in prop::option::of(-3600i64..=7200),
    ) {
        let now = origin();
        let wake = wake_offset.map(|s| offset_from_origin(now, s));
        let report = report_offset.map(|s| offset_from_origin(now, s));
        let sleep = compute_sleep_timeout(wake, report, now);

        // If either deadline is in the past, sleep is zero (duration clamps to
        // ZERO when the NaiveDateTime math goes negative).
        let wake_past = wake_offset.is_some_and(|s| s <= 0);
        let report_past = report_offset.is_some_and(|s| s <= 0);
        if wake_past || report_past {
            prop_assert_eq!(sleep, Duration::ZERO);
            return Ok(());
        }

        // Otherwise sleep is at most min(future deadlines).
        let deadlines: Vec<i64> = [wake_offset, report_offset]
            .into_iter()
            .flatten()
            .filter(|s| *s > 0)
            .collect();
        if deadlines.is_empty() {
            prop_assert_eq!(sleep, Duration::from_secs(3600));
        } else {
            let min_future = *deadlines.iter().min().unwrap() as u64;
            prop_assert!(
                sleep <= Duration::from_secs(min_future),
                "sleep {sleep:?} exceeded nearest deadline {min_future}s"
            );
        }
    }
}

// ---------- resolve_hibernate_request ----------

proptest! {
    /// Exit table:
    ///   exit_code != 0               → ValidationFailed
    ///   complete && exit_code == 0   → PlanComplete
    ///   !has_pending_todos           → outcome=None (rejected)
    ///   otherwise                    → Hibernate
    #[test]
    fn prop_resolve_hibernate_request_matches_spec(
        complete in any::<bool>(),
        exit_code in 0u8..=10,
        summary in prop::option::of("[A-Za-z0-9 ]{0,32}"),
        has_pending_todos in any::<bool>(),
    ) {
        let decision = resolve_hibernate_request(
            complete,
            exit_code,
            summary.as_deref(),
            has_pending_todos,
        );

        if exit_code != 0 {
            prop_assert_eq!(
                decision.outcome,
                Some(SessionLoopOutcome::ValidationFailed { quick_exit: false })
            );
            prop_assert!(decision.response_ok, "failure path still ACKs the agent");
        } else if complete {
            prop_assert_eq!(decision.outcome, Some(SessionLoopOutcome::PlanComplete));
        } else if !has_pending_todos {
            prop_assert_eq!(decision.outcome, None);
            prop_assert!(!decision.response_ok);
        } else {
            prop_assert_eq!(decision.outcome, Some(SessionLoopOutcome::Hibernate));
            prop_assert!(decision.response_ok);
        }
    }
}

// ---------- resolve_interrupted_session ----------

proptest! {
    /// Whenever the agent managed to hibernate before the interruption, its
    /// outcome wins — we never throw away a clean hibernate because a SIGTERM
    /// or timeout arrived milliseconds later.
    #[test]
    fn prop_interrupted_prefers_hibernate_outcome(
        outcome in hibernate_outcome_strategy(),
        shutdown in any::<bool>(),
    ) {
        let kind = if shutdown { SessionInterruption::Shutdown } else { SessionInterruption::Timeout };
        let decision = resolve_interrupted_session(kind, Some(outcome.clone()));
        prop_assert_eq!(decision.outcome, outcome);
    }

    /// No hibernate outcome → ValidationFailed with quick_exit=false regardless
    /// of how the session ended. `quick_exit` is only a meaningful signal when
    /// the child exited naturally, not when we killed it.
    #[test]
    fn prop_interrupted_without_hibernate_is_non_quick_failure(shutdown in any::<bool>()) {
        let kind = if shutdown { SessionInterruption::Shutdown } else { SessionInterruption::Timeout };
        let decision = resolve_interrupted_session(kind, None);
        prop_assert_eq!(
            decision.outcome,
            SessionLoopOutcome::ValidationFailed { quick_exit: false }
        );
    }
}

// ---------- resolve_child_exit ----------

proptest! {
    /// `quick_exit` is exactly "child exited naturally AND elapsed < 5s". If
    /// the agent hibernated first, quick_exit is always false (the session
    /// succeeded in the eyes of the protocol, whatever the elapsed time).
    #[test]
    fn prop_resolve_child_exit_quick_exit_threshold(
        elapsed_ms in 0u64..30_000,
        hibernated in any::<bool>(),
        outcome in hibernate_outcome_strategy(),
    ) {
        let elapsed = Duration::from_millis(elapsed_ms);
        let hib_outcome = if hibernated { Some(outcome.clone()) } else { None };
        let decision = resolve_child_exit(hib_outcome, elapsed);

        if hibernated {
            prop_assert_eq!(decision.outcome, outcome);
            prop_assert!(!decision.quick_exit, "hibernate path must suppress quick_exit");
        } else {
            let should_be_quick = elapsed < Duration::from_secs(5);
            prop_assert_eq!(decision.quick_exit, should_be_quick);
            prop_assert_eq!(
                decision.outcome,
                SessionLoopOutcome::ValidationFailed { quick_exit: should_be_quick }
            );
        }
    }
}

// ---------- wait_for_idle_event ----------

/// Minimal EventSource for property tests — exposes one scripted event.
struct ScriptedSource {
    events: Mutex<VecDeque<Result<DaemonEvent, WaitError>>>,
}

impl ScriptedSource {
    fn new(events: Vec<Result<DaemonEvent, WaitError>>) -> Self {
        Self {
            events: Mutex::new(events.into()),
        }
    }
}

impl EventSource for ScriptedSource {
    fn recv_timeout(&self, _timeout: Duration) -> Result<DaemonEvent, WaitError> {
        self.events
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Err(WaitError::Timeout))
    }

    fn drain_inbox_changed(&self) {
        let mut events = self.events.lock().unwrap();
        while matches!(events.front(), Some(Ok(DaemonEvent::InboxChanged))) {
            events.pop_front();
        }
    }
}

proptest! {
    /// Event priority is total and fully determined by the first event on the
    /// channel plus the wake deadline:
    ///   InboxChanged   ⇒ WakeFromInbox
    ///   Shutdown       ⇒ Shutdown
    ///   Timeout + deadline reached     ⇒ WakeFromSchedule
    ///   Timeout + deadline unreachable ⇒ StayIdle
    ///   Disconnected  ⇒ Disconnected
    #[test]
    fn prop_idle_event_priority(
        event_kind in 0u8..5,
        deadline_offset in prop::option::of(-3600i64..=3600),
    ) {
        let now = origin();
        let deadline = deadline_offset.map(|s| offset_from_origin(now, s));

        let event: Result<DaemonEvent, WaitError> = match event_kind {
            0 => Ok(DaemonEvent::InboxChanged),
            1 => Ok(DaemonEvent::Shutdown),
            2 => Err(WaitError::Timeout),
            3 => Err(WaitError::Disconnected),
            _ => Err(WaitError::Timeout), // another timeout case
        };

        let source = ScriptedSource::new(vec![event]);
        let outcome = wait_for_idle_event(&source, Duration::from_millis(1), deadline, || now);

        match event_kind {
            0 => prop_assert_eq!(outcome, IdleWaitOutcome::WakeFromInbox),
            1 => prop_assert_eq!(outcome, IdleWaitOutcome::Shutdown),
            3 => prop_assert_eq!(outcome, IdleWaitOutcome::Disconnected),
            _ => {
                // Timeout branch: wake iff deadline reached.
                let expected = match deadline_offset {
                    Some(s) if s <= 0 => IdleWaitOutcome::WakeFromSchedule,
                    _ => IdleWaitOutcome::StayIdle,
                };
                prop_assert_eq!(outcome, expected);
            }
        }
    }
}
