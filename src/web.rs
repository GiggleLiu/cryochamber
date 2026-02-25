use axum::{
    extract::State,
    response::{
        sse::{Event, KeepAlive, Sse},
        Json,
    },
    routing::{get, post},
    Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use crate::{config, log, message, state};

#[derive(Clone, Debug)]
pub enum SseEvent {
    NewMessage {
        direction: String,
        from: String,
        subject: String,
        body: String,
        timestamp: String,
    },
    StatusChange,
    LogLine(String),
}

pub struct AppState {
    pub project_dir: PathBuf,
    pub tx: tokio::sync::broadcast::Sender<SseEvent>,
}

pub fn build_router(project_dir: PathBuf) -> Router {
    let (tx, _rx) = tokio::sync::broadcast::channel::<SseEvent>(256);
    let state = Arc::new(AppState { project_dir, tx });
    Router::new()
        .route("/api/status", get(get_status))
        .route("/api/messages", get(get_messages))
        .route("/api/send", post(post_send))
        .route("/api/wake", post(post_wake))
        .route("/api/events", get(get_events))
        .with_state(state)
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

#[derive(Deserialize)]
struct SendRequest {
    body: String,
    from: Option<String>,
    subject: Option<String>,
}

async fn post_send(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SendRequest>,
) -> Json<Value> {
    let dir = &state.project_dir;
    let from = req.from.as_deref().unwrap_or("human");
    let subject = req.subject.unwrap_or_else(|| {
        let end = req.body.len().min(50);
        let mut e = end;
        while e > 0 && !req.body.is_char_boundary(e) {
            e -= 1;
        }
        req.body[..e].to_string()
    });

    let msg = message::Message {
        from: from.to_string(),
        subject,
        body: req.body.clone(),
        timestamp: chrono::Local::now().naive_local(),
        metadata: std::collections::BTreeMap::new(),
    };

    match message::write_message(dir, "inbox", &msg) {
        Ok(_) => Json(json!({"ok": true, "message": "Message sent"})),
        Err(e) => Json(json!({"ok": false, "message": format!("Failed: {e}")})),
    }
}

#[derive(Deserialize)]
struct WakeRequest {
    message: Option<String>,
}

async fn post_wake(
    State(state): State<Arc<AppState>>,
    Json(req): Json<WakeRequest>,
) -> Json<Value> {
    let dir = &state.project_dir;
    let body = req
        .message
        .as_deref()
        .unwrap_or("Wake requested from web UI.");

    let msg = message::Message {
        from: "operator".to_string(),
        subject: "Wake".to_string(),
        body: body.to_string(),
        timestamp: chrono::Local::now().naive_local(),
        metadata: std::collections::BTreeMap::new(),
    };

    if let Err(e) = message::write_message(dir, "inbox", &msg) {
        return Json(json!({"ok": false, "message": format!("Failed to write: {e}")}));
    }

    // Send SIGUSR1 to daemon
    let signaled = signal_daemon(dir);

    Json(json!({
        "ok": true,
        "message": if signaled { "Wake signal sent" } else { "Message queued (no daemon running)" }
    }))
}

async fn get_events(
    State(state): State<Arc<AppState>>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let rx = state.tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|result: Result<SseEvent, _>| {
        result.ok().map(|event| {
            let sse_event = match event {
                SseEvent::NewMessage {
                    direction,
                    from,
                    subject,
                    body,
                    timestamp,
                } => Event::default()
                    .event("message")
                    .json_data(json!({
                        "direction": direction,
                        "from": from,
                        "subject": subject,
                        "body": body,
                        "timestamp": timestamp,
                    }))
                    .unwrap(),
                SseEvent::StatusChange => Event::default().event("status").data("changed"),
                SseEvent::LogLine(line) => Event::default()
                    .event("log")
                    .json_data(json!({"line": line}))
                    .unwrap(),
            };
            Ok(sse_event)
        })
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

fn signal_daemon(dir: &std::path::Path) -> bool {
    if let Ok(Some(st)) = state::load_state(&state::state_path(dir)) {
        if let Some(pid) = st.pid {
            if state::is_locked(&st) {
                return crate::process::send_signal(pid, libc::SIGUSR1);
            }
        }
    }
    false
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
        let (tx, _rx) = tokio::sync::broadcast::channel::<SseEvent>(16);
        let state = AppState {
            project_dir: dir.path().to_path_buf(),
            tx,
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
        let (tx, _rx) = tokio::sync::broadcast::channel::<SseEvent>(16);
        let state = AppState {
            project_dir: dir.path().to_path_buf(),
            tx,
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

        let (tx, _rx) = tokio::sync::broadcast::channel::<SseEvent>(16);
        let state = AppState {
            project_dir: dir.path().to_path_buf(),
            tx,
        };
        let resp = get_messages(State(Arc::new(state))).await;
        let msgs: Vec<serde_json::Value> = serde_json::from_value(resp.0).unwrap();
        assert_eq!(msgs.len(), 2);
        // Sorted by timestamp — inbox first
        assert_eq!(msgs[0]["direction"], "inbox");
        assert_eq!(msgs[1]["direction"], "outbox");
    }

    #[tokio::test]
    async fn test_post_send_creates_inbox_message() {
        let dir = tempfile::tempdir().unwrap();
        crate::message::ensure_dirs(dir.path()).unwrap();
        let (tx, _rx) = tokio::sync::broadcast::channel::<SseEvent>(16);
        let state = Arc::new(AppState {
            project_dir: dir.path().to_path_buf(),
            tx,
        });

        let body = Json(SendRequest {
            body: "Please fix the bug".to_string(),
            from: Some("alice".to_string()),
            subject: Some("Bug report".to_string()),
        });
        let resp = post_send(State(state), body).await;
        assert!(resp.0["ok"].as_bool().unwrap());

        // Verify message was written to inbox
        let msgs = crate::message::read_inbox(dir.path()).unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].1.from, "alice");
        assert_eq!(msgs[0].1.body, "Please fix the bug");
    }

    #[tokio::test]
    async fn test_post_send_defaults() {
        let dir = tempfile::tempdir().unwrap();
        crate::message::ensure_dirs(dir.path()).unwrap();
        let (tx, _rx) = tokio::sync::broadcast::channel::<SseEvent>(16);
        let state = Arc::new(AppState {
            project_dir: dir.path().to_path_buf(),
            tx,
        });

        let body = Json(SendRequest {
            body: "Hello".to_string(),
            from: None,
            subject: None,
        });
        let resp = post_send(State(state), body).await;
        assert!(resp.0["ok"].as_bool().unwrap());

        let msgs = crate::message::read_inbox(dir.path()).unwrap();
        assert_eq!(msgs[0].1.from, "human");
    }

    #[tokio::test]
    async fn test_broadcast_channel() {
        let (tx, mut rx1) = tokio::sync::broadcast::channel::<SseEvent>(16);
        let mut rx2 = tx.subscribe();

        tx.send(SseEvent::StatusChange).unwrap();

        assert!(matches!(rx1.recv().await.unwrap(), SseEvent::StatusChange));
        assert!(matches!(rx2.recv().await.unwrap(), SseEvent::StatusChange));
    }
}
