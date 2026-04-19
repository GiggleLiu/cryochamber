//! Per-chamber sync backend handlers. Delegates to `sync_common` for
//! summaries; `require_workspace` guards the mutating endpoints.

use std::sync::Arc;

use axum::{
    extract::{Path as AxumPath, State},
    http::StatusCode,
    response::Json,
};
use serde_json::Value;

use crate::sync_common::{self, SyncBackend};
use crate::web::discovery::Source;
use crate::web::state::{AppState, SseEvent};

fn require_workspace(entry: &crate::web::discovery::ChamberEntry) -> Result<(), StatusCode> {
    if entry.source == Source::External {
        return Err(StatusCode::CONFLICT);
    }
    Ok(())
}

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
    require_workspace(&entry)?;
    let backend = SyncBackend::parse(&backend_str).ok_or(StatusCode::BAD_REQUEST)?;
    let result = match verb.as_str() {
        "start" => sync_common::start(backend, &path),
        "stop" => sync_common::stop(backend, &path),
        "pull" => sync_common::pull(backend, &path),
        "push" => sync_common::push(backend, &path),
        _ => return Err(StatusCode::BAD_REQUEST),
    };
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
    use crate::web::discovery::{encode_id, ChamberEntry, Source};

    #[tokio::test]
    async fn get_sync_returns_empty_for_unconfigured_chamber() {
        let dir = tempfile::tempdir().unwrap();
        let chambers = dir.path().join("chambers");
        let alpha = chambers.join("alpha");
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
        let chambers = dir.path().join("chambers");
        let alpha = chambers.join("alpha");
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
    async fn post_sync_start_returns_409_for_external_chamber() {
        let dir = tempfile::tempdir().unwrap();
        let external = dir.path().join("outside");
        std::fs::create_dir_all(&external).unwrap();
        let app = Arc::new(AppState::new(dir.path().to_path_buf()));
        let id = encode_id(&external.canonicalize().unwrap());
        let entry = ChamberEntry {
            id: id.clone(),
            name: "outside".into(),
            path: external.canonicalize().unwrap(),
            source: Source::External,
            config_error: None,
            running: true,
            session: None,
            next_wake: None,
            unread: 0,
            completed: false,
            sync: vec![],
        };
        app.chambers.write().unwrap().insert(id.clone(), entry);

        let err = post_sync_action(State(app), AxumPath((id, "gh".into(), "start".into())))
            .await
            .unwrap_err();
        assert_eq!(err, StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn post_sync_action_rejects_unknown_backend() {
        let dir = tempfile::tempdir().unwrap();
        let chambers = dir.path().join("chambers");
        let alpha = chambers.join("alpha");
        std::fs::create_dir_all(&alpha).unwrap();
        let cfg = crate::config::CryoConfig::default();
        crate::config::save_config(&alpha.join("cryo.toml"), &cfg).unwrap();
        let app = Arc::new(AppState::new(dir.path().to_path_buf()));
        app.refresh();
        let id = encode_id(&alpha.canonicalize().unwrap());
        let err = post_sync_action(
            State(app),
            AxumPath((id, "bogus".into(), "start".into())),
        )
        .await
        .unwrap_err();
        assert_eq!(err, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn post_sync_action_rejects_unknown_verb() {
        let dir = tempfile::tempdir().unwrap();
        let chambers = dir.path().join("chambers");
        let alpha = chambers.join("alpha");
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
