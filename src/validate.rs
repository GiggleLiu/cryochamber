// src/validate.rs
use crate::marker::CryoMarkers;
use chrono::Local;

pub struct ValidationResult {
    pub can_hibernate: bool,
    pub plan_complete: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

pub fn validate_markers(markers: &CryoMarkers) -> ValidationResult {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    // Check EXIT marker exists
    if markers.exit_code.is_none() {
        errors.push("No [CRYO:EXIT] marker found. Agent must report exit status.".to_string());
    }

    // Explicit [CRYO:PLAN COMPLETE] takes priority over wake time
    let explicit_complete = markers
        .plan_note
        .as_ref()
        .is_some_and(|note| note.eq_ignore_ascii_case("COMPLETE"));

    // Plan complete: explicit PLAN COMPLETE marker, or no WAKE with a valid exit code
    if explicit_complete || (markers.wake_time.is_none() && markers.exit_code.is_some()) {
        return ValidationResult {
            can_hibernate: false,
            plan_complete: true,
            errors: vec![],
            warnings: vec![],
        };
    }

    // No WAKE and no exit code — can't determine state
    if markers.wake_time.is_none() {
        return ValidationResult {
            can_hibernate: false,
            plan_complete: false,
            errors,
            warnings,
        };
    }

    // Check wake time is in the future (or recently past — treat as "wake now")
    if let Some(wake) = &markers.wake_time {
        let now = Local::now().naive_local();
        if *wake.inner() < now {
            let age = now - *wake.inner();
            if age > chrono::Duration::minutes(10) {
                errors.push("Wake time is in the past. Please specify a future time.".to_string());
            } else {
                warnings.push(format!(
                    "Wake time is {}m ago — treating as immediate wake.",
                    age.num_minutes()
                ));
            }
        }
    }

    // Check command exists
    if markers.command.is_none() {
        warnings.push("No [CRYO:CMD] marker. Will re-use previous command.".to_string());
    }

    ValidationResult {
        can_hibernate: errors.is_empty(),
        plan_complete: false,
        errors,
        warnings,
    }
}
