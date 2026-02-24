// src/session.rs
//! Pure business logic extracted from command handlers for testability.

use std::path::Path;

/// Check whether a plan file should be copied to the destination.
///
/// Returns false if both paths resolve to the same file (avoiding self-copy).
pub fn should_copy_plan(source: &Path, dest: &Path) -> bool {
    match (std::fs::canonicalize(source), std::fs::canonicalize(dest)) {
        (Ok(src), Ok(dst)) => src != dst,
        _ => true,
    }
}
