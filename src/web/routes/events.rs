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

use crate::web::state::{AppState, SseEvent};

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
                } => Event::default()
                    .event("message")
                    .json_data(json!({
                        "chamber_id": chamber_id,
                        "direction": direction,
                        "from": from,
                        "subject": subject,
                        "body": body,
                        "timestamp": timestamp,
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
mod tests {
    use super::*;

    #[tokio::test]
    async fn broadcast_multiplexes_by_chamber_id() {
        let (tx, mut rx_a) = tokio::sync::broadcast::channel::<SseEvent>(16);
        let mut rx_b = tx.subscribe();
        tx.send(SseEvent::StatusChange {
            chamber_id: "alpha".into(),
        })
        .unwrap();
        let a = rx_a.recv().await.unwrap();
        let b = rx_b.recv().await.unwrap();
        match (a, b) {
            (
                SseEvent::StatusChange { chamber_id: ca },
                SseEvent::StatusChange { chamber_id: cb },
            ) => {
                assert_eq!(ca, "alpha");
                assert_eq!(cb, "alpha");
            }
            _ => panic!("expected StatusChange"),
        }
    }
}
