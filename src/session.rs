// src/session.rs
//! Pure business logic extracted from command handlers for testability.

use chrono::NaiveDateTime;
use std::path::Path;

use crate::fallback::FallbackAction;
use crate::marker::{self, CryoMarkers};
use crate::validate;

/// The outcome of processing agent output after a session completes.
#[derive(Debug)]
pub enum SessionOutcome {
    /// Plan is complete — no more wake-ups needed.
    PlanComplete,
    /// Ready to hibernate until the next wake time.
    Hibernate {
        wake_time: NaiveDateTime,
        fallback: Option<FallbackAction>,
        command: Option<String>,
    },
    /// Validation failed — cannot hibernate.
    ValidationFailed {
        errors: Vec<String>,
        warnings: Vec<String>,
    },
}

/// Decide what should happen after a session, given parsed markers.
///
/// Returns warnings separately so the caller can log them regardless of outcome.
pub fn decide_session_outcome(markers: &CryoMarkers) -> (SessionOutcome, Vec<String>) {
    let validation = validate::validate_markers(markers);

    if validation.plan_complete {
        return (SessionOutcome::PlanComplete, validation.warnings);
    }

    if !validation.can_hibernate {
        return (
            SessionOutcome::ValidationFailed {
                errors: validation.errors,
                warnings: validation.warnings.clone(),
            },
            validation.warnings,
        );
    }

    let wake_time = markers.wake_time.as_ref().unwrap().0;
    let outcome = SessionOutcome::Hibernate {
        wake_time,
        fallback: markers.fallbacks.first().cloned(),
        command: markers.command.clone(),
    };
    (outcome, validation.warnings)
}

/// Format a session summary for posting to GitHub Discussions.
pub fn format_session_summary(session_num: u32, markers: &CryoMarkers) -> String {
    let exit_str = markers
        .exit_code
        .as_ref()
        .map(|c| format!("{}", c.as_code()))
        .unwrap_or_else(|| "?".to_string());
    let summary = markers.exit_summary.as_deref().unwrap_or("");
    let plan_note = markers.plan_note.as_deref().unwrap_or("(none)");
    let wake_str = markers
        .wake_time
        .as_ref()
        .map(|w| w.0.format("%Y-%m-%dT%H:%M").to_string())
        .unwrap_or_else(|| "plan complete".to_string());

    format!(
        "## Session {session_num} Summary\n- Exit: {exit_str} {summary}\n- Plan: {plan_note}\n- Next wake: {wake_str}"
    )
}

/// Derive the next task description from session output.
///
/// Prefers CRYO:CMD if present, falls back to CRYO:PLAN note.
pub fn derive_task_from_output(output: &str) -> Option<String> {
    let markers = marker::parse_markers(output).ok()?;
    markers.command.or(markers.plan_note)
}

/// Check whether a plan file should be copied to the destination.
///
/// Returns false if both paths resolve to the same file (avoiding self-copy).
pub fn should_copy_plan(source: &Path, dest: &Path) -> bool {
    match (std::fs::canonicalize(source), std::fs::canonicalize(dest)) {
        (Ok(src), Ok(dst)) => src != dst,
        _ => true,
    }
}
