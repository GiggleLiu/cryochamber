//! Per-chamber HTTP handlers. All functions take `dir: &Path` so they can be
//! reused across chambers — nothing here is tied to a single global project.

use std::path::Path;
use std::sync::Arc;

use axum::{
    extract::{Path as AxumPath, State},
    http::StatusCode,
    response::Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::web::state::{AppState, SseEvent};

/// Build the JSON status payload for a single chamber.
pub fn status_json(dir: &Path) -> Value {
    let cfg = crate::config::load_config(&crate::config::config_path(dir))
        .ok()
        .flatten()
        .unwrap_or_default();

    let (running, session, agent) = match crate::state::load_state(&crate::state::state_path(dir))
        .ok()
        .flatten()
    {
        Some(st) => {
            let is_running = crate::state::is_locked(&st);
            let effective_agent = st
                .agent_override
                .as_deref()
                .unwrap_or(&cfg.agent)
                .to_string();
            (is_running, st.session_number, effective_agent)
        }
        None => (false, 0, cfg.agent.clone()),
    };

    let next_wake: Option<String> = {
        let todo_path = dir.join("todo.json");
        crate::todo::TodoList::load(&todo_path)
            .ok()
            .and_then(|list| list.next_wake_time().map(String::from))
    };

    let log_file = crate::log::log_path(dir);
    let log_tail = crate::log::read_current_session(&log_file)
        .ok()
        .flatten()
        .unwrap_or_default();
    let notes = crate::log::parse_latest_session_notes(&log_file).unwrap_or_default();
    let task = crate::log::parse_latest_session_task(&log_file)
        .ok()
        .flatten();

    let next_wake_rel = next_wake.as_deref().and_then(|w| {
        let wake = chrono::NaiveDateTime::parse_from_str(w, "%Y-%m-%dT%H:%M").ok()?;
        let now = chrono::Local::now().naive_local();
        let diff_ms = (wake - now).num_milliseconds();
        Some(format!(
            "{w} ({})",
            crate::web::format_relative_time(diff_ms)
        ))
    });

    json!({
        "running": running,
        "session": session,
        "agent": agent,
        "log_tail": log_tail,
        "next_wake": next_wake_rel,
        "notes": notes,
        "task": task,
    })
}

/// Build the list of all messages (archive + inbox + outbox) for a chamber.
pub fn messages_json(dir: &Path) -> Value {
    let mut all: Vec<Value> = Vec::new();
    let to_json = |msg: &crate::message::Message, direction: &str| -> Value {
        json!({
            "direction": direction,
            "from": msg.from,
            "subject": msg.subject,
            "body": msg.body,
            "timestamp": msg.timestamp.format("%Y-%m-%dT%H:%M:%S").to_string(),
        })
    };
    if let Ok(archived) = crate::message::read_inbox_archive(dir) {
        for (_f, m) in archived {
            all.push(to_json(&m, "inbox"));
        }
    }
    if let Ok(inbox) = crate::message::read_inbox(dir) {
        for (_f, m) in inbox {
            all.push(to_json(&m, "inbox"));
        }
    }
    if let Ok(outbox) = crate::message::read_outbox(dir) {
        for (_f, m) in outbox {
            all.push(to_json(&m, "outbox"));
        }
    }
    all.sort_by(|a, b| {
        a["timestamp"]
            .as_str()
            .unwrap_or("")
            .cmp(b["timestamp"].as_str().unwrap_or(""))
    });
    Value::Array(all)
}

pub async fn get_status(
    State(app): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<Value>, StatusCode> {
    let (path, _) = app.resolve(&id).ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(status_json(&path)))
}

pub async fn get_messages(
    State(app): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<Value>, StatusCode> {
    let (path, _) = app.resolve(&id).ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(messages_json(&path)))
}

#[derive(Deserialize)]
pub struct SendRequest {
    body: String,
    from: Option<String>,
    subject: Option<String>,
}

pub async fn post_send(
    State(app): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
    Json(req): Json<SendRequest>,
) -> Result<Json<Value>, StatusCode> {
    let (path, entry) = app.resolve(&id).ok_or(StatusCode::NOT_FOUND)?;
    let msg = crate::message::Message {
        from: req.from.unwrap_or_else(|| "human".into()),
        subject: req.subject.unwrap_or_default(),
        body: req.body,
        timestamp: chrono::Local::now().naive_local(),
        metadata: Default::default(),
    };
    match crate::message::write_message(&path, "inbox", &msg) {
        Ok(_) => {
            let _ = app.tx.send(SseEvent::NewMessage {
                chamber_id: entry.id,
                direction: "inbox".into(),
                from: msg.from.clone(),
                subject: msg.subject.clone(),
                body: msg.body.clone(),
                timestamp: msg.timestamp.format("%Y-%m-%dT%H:%M:%S").to_string(),
            });
            Ok(Json(json!({"ok": true, "message": "Message sent"})))
        }
        Err(e) => Ok(Json(
            json!({"ok": false, "message": format!("Failed: {e}")}),
        )),
    }
}

#[derive(Deserialize, Default)]
pub struct WakeRequest {
    message: Option<String>,
}

pub async fn post_wake(
    State(app): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
    Json(req): Json<WakeRequest>,
) -> Result<Json<Value>, StatusCode> {
    let (path, _entry) = app.resolve(&id).ok_or(StatusCode::NOT_FOUND)?;
    let body = req
        .message
        .unwrap_or_else(|| "Wake requested from web UI.".into());
    let msg = crate::message::Message {
        from: "operator".into(),
        subject: "Wake".into(),
        body,
        timestamp: chrono::Local::now().naive_local(),
        metadata: Default::default(),
    };
    if let Err(e) = crate::message::write_message(&path, "inbox", &msg) {
        return Ok(Json(
            json!({"ok": false, "message": format!("Failed: {e}")}),
        ));
    }
    let signaled = crate::process::signal_daemon_wake(&path);
    Ok(Json(json!({
        "ok": true,
        "message": if signaled { "Wake signal sent" } else { "Message queued (no daemon running)" }
    })))
}

use crate::web::discovery::Source;

fn require_workspace(entry: &crate::web::discovery::ChamberEntry) -> Result<(), StatusCode> {
    if entry.source == Source::External {
        return Err(StatusCode::CONFLICT);
    }
    Ok(())
}

pub async fn post_start(
    State(app): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<Value>, StatusCode> {
    let (path, entry) = app.resolve(&id).ok_or(StatusCode::NOT_FOUND)?;
    require_workspace(&entry)?;
    let result = crate::web::lifecycle::start_chamber(&path);
    app.refresh();
    match result {
        Ok(()) => Ok(Json(json!({"ok": true, "message": "Started"}))),
        Err(e) => Ok(Json(json!({"ok": false, "message": e.to_string()}))),
    }
}

pub async fn post_stop(
    State(app): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<Value>, StatusCode> {
    let (path, entry) = app.resolve(&id).ok_or(StatusCode::NOT_FOUND)?;
    require_workspace(&entry)?;
    let result = crate::web::lifecycle::stop_chamber(&path);
    app.refresh();
    match result {
        Ok(()) => Ok(Json(json!({"ok": true, "message": "Stopped"}))),
        Err(e) => Ok(Json(json!({"ok": false, "message": e.to_string()}))),
    }
}

pub async fn post_restart(
    State(app): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<Value>, StatusCode> {
    let (path, entry) = app.resolve(&id).ok_or(StatusCode::NOT_FOUND)?;
    require_workspace(&entry)?;
    let result = crate::web::lifecycle::restart_chamber(&path);
    app.refresh();
    match result {
        Ok(()) => Ok(Json(json!({"ok": true, "message": "Restarted"}))),
        Err(e) => Ok(Json(json!({"ok": false, "message": e.to_string()}))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_json_for_missing_state_has_zero_session() {
        let dir = tempfile::tempdir().unwrap();
        let v = status_json(dir.path());
        assert_eq!(v["running"], false);
        assert_eq!(v["session"], 0);
    }

    #[test]
    fn messages_json_sorted_by_timestamp() {
        let dir = tempfile::tempdir().unwrap();
        crate::message::ensure_dirs(dir.path()).unwrap();
        let early = crate::message::Message {
            from: "a".into(),
            subject: "".into(),
            body: "first".into(),
            timestamp: chrono::NaiveDate::from_ymd_opt(2026, 1, 1)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap(),
            metadata: Default::default(),
        };
        let late = crate::message::Message {
            from: "b".into(),
            subject: "".into(),
            body: "second".into(),
            timestamp: chrono::NaiveDate::from_ymd_opt(2026, 1, 2)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap(),
            metadata: Default::default(),
        };
        crate::message::write_message(dir.path(), "inbox", &late).unwrap();
        crate::message::write_message(dir.path(), "outbox", &early).unwrap();
        let arr = messages_json(dir.path());
        let arr = arr.as_array().unwrap();
        assert_eq!(arr[0]["body"], "first");
        assert_eq!(arr[1]["body"], "second");
    }

    #[tokio::test]
    async fn start_stop_restart_return_409_for_external() {
        let dir = tempfile::tempdir().unwrap();
        let external = dir.path().join("outside");
        std::fs::create_dir_all(&external).unwrap();

        let app = Arc::new(AppState::new(dir.path().to_path_buf()));
        // Inject a synthetic external entry directly into the index
        let id = {
            let id = crate::web::discovery::encode_id(&external.canonicalize().unwrap());
            let entry = crate::web::discovery::ChamberEntry {
                id: id.clone(),
                name: "outside".into(),
                path: external.canonicalize().unwrap(),
                source: Source::External,
                config_error: None,
                running: true,
                session: None,
                next_wake: None,
                unread: 0,
            };
            app.chambers.write().unwrap().insert(id.clone(), entry);
            id
        };

        let err = post_start(State(app.clone()), AxumPath(id.clone()))
            .await
            .unwrap_err();
        assert_eq!(err, StatusCode::CONFLICT);

        let err = post_stop(State(app.clone()), AxumPath(id.clone()))
            .await
            .unwrap_err();
        assert_eq!(err, StatusCode::CONFLICT);

        let err = post_restart(State(app), AxumPath(id)).await.unwrap_err();
        assert_eq!(err, StatusCode::CONFLICT);
    }
}
