use crate::channel::store::MessageStore;
use crate::message::Message;
use pulldown_cmark::{html, Event, Options, Parser, Tag, TagEnd};
use serde::Serialize;
use std::path::Path;

/// Parse `cryo.toml` text into a flat list of display rows for the Settings
/// drawer. Top-level scalars become single rows; the `provider` table and
/// legacy `providers` array are expanded to provider rows (name + the *keys*
/// of env maps, never the values, since they may hold API secrets). Unknown
/// sections collapse to a placeholder row. Returns an empty vector for empty
/// or unparseable input so the frontend can fall back to the raw text view.
pub fn parse_settings_rows(src: &str) -> Vec<SettingsRow> {
    if src.is_empty() {
        return Vec::new();
    }
    // The toml v1 crate's `s.parse::<Value>()` parses a single value, not a
    // document. For a TOML document we need `from_str::<Table>`.
    let table: toml::Table = match toml::from_str(src) {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };

    let mut rows = Vec::new();
    for (key, val) in &table {
        match val {
            toml::Value::String(s) => rows.push(SettingsRow {
                key: key.clone(),
                value: format!("\"{s}\""),
                kind: "scalar".into(),
            }),
            toml::Value::Integer(i) => rows.push(SettingsRow {
                key: key.clone(),
                value: i.to_string(),
                kind: "scalar".into(),
            }),
            toml::Value::Float(f) => rows.push(SettingsRow {
                key: key.clone(),
                value: f.to_string(),
                kind: "scalar".into(),
            }),
            toml::Value::Boolean(b) => rows.push(SettingsRow {
                key: key.clone(),
                value: b.to_string(),
                kind: "scalar".into(),
            }),
            toml::Value::Datetime(d) => rows.push(SettingsRow {
                key: key.clone(),
                value: d.to_string(),
                kind: "scalar".into(),
            }),
            toml::Value::Array(arr) if key == "providers" => {
                for (i, prov) in arr.iter().enumerate() {
                    rows.push(provider_row(i, prov));
                }
            }
            toml::Value::Array(arr) if arr.iter().all(is_scalar) => rows.push(SettingsRow {
                key: key.clone(),
                value: format_scalar_array(arr),
                kind: "scalar".into(),
            }),
            toml::Value::Array(arr) => rows.push(SettingsRow {
                key: key.clone(),
                value: format!("[{} items]", arr.len()),
                kind: "section".into(),
            }),
            toml::Value::Table(table) if key == "provider" => {
                rows.push(provider_table_row("provider".to_string(), table));
            }
            toml::Value::Table(_) => rows.push(SettingsRow {
                key: key.clone(),
                value: "[table]".into(),
                kind: "section".into(),
            }),
        }
    }
    rows
}

fn is_scalar(value: &toml::Value) -> bool {
    matches!(
        value,
        toml::Value::String(_)
            | toml::Value::Integer(_)
            | toml::Value::Float(_)
            | toml::Value::Boolean(_)
            | toml::Value::Datetime(_)
    )
}

fn format_scalar(value: &toml::Value) -> String {
    match value {
        toml::Value::String(s) => format!("\"{s}\""),
        toml::Value::Integer(i) => i.to_string(),
        toml::Value::Float(f) => f.to_string(),
        toml::Value::Boolean(b) => b.to_string(),
        toml::Value::Datetime(d) => d.to_string(),
        _ => String::new(),
    }
}

fn format_scalar_array(arr: &[toml::Value]) -> String {
    let parts: Vec<String> = arr.iter().map(format_scalar).collect();
    format!("[{}]", parts.join(", "))
}

fn provider_row(index: usize, value: &toml::Value) -> SettingsRow {
    let table = match value.as_table() {
        Some(t) => t,
        None => {
            return SettingsRow {
                key: format!("providers[{index}]"),
                value: "(invalid)".into(),
                kind: "section".into(),
            }
        }
    };
    provider_table_row(format!("providers[{index}]"), table)
}

fn provider_table_row(key: String, table: &toml::Table) -> SettingsRow {
    let name = table
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("(unnamed)");
    let env_keys: Vec<&str> = table
        .get("env")
        .and_then(|v| v.as_table())
        .map(|env| env.keys().map(String::as_str).collect())
        .unwrap_or_default();
    let env_summary = if env_keys.is_empty() {
        String::new()
    } else {
        format!(" · env: {}", env_keys.join(", "))
    };
    SettingsRow {
        key,
        value: format!("{name}{env_summary}"),
        kind: "section".into(),
    }
}

/// Render markdown to HTML, escaping any embedded raw HTML so plan.md can't
/// inject script tags into the hub UI. Output is empty when input is empty.
pub fn render_markdown_safe(src: &str) -> String {
    if src.is_empty() {
        return String::new();
    }
    let opts = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let parser = Parser::new_ext(src, opts).map(|event| match event {
        Event::Html(s) | Event::InlineHtml(s) => Event::Text(s),
        // Drop raw images — they're a side-channel for fetching arbitrary URLs
        // from the operator's browser, and plan.md is reference text, not a
        // gallery. Keeps the rendered output minimal and predictable.
        Event::Start(Tag::Image { .. }) => Event::Start(Tag::Emphasis),
        Event::End(TagEnd::Image) => Event::End(TagEnd::Emphasis),
        other => other,
    });
    let mut out = String::with_capacity(src.len() + src.len() / 4);
    html::push_html(&mut out, parser);
    out
}

