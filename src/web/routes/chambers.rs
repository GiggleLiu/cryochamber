//! `/api/chambers` routes: list + refresh.

use std::sync::Arc;

use axum::{extract::State, response::Json};
use serde_json::Value;

use crate::web::state::AppState;

pub async fn get_chambers(State(app): State<Arc<AppState>>) -> Json<Value> {
    let idx = app.chambers.read().unwrap();
    let list: Vec<&crate::web::discovery::ChamberEntry> = idx.values().collect();
    Json(serde_json::to_value(&list).unwrap_or(Value::Array(vec![])))
}

pub async fn post_refresh(State(app): State<Arc<AppState>>) -> Json<Value> {
    app.refresh();
    let idx = app.chambers.read().unwrap();
    let list: Vec<&crate::web::discovery::ChamberEntry> = idx.values().collect();
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
        // Populate index without calling registry::list() so tests are
        // isolated from any real daemons the machine might have running.
        let mut idx = crate::web::discovery::scan_workspace(dir.path());
        crate::web::discovery::populate_runtime(&mut idx);
        *app.chambers.write().unwrap() = idx;

        let Json(v) = get_chambers(State(app)).await;
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["name"], "alpha");
        assert_eq!(arr[0]["source"], "workspace");
    }

    #[tokio::test]
    async fn refresh_picks_up_new_chamber() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("chambers")).unwrap();
        let app = Arc::new(AppState::new(dir.path().to_path_buf()));
        // Start with empty index (avoid calling refresh, which hits the
        // global registry — not reliable for isolation). The first
        // post_refresh below is the one we're actually testing.
        let Json(initial) = get_chambers(State(app.clone())).await;
        assert_eq!(initial.as_array().unwrap().len(), 0);

        // Add a chamber, then refresh (this still calls the registry but
        // will add the new workspace chamber on top of whatever's in the
        // registry, so we only assert the new chamber is present).
        let new_dir = dir.path().join("chambers").join("beta");
        std::fs::create_dir_all(&new_dir).unwrap();
        let cfg = crate::config::CryoConfig::default();
        crate::config::save_config(&new_dir.join("cryo.toml"), &cfg).unwrap();

        let Json(after) = post_refresh(State(app)).await;
        let arr = after.as_array().unwrap();
        // At least one entry, and one must be named "beta"
        assert!(arr.iter().any(|e| e["name"] == "beta"));
    }
}
