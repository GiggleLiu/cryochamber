//! `/api/chambers` routes: list + refresh.

use std::sync::Arc;

use axum::{extract::State, response::Json};
use serde_json::Value;

use crate::hub::state::AppState;

pub async fn get_chambers(State(app): State<Arc<AppState>>) -> Json<Value> {
    let idx = app.chambers.read().unwrap();
    let list: Vec<&crate::hub::discovery::ChamberEntry> = idx.values().collect();
    Json(serde_json::to_value(&list).unwrap_or(Value::Array(vec![])))
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
        let chambers = dir.path().join("chambers");
        std::fs::create_dir_all(chambers.join("alpha")).unwrap();
        let cfg = crate::config::CryoConfig::default();
        crate::config::save_config(&chambers.join("alpha").join("cryo.toml"), &cfg).unwrap();

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
        std::fs::create_dir_all(dir.path().join("chambers")).unwrap();
        let app = Arc::new(AppState::new(dir.path().to_path_buf()));
        let Json(initial) = get_chambers(State(app.clone())).await;
        assert_eq!(initial.as_array().unwrap().len(), 0);

        let new_dir = dir.path().join("chambers").join("beta");
        std::fs::create_dir_all(&new_dir).unwrap();
        let cfg = crate::config::CryoConfig::default();
        crate::config::save_config(&new_dir.join("cryo.toml"), &cfg).unwrap();

        let Json(after) = post_refresh(State(app)).await;
        let arr = after.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["name"], "beta");
    }
}
