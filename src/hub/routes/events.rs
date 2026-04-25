//! GET /api/events — one SSE stream for the entire UI. Every event carries
//! `chamber_id` (except `IndexChanged`, which is workspace-level).

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

pub async fn get_events(
    State(app): State<Arc<AppState>>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let rx = app.tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|result: Result<SseEvent, _>| {
        result.ok().map(|event| {
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
            Ok(ev)
        })
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

#[cfg(test)]
#[path = "../../unit_tests/hub/routes/events.rs"]
mod tests;
