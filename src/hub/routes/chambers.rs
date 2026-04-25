//! `/api/chambers` routes: list + refresh.

use std::sync::Arc;

use axum::{extract::State, response::Json};
use serde_json::Value;

use crate::hub::state::AppState;

/// Validate a user-supplied chamber name. Returns `Ok(())` if safe to use as
/// a directory name under the workspace, otherwise a one-line reason string
/// suitable for the `error` field of a 400 response.
pub fn validate_chamber_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("name is empty".to_string());
    }
    if name.len() > 64 {
        return Err("name too long (max 64 chars)".to_string());
    }
    if name.starts_with('.') {
        return Err("name contains illegal characters".to_string());
    }
    if name == ".." {
        return Err("name contains illegal characters".to_string());
    }
    for c in name.chars() {
        if c == '/' || c == '\\' || c.is_whitespace() || c.is_control() {
            return Err("name contains illegal characters".to_string());
        }
    }
    Ok(())
}

pub async fn get_chambers(State(app): State<Arc<AppState>>) -> Json<Value> {
    // Snapshot the index under a short-lived reader, then run blocking
    // per-chamber I/O (state/todos/inbox reads + libc::kill probes) off the
    // async worker. Only reacquire the writer to swap the populated snapshot
    // back in. This avoids holding the std RwLock writer across filesystem
    // walks, which would otherwise stall every concurrent route that calls
    // `app.resolve(..)` (they all go through `chambers.read()`).
    let app_task = app.clone();
    let value = tokio::task::spawn_blocking(move || {
        let mut snapshot = app_task
            .chambers
            .read()
            .map(|g| g.clone())
            .unwrap_or_default();
        crate::hub::discovery::populate_runtime(&mut snapshot);
        let value = serde_json::to_value(snapshot.values().collect::<Vec<_>>())
            .unwrap_or(Value::Array(vec![]));
        if let Ok(mut idx) = app_task.chambers.write() {
            *idx = snapshot;
        }
        value
    })
    .await
    .unwrap_or(Value::Array(vec![]));
    Json(value)
}

pub async fn post_refresh(State(app): State<Arc<AppState>>) -> Json<Value> {
    app.refresh();
    let idx = app.chambers.read().unwrap();
    let list: Vec<&crate::hub::discovery::ChamberEntry> = idx.values().collect();
    Json(serde_json::to_value(&list).unwrap_or(Value::Array(vec![])))
}

#[cfg(test)]
#[path = "../../unit_tests/hub/routes/chambers.rs"]
mod tests;
