//! Per-chamber sync backend handlers. Delegates to `sync_common` for
//! summaries.

use std::sync::Arc;

use axum::{
    extract::{Path as AxumPath, State},
    http::StatusCode,
    response::Json,
};
use serde_json::Value;

use crate::hub::state::{AppState, SseEvent};
use crate::sync_common::{self, SyncBackend};

pub async fn get_sync(
    State(app): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<Value>, StatusCode> {
    let (path, _) = app.resolve(&id).ok_or(StatusCode::NOT_FOUND)?;
    let summaries = sync_common::summarize_all(&path);
    Ok(Json(
        serde_json::to_value(summaries).unwrap_or(Value::Array(vec![])),
    ))
}

pub async fn post_sync_action(
    State(app): State<Arc<AppState>>,
    AxumPath((id, backend_str, verb)): AxumPath<(String, String, String)>,
) -> Result<Json<Value>, StatusCode> {
    let (path, entry) = app.resolve(&id).ok_or(StatusCode::NOT_FOUND)?;
    let backend = SyncBackend::parse(&backend_str).ok_or(StatusCode::BAD_REQUEST)?;
    // Validate verb before spawning so BAD_REQUEST stays synchronous.
    if !matches!(verb.as_str(), "start" | "stop" | "pull" | "push") {
        return Err(StatusCode::BAD_REQUEST);
    }
    let path_for_task = path.clone();
    let verb_for_task = verb.clone();
    let result = tokio::task::spawn_blocking(move || match verb_for_task.as_str() {
        // `launchctl load -w` returns before the sync daemon has written its
        // pid file, and `unload -w` returns before the daemon has cleared it.
        // Wait for the observable state to settle so the SSE event that fires
        // below sees the settled state — otherwise the hub toggle bounces back
        // because GET /sync reports running=false while the daemon is still
        // booting.
        "start" => {
            let r = sync_common::start(backend, &path_for_task);
            if r.is_ok() {
                let _ = sync_common::wait_for_state(
                    backend,
                    &path_for_task,
                    true,
                    std::time::Duration::from_secs(3),
                );
            }
            r
        }
        "stop" => {
            let r = sync_common::stop(backend, &path_for_task);
            if r.is_ok() {
                let _ = sync_common::wait_for_state(
                    backend,
                    &path_for_task,
                    false,
                    std::time::Duration::from_secs(3),
                );
            }
            r
        }
        "pull" => sync_common::pull(backend, &path_for_task),
        "push" => sync_common::push(backend, &path_for_task),
        _ => unreachable!("verb validated above"),
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let _ = app.tx.send(SseEvent::StatusChange {
        chamber_id: entry.id.clone(),
    });
    match result {
        Ok(()) => Ok(Json(serde_json::json!({
            "ok": true,
            "message": format!("{} {}", backend.as_str(), verb),
        }))),
        Err(e) => Ok(Json(serde_json::json!({
            "ok": false,
            "message": e.to_string(),
        }))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub::discovery::encode_id;

    #[tokio::test]
    async fn get_sync_returns_empty_for_unconfigured_chamber() {
        let dir = tempfile::tempdir().unwrap();
        let alpha = dir.path().join("alpha");
        std::fs::create_dir_all(&alpha).unwrap();
        let cfg = crate::config::CryoConfig::default();
        crate::config::save_config(&alpha.join("cryo.toml"), &cfg).unwrap();

        let app = Arc::new(AppState::new(dir.path().to_path_buf()));
        app.refresh();
        let id = encode_id(&alpha.canonicalize().unwrap());
        let res = get_sync(State(app), AxumPath(id)).await.unwrap();
        assert_eq!(res.0, serde_json::json!([]));
    }

    #[tokio::test]
    async fn get_sync_reports_configured_gh_backend() {
        let dir = tempfile::tempdir().unwrap();
        let alpha = dir.path().join("alpha");
        std::fs::create_dir_all(&alpha).unwrap();
        let cfg = crate::config::CryoConfig::default();
        crate::config::save_config(&alpha.join("cryo.toml"), &cfg).unwrap();
        let state = crate::gh_sync::GhSyncState {
            repo: "alice/x".into(),
            discussion_number: 1,
            discussion_node_id: "n".into(),
            last_read_cursor: None,
            self_login: None,
            last_pushed_session: None,
        };
        crate::gh_sync::save_sync_state(&alpha.join("gh-sync.json"), &state).unwrap();

        let app = Arc::new(AppState::new(dir.path().to_path_buf()));
        app.refresh();
        let id = encode_id(&alpha.canonicalize().unwrap());
        let res = get_sync(State(app), AxumPath(id)).await.unwrap();
        let arr = res.0.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["backend"], "gh");
        assert_eq!(arr[0]["target"], "alice/x#1");
    }

    #[tokio::test]
    async fn post_sync_action_rejects_unknown_backend() {
        let dir = tempfile::tempdir().unwrap();
        let alpha = dir.path().join("alpha");
        std::fs::create_dir_all(&alpha).unwrap();
        let cfg = crate::config::CryoConfig::default();
        crate::config::save_config(&alpha.join("cryo.toml"), &cfg).unwrap();
        let app = Arc::new(AppState::new(dir.path().to_path_buf()));
        app.refresh();
        let id = encode_id(&alpha.canonicalize().unwrap());
        let err = post_sync_action(State(app), AxumPath((id, "bogus".into(), "start".into())))
            .await
            .unwrap_err();
        assert_eq!(err, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn post_sync_action_rejects_unknown_verb() {
        let dir = tempfile::tempdir().unwrap();
        let alpha = dir.path().join("alpha");
        std::fs::create_dir_all(&alpha).unwrap();
        let cfg = crate::config::CryoConfig::default();
        crate::config::save_config(&alpha.join("cryo.toml"), &cfg).unwrap();
        let app = Arc::new(AppState::new(dir.path().to_path_buf()));
        app.refresh();
        let id = encode_id(&alpha.canonicalize().unwrap());
        let err = post_sync_action(State(app), AxumPath((id, "gh".into(), "dance".into())))
            .await
            .unwrap_err();
        assert_eq!(err, StatusCode::BAD_REQUEST);
    }
}
