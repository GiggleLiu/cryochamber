//! `/api/chambers` routes: list + refresh.

use std::sync::Arc;

use axum::{extract::State, response::Json};
use serde_json::Value;

use crate::hub::state::AppState;

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
mod tests {
    use super::*;

    #[tokio::test]
    async fn get_chambers_lists_workspace_scans() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("alpha")).unwrap();
        let cfg = crate::config::CryoConfig::default();
        crate::config::save_config(&dir.path().join("alpha").join("cryo.toml"), &cfg).unwrap();

        let app = Arc::new(AppState::new(dir.path().to_path_buf()));
        app.refresh();

        let Json(v) = get_chambers(State(app)).await;
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["name"], "alpha");
    }

    #[tokio::test]
    async fn refresh_picks_up_new_chamber() {
        let dir = tempfile::tempdir().unwrap();
        let app = Arc::new(AppState::new(dir.path().to_path_buf()));
        let Json(initial) = get_chambers(State(app.clone())).await;
        assert_eq!(initial.as_array().unwrap().len(), 0);

        let new_dir = dir.path().join("beta");
        std::fs::create_dir_all(&new_dir).unwrap();
        let cfg = crate::config::CryoConfig::default();
        crate::config::save_config(&new_dir.join("cryo.toml"), &cfg).unwrap();

        let Json(after) = post_refresh(State(app)).await;
        let arr = after.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["name"], "beta");
    }

    #[tokio::test]
    async fn get_chambers_refreshes_runtime_fields_from_disk() {
        let dir = tempfile::tempdir().unwrap();
        let alpha = dir.path().join("alpha");
        std::fs::create_dir_all(&alpha).unwrap();
        let cfg = crate::config::CryoConfig::default();
        crate::config::save_config(&alpha.join("cryo.toml"), &cfg).unwrap();
        crate::state::save_state(
            &crate::state::state_path(&alpha),
            &crate::state::CryoState {
                session_number: 5,
                pid: None,
                retry_count: 0,
                agent_override: None,
                max_retries_override: None,
                max_session_duration_override: None,
                last_report_time: None,
                provider_index: None,
                instance_id: None,
                pending_fallback: None,
            },
        )
        .unwrap();
        std::fs::write(
            alpha.join("cryo.log"),
            "--- CRYO SESSION 5 | 2026-04-20T16:32:59Z ---\n\
             task: Continue the plan\n\
             agent: opencode\n\
             [16:33:52] hibernate: plan complete, exit=0, summary=\"done\"\n\
             --- CRYO END ---\n",
        )
        .unwrap();

        let app = Arc::new(AppState::new(dir.path().to_path_buf()));
        let mut stale = crate::hub::discovery::scan_workspace(dir.path());
        for entry in stale.values_mut() {
            entry.running = true;
            entry.completed = false;
        }
        *app.chambers.write().unwrap() = stale;

        let Json(v) = get_chambers(State(app)).await;
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["running"], false);
        assert_eq!(arr[0]["completed"], true);
        assert_eq!(arr[0]["session"], 5);
    }
}