#[derive(Debug, Clone, Serialize)]
pub struct ChamberStatus {
    pub running: bool,
    pub agent_running: bool,
    pub session: u32,
    pub agent: String,
    pub log_tail: String,
    pub daily_digests: Vec<crate::log::DailyDigest>,
    pub next_wake: Option<String>,
    pub notes_content: String,
    pub notes_html: String,
    pub plan_content: String,
    pub plan_html: String,
    pub config_content: String,
    pub settings_rows: Vec<SettingsRow>,
    pub task: Option<String>,
    pub session_summary: Option<String>,
    pub completed: bool,
    pub completion_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SettingsRow {
    pub key: String,
    pub value: String,
    pub kind: String,
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
    pub is_question: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChamberSyncBadge {
    pub backend: String,
    pub running: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChamberOverview {
    pub running: bool,
    pub agent_running: bool,
    pub session: Option<u32>,
    pub next_wake: Option<String>,
    pub next_wake_display: Option<String>,
    pub wake_imminent: bool,
    pub has_open_question: bool,
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

    let state = crate::state::load_state(&crate::state::state_path(dir))
        .ok()
        .flatten();
    let (running, agent_running) = runtime_flags(state.as_ref());
    let session = state.as_ref().map(|st| st.session_number).unwrap_or(0);
    let agent = state
        .as_ref()
        .and_then(|st| st.agent_override.as_deref())
        .unwrap_or(&cfg.agent)
        .to_string();

    let log_file = crate::log::log_path(dir);
    let completion_summary = crate::log::parse_latest_session_plan_complete(&log_file)
        .ok()
        .flatten();
    let completed = completion_summary.is_some();

    let plan_content = std::fs::read_to_string(dir.join("plan.md")).unwrap_or_default();
    let plan_html = render_markdown_safe(&plan_content);
    let notes_content = std::fs::read_to_string(dir.join("NOTES.md")).unwrap_or_default();
    let notes_html = render_markdown_safe(&notes_content);
    let config_content = std::fs::read_to_string(dir.join("cryo.toml")).unwrap_or_default();
    let settings_rows = parse_settings_rows(&config_content);

    ChamberStatus {
        running,
        agent_running,
        session,
        agent,
        log_tail: crate::log::read_recent_sessions(&log_file, 5)
            .ok()
            .flatten()
            .unwrap_or_default(),
        daily_digests: crate::log::daily_digests(&log_file, 14).unwrap_or_default(),
        next_wake: next_wake(dir),
        notes_content,
        notes_html,
        plan_content,
        plan_html,
        config_content,
        settings_rows,
        task: crate::log::parse_latest_session_task(&log_file)
            .ok()
            .flatten(),
        session_summary: crate::log::parse_latest_session_summary(&log_file)
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

    let (running, agent_running) = runtime_flags(state.as_ref());

    ChamberOverview {
        running,
        agent_running,
        session: state.as_ref().map(|st| st.session_number),
        next_wake_display: next_wake.clone(),
        wake_imminent: wake_imminent(next_wake.as_deref()),
        next_wake,
        has_open_question: has_open_question_for(&store),
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

/// True iff the chamber has an open agent-asked question — that is, the
/// most recent outbox message marked `is_question` is newer than the most
/// recent human inbox reply (across `inbox/` and `inbox/archive/`).
///
/// `operator` and `cryochamber` inbox messages are *not* counted as
/// replies; they represent system actions (wake signals, fallback notices)
/// rather than answers from the user.
pub fn has_open_question(dir: &Path) -> bool {
    has_open_question_for(&MessageStore::new(dir.to_path_buf()))
}

fn has_open_question_for(store: &MessageStore) -> bool {
    let last_human_reply_ts = latest_human_inbox_timestamp(store);
    let mut outbox: Vec<(String, Message)> = Vec::new();
    if let Ok(messages) = store.read_outbox_named() {
        outbox.extend(messages);
    }
    if let Ok(messages) = store.read_outbox_archive_named() {
        outbox.extend(messages);
    }
    // Reverse-chrono walk: stop at the first message strictly older than the
    // last human reply, and return true on the first question seen above that.
    // Message timestamps only persist second precision, so equality is not
    // enough to prove a human reply happened after an agent question.
    outbox.sort_by(|left, right| {
        right
            .1
            .timestamp
            .cmp(&left.1.timestamp)
            .then_with(|| right.0.cmp(&left.0))
    });
    for (_, msg) in outbox {
        if let Some(reply_ts) = last_human_reply_ts {
            if msg.timestamp < reply_ts {
                return false;
            }
        }
        if msg.is_question {
            return true;
        }
    }
    false
}

fn latest_human_inbox_timestamp(store: &MessageStore) -> Option<chrono::NaiveDateTime> {
    let mut messages = Vec::new();
    if let Ok(inbox) = store.read_inbox_named() {
        messages.extend(inbox);
    }
    if let Ok(archive) = store.read_inbox_archive_named() {
        messages.extend(archive);
    }
    messages
        .into_iter()
        .filter(|(_, msg)| is_human_reply_sender(&msg.from))
        .map(|(_, msg)| msg.timestamp)
        .max()
}

fn is_human_reply_sender(from: &str) -> bool {
    !matches!(from, "operator" | "cryochamber" | "agent")
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

fn runtime_flags(state: Option<&crate::state::CryoState>) -> (bool, bool) {
    let running = state.map(crate::state::is_locked).unwrap_or(false);
    let agent_running = state
        .map(|st| st.session_active && running)
        .unwrap_or(false);
    (running, agent_running)
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
        is_question: msg.is_question,
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
