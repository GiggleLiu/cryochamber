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

    // No WAKE = plan complete (only if we have an exit code)
    if markers.wake_time.is_none() {
        if markers.exit_code.is_some() {
            return ValidationResult {
                can_hibernate: false,
                plan_complete: true,
                errors: vec![],
                warnings: vec![],
            };
        } else {
            return ValidationResult {
                can_hibernate: false,
                plan_complete: false,
                errors,
                warnings,
            };
        }
    }

    // Check wake time is in the future
    if let Some(wake) = &markers.wake_time {
        let now = Local::now().naive_local();
        if *wake.inner() < now {
            errors.push("Wake time is in the past. Please specify a future time.".to_string());
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
