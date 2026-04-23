use crate::channel::store::MessageStore;
use crate::message::Message;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct ChamberStatus {
    pub running: bool,
    pub session: u32,
    pub agent: String,
    pub log_tail: String,
    pub next_wake: Option<String>,
    pub notes_content: String,
    pub task: Option<String>,
    pub completed: bool,
    pub completion_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChamberMessage {
    pub id: String,
    pub direction: String,
    pub from: String,
    pub subject: String,
    pub body: String,
    pub timestamp: String,
    pub session: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChamberSyncBadge {
    pub backend: String,
    pub running: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChamberOverview {
    pub running: bool,
    pub session: Option<u32>,
    pub next_wake: Option<String>,
    pub next_wake_display: Option<String>,
    pub wake_imminent: bool,
    pub unread: usize,
    pub task: Option<String>,
    pub last_message_preview: Option<String>,
    pub completed: bool,
    pub sync: Vec<ChamberSyncBadge>,
}

pub fn status(dir: &Path) -> ChamberStatus {
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

    let log_file = crate::log::log_path(dir);
    let completion_summary = crate::log::parse_latest_session_plan_complete(&log_file)
        .ok()
        .flatten();
    let completed = completion_summary.is_some();

    ChamberStatus {
        running,
        session,
        agent,
        log_tail: crate::log::read_recent_sessions(&log_file, 5)
            .ok()
            .flatten()
            .unwrap_or_default(),
        next_wake: next_wake(dir),
        notes_content: std::fs::read_to_string(dir.join("NOTES.md")).unwrap_or_default(),
        task: crate::log::parse_latest_session_task(&log_file)
            .ok()
            .flatten(),
        completed,
        completion_summary,
    }
}

pub fn messages(dir: &Path) -> Vec<ChamberMessage> {
    let store = MessageStore::new(dir.to_path_buf());
    let sessions = message_sessions(dir);
    let mut all = Vec::new();
    collect_messages(&store, &sessions, "inbox/archive", "inbox", &mut all);
    collect_messages(&store, &sessions, "inbox", "inbox", &mut all);
    collect_messages(&store, &sessions, "outbox", "outbox", &mut all);
    collect_messages(&store, &sessions, "outbox/archive", "outbox", &mut all);
    all.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    all
}

pub fn todos(dir: &Path) -> Vec<crate::todo::TodoItem> {
    crate::todo::TodoFile::new(dir.join("todo.json"))
        .items()
        .unwrap_or_default()
}

pub fn overview(dir: &Path) -> ChamberOverview {
    let store = MessageStore::new(dir.to_path_buf());
    let state = crate::state::load_state(&crate::state::state_path(dir))
        .ok()
        .flatten();
    let next_wake = next_wake(dir);
    let log_file = crate::log::log_path(dir);

    ChamberOverview {
        running: state.as_ref().map(crate::state::is_locked).unwrap_or(false),
        session: state.as_ref().map(|st| st.session_number),
        next_wake_display: next_wake.clone(),
        wake_imminent: wake_imminent(next_wake.as_deref()),
        next_wake,
        unread: store
            .read_inbox_named()
            .map(|messages| messages.len())
            .unwrap_or(0),
        task: crate::log::parse_latest_session_task(&log_file)
            .ok()
            .flatten(),
        last_message_preview: last_message_preview(dir),
        completed: crate::log::parse_latest_session_plan_complete(&log_file)
            .ok()
            .flatten()
            .is_some(),
        sync: crate::sync_control::summarize_all(dir)
            .into_iter()
            .map(|summary| ChamberSyncBadge {
                backend: summary.backend.as_str().to_string(),
                running: summary.running,
            })
            .collect(),
    }
}

fn next_wake(dir: &Path) -> Option<String> {
    crate::todo::TodoFile::new(dir.join("todo.json"))
        .next_wake_time()
        .ok()
        .flatten()
}

fn wake_imminent(next_wake: Option<&str>) -> bool {
    next_wake
        .and_then(|w| chrono::NaiveDateTime::parse_from_str(w, "%Y-%m-%dT%H:%M").ok())
        .map(|wake| {
            let diff = wake - chrono::Local::now().naive_local();
            let diff_ms = diff.num_milliseconds();
            (0..=3_600_000).contains(&diff_ms)
        })
        .unwrap_or(false)
}

fn message_sessions(dir: &Path) -> Vec<crate::log::SessionSummary> {
    let epoch = chrono::NaiveDate::from_ymd_opt(1970, 1, 1)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap();
    crate::log::parse_sessions_since(&dir.join("cryo.log"), epoch).unwrap_or_default()
}

fn session_for_message(
    sessions: &[crate::log::SessionSummary],
    msg_ts: chrono::NaiveDateTime,
) -> Option<u32> {
    let mut current = None;
    for session in sessions {
        if session.timestamp <= msg_ts {
            current = Some(session.session_number);
        } else {
            break;
        }
    }
    current
}

fn collect_messages(
    store: &MessageStore,
    sessions: &[crate::log::SessionSummary],
    source: &str,
    direction: &str,
    out: &mut Vec<ChamberMessage>,
) {
    let messages = match source {
        "inbox/archive" => store.read_inbox_archive_named(),
        "inbox" => store.read_inbox_named(),
        "outbox" => store.read_outbox_named(),
        "outbox/archive" => store.read_outbox_archive_named(),
        _ => return,
    };
    if let Ok(messages) = messages {
        for (filename, msg) in messages {
            out.push(message_model(&filename, &msg, direction, source, sessions));
        }
    }
}

fn message_model(
    filename: &str,
    msg: &Message,
    direction: &str,
    source: &str,
    sessions: &[crate::log::SessionSummary],
) -> ChamberMessage {
    ChamberMessage {
        id: format!("{source}/{filename}"),
        direction: direction.to_string(),
        from: msg.from.clone(),
        subject: msg.subject.clone(),
        body: msg.body.clone(),
        timestamp: msg.timestamp.format("%Y-%m-%dT%H:%M:%S").to_string(),
        session: session_for_message(sessions, msg.timestamp),
    }
}

fn last_message_preview(dir: &Path) -> Option<String> {
    let store = MessageStore::new(dir.to_path_buf());
    let mut messages = Vec::new();
    if let Ok(archived) = store.read_inbox_archive_named() {
        messages.extend(archived);
    }
    if let Ok(inbox) = store.read_inbox_named() {
        messages.extend(inbox);
    }
    if let Ok(outbox) = store.read_outbox_named() {
        messages.extend(outbox);
    }
    if let Ok(archived) = store.read_outbox_archive_named() {
        messages.extend(archived);
    }
    messages
        .into_iter()
        .max_by(|(file_a, msg_a), (file_b, msg_b)| {
            msg_a
                .timestamp
                .cmp(&msg_b.timestamp)
                .then_with(|| file_a.cmp(file_b))
        })
        .and_then(|(_, msg)| preview_body(&msg.body))
}

fn preview_body(body: &str) -> Option<String> {
    let line = body.lines().find(|line| !line.trim().is_empty())?.trim();
    if line.chars().count() <= 120 {
        return Some(line.to_string());
    }
    Some(line.chars().take(117).collect::<String>() + "...")
}

#[cfg(test)]
#[path = "unit_tests/chamber_status.rs"]
mod tests;
