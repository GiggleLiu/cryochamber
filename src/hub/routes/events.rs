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

use crate::hub::auth::AuthCtx;
use crate::hub::state::{AppState, SseEvent};
use crate::hub::tokens::Role;

/// How the per-event filter decides what this stream may carry.
///
/// The auth guard runs once, when the stream is opened; an SSE connection then
/// stays alive indefinitely. So the invite case re-reads the *live* token store
/// on every event: revoking an invite has to silence its already-open stream,
/// not just its next request. Without a store handle (unit tests, and any
/// caller that layers `Role` without `AuthCtx`) it falls back to the scope
/// frozen at connect time.
enum StreamScope {
    /// Owner, or open (loopback) mode.
    Unfiltered,
    Frozen(Vec<String>),
    Live {
        name: String,
        ctx: Arc<AuthCtx>,
    },
}

impl StreamScope {
    /// May this stream carry an event about `chamber_id` (`None` for
    /// index-level events, which carry no chamber content)?
    fn allows(&self, chamber_id: Option<&str>) -> bool {
        match self {
            Self::Unfiltered => true,
            Self::Frozen(chambers) => {
                chamber_id.is_none_or(|id| crate::hub::auth::scope_covers(chambers, id))
            }
            Self::Live { name, ctx } => {
                let store = ctx.store.read().expect("token store poisoned");
                // A revoked invite gets nothing at all — not even index events.
                let Some(invite) = store
                    .invites
                    .iter()
                    .find(|i| i.name == *name && i.revoked_at.is_none())
                else {
                    return false;
                };
                chamber_id.is_none_or(|id| crate::hub::auth::scope_covers(&invite.chambers, id))
            }
        }
    }
}

/// One SSE stream per client. An invite only sees events for the chambers its
/// token is scoped to — the stream is the one surface that would otherwise
/// push other chambers' message bodies to a guest without being asked.
/// `IndexChanged` carries no chamber content, so it reaches every live client.
pub async fn get_events(
    State(app): State<Arc<AppState>>,
    role: Option<axum::Extension<Role>>,
    ctx: Option<axum::Extension<Arc<AuthCtx>>>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let scope = match (role, ctx) {
        (Some(axum::Extension(Role::Invite { name, .. })), Some(axum::Extension(ctx))) => {
            StreamScope::Live { name, ctx }
        }
        (Some(axum::Extension(Role::Invite { chambers, .. })), None) => {
            StreamScope::Frozen(chambers)
        }
        // Owner, or open (local) mode: unfiltered.
        _ => StreamScope::Unfiltered,
    };
    let rx = app.tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(move |result: Result<SseEvent, _>| {
        let event = result.ok()?;
        let chamber_id = match &event {
            SseEvent::NewMessage { chamber_id, .. }
            | SseEvent::StatusChange { chamber_id }
            | SseEvent::LogLine { chamber_id, .. } => Some(chamber_id.as_str()),
            SseEvent::IndexChanged => None,
        };
        if !scope.allows(chamber_id) {
            return None;
        }
        let ev = match event {
            SseEvent::NewMessage {
                id,
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
                    "id": id,
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
