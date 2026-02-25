use axum::{
    extract::State,
    response::Json,
    routing::get,
    Router,
};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;

use crate::{config, log, state};

pub struct AppState {
    pub project_dir: PathBuf,
}

pub fn build_router(project_dir: PathBuf) -> Router {
    let shared = Arc::new(AppState { project_dir });
    Router::new()
        .route("/api/status", get(get_status))
        .with_state(shared)
}

async fn get_status(State(state): State<Arc<AppState>>) -> Json<Value> {
    let dir = &state.project_dir;

    let cfg = config::load_config(&config::config_path(dir))
        .ok()
        .flatten()
        .unwrap_or_default();

    let (running, session, agent) =
        match state::load_state(&state::state_path(dir)).ok().flatten() {
            Some(st) => {
                let is_running = state::is_locked(&st);
                let effective_agent = st
                    .agent_override
                    .as_deref()
                    .unwrap_or(&cfg.agent)
                    .to_string();
                (is_running, st.session_number, effective_agent)
            }
            None => (false, 0, cfg.agent.clone()),
        };

    let log_tail = log::read_latest_session(&log::log_path(dir))
        .ok()
        .flatten()
        .unwrap_or_default();

    Json(json!({
        "running": running,
        "session": session,
        "agent": agent,
        "log_tail": log_tail,
    }))
}

pub async fn serve(project_dir: PathBuf, port: u16) -> anyhow::Result<()> {
    let app = build_router(project_dir);
    let addr = format!("127.0.0.1:{port}");
    println!("Cryochamber web UI: http://{addr}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::State;

    #[tokio::test]
    async fn test_get_status_no_daemon() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState {
            project_dir: dir.path().to_path_buf(),
        };
        let resp = get_status(State(Arc::new(state))).await;
        let status = &resp.0;
        assert_eq!(status["running"], false);
        assert_eq!(status["session"], 0);
    }
}
