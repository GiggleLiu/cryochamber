//! Per-chamber sync backend handlers. Delegates to `sync_common` for
//! summaries; `require_workspace` guards the mutating endpoints.

use std::sync::Arc;

use axum::{
    extract::{Path as AxumPath, State},
    http::StatusCode,
    response::Json,
};
use serde_json::Value;

use crate::sync_common;
use crate::web::state::AppState;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web::discovery::encode_id;

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
}
