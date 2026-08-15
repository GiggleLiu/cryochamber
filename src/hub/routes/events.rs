//! GET /api/events — one SSE stream for the entire UI. Every event carries
//! `chamber_id` (except `IndexChanged`, which applies to the whole index).

use std::convert::Infallible;
use std::sync::Arc;

use axum::{
    extract::State,
    response::sse::{Event, KeepAlive, Sse},
};
use serde_json::json;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use crate::hub::state::{AppState, SseEvent};

/// One SSE stream per client. An invite only sees events for the chambers its
/// token is scoped to — the stream is the one surface that would otherwise
/// push other chambers' message bodies to a guest without being asked.
/// `IndexChanged` carries no chamber content, so it reaches everyone.
pub async fn get_events(
    State(app): State<Arc<AppState>>,
    role: Option<axum::Extension<crate::hub::tokens::Role>>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let scope: Option<Vec<String>> = match role {
        Some(axum::Extension(crate::hub::tokens::Role::Invite { chambers, .. })) => Some(chambers),
        // Owner, or open (local) mode: unfiltered.
        _ => None,
    };
    let rx = app.tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(move |result: Result<SseEvent, _>| {
        let event = result.ok()?;
        if let Some(scope) = &scope {
            let chamber_id = match &event {
                SseEvent::NewMessage { chamber_id, .. }
                | SseEvent::StatusChange { chamber_id }
                | SseEvent::LogLine { chamber_id, .. } => Some(chamber_id.as_str()),
                // Index-level: no chamber content, passes to every client.
                SseEvent::IndexChanged => None,
            };
            if let Some(id) = chamber_id {
                if !crate::hub::auth::scope_covers(scope, id) {
                    return None;
                }
            }
        }
        let ev = match event {
            SseEvent::NewMessage {
                chamber_id,
                direction,
                from,
                subject,
                body,
                timestamp,
                is_question,
            } => Event::default()
                .event("message")
                .json_data(json!({
                    "chamber_id": chamber_id,
                    "direction": direction,
                    "from": from,
                    "subject": subject,
                    "body": body,
                    "timestamp": timestamp,
                    "is_question": is_question,
                }))
                .unwrap(),
            SseEvent::StatusChange { chamber_id } => Event::default()
                .event("status")
                .json_data(json!({"chamber_id": chamber_id}))
                .unwrap(),
            SseEvent::LogLine { chamber_id, line } => Event::default()
                .event("log")
                .json_data(json!({"chamber_id": chamber_id, "line": line}))
                .unwrap(),
            SseEvent::IndexChanged => Event::default().event("index").data("changed"),
        };
        Some(Ok(ev))
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

#[cfg(test)]
#[path = "../../unit_tests/hub/routes/events.rs"]
mod tests;
