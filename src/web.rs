use axum::{
    extract::State,
    response::Json,
    routing::get,
    Router,
};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;

use crate::{config, log, message, state};

pub struct AppState {
    pub project_dir: PathBuf,
}

pub fn build_router(project_dir: PathBuf) -> Router {
    let shared = Arc::new(AppState { project_dir });
    Router::new()
        .route("/api/status", get(get_status))
        .route("/api/messages", get(get_messages))
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

async fn get_messages(State(state): State<Arc<AppState>>) -> Json<Value> {
    let dir = &state.project_dir;

    let mut all_messages: Vec<Value> = Vec::new();

    if let Ok(inbox) = message::read_inbox(dir) {
        for (_filename, msg) in inbox {
            all_messages.push(message_to_json(&msg, "inbox"));
        }
    }

    if let Ok(outbox) = message::read_outbox(dir) {
        for (_filename, msg) in outbox {
            all_messages.push(message_to_json(&msg, "outbox"));
        }
    }

    // Sort by timestamp
    all_messages.sort_by(|a, b| {
        let ta = a["timestamp"].as_str().unwrap_or("");
        let tb = b["timestamp"].as_str().unwrap_or("");
        ta.cmp(tb)
    });

    Json(Value::Array(all_messages))
}

fn message_to_json(msg: &message::Message, direction: &str) -> Value {
    json!({
        "direction": direction,
        "from": msg.from,
        "subject": msg.subject,
        "body": msg.body,
        "timestamp": msg.timestamp.format("%Y-%m-%dT%H:%M:%S").to_string(),
    })
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

    #[tokio::test]
    async fn test_get_messages_empty() {
        let dir = tempfile::tempdir().unwrap();
        crate::message::ensure_dirs(dir.path()).unwrap();
        let state = AppState {
            project_dir: dir.path().to_path_buf(),
        };
        let resp = get_messages(State(Arc::new(state))).await;
        let msgs: Vec<serde_json::Value> = serde_json::from_value(resp.0).unwrap();
        assert!(msgs.is_empty());
    }

    #[tokio::test]
    async fn test_get_messages_with_inbox_and_outbox() {
        let dir = tempfile::tempdir().unwrap();
        crate::message::ensure_dirs(dir.path()).unwrap();

        // Write one inbox message
        let msg = crate::message::Message {
            from: "human".to_string(),
            subject: "Hello".to_string(),
            body: "Hi agent".to_string(),
            timestamp: chrono::NaiveDate::from_ymd_opt(2026, 2, 25)
                .unwrap()
                .and_hms_opt(10, 0, 0)
                .unwrap(),
            metadata: std::collections::BTreeMap::new(),
        };
        crate::message::write_message(dir.path(), "inbox", &msg).unwrap();

        // Write one outbox message
        let reply = crate::message::Message {
            from: "agent".to_string(),
            subject: "Reply".to_string(),
            body: "Got it".to_string(),
            timestamp: chrono::NaiveDate::from_ymd_opt(2026, 2, 25)
                .unwrap()
                .and_hms_opt(10, 5, 0)
                .unwrap(),
            metadata: std::collections::BTreeMap::new(),
        };
        crate::message::write_message(dir.path(), "outbox", &reply).unwrap();

        let state = AppState {
            project_dir: dir.path().to_path_buf(),
        };
        let resp = get_messages(State(Arc::new(state))).await;
        let msgs: Vec<serde_json::Value> = serde_json::from_value(resp.0).unwrap();
        assert_eq!(msgs.len(), 2);
        // Sorted by timestamp — inbox first
        assert_eq!(msgs[0]["direction"], "inbox");
        assert_eq!(msgs[1]["direction"], "outbox");
    }
}
