//! GET /api/events — one SSE stream for the entire UI. Every event carries
//! `chamber_id` (except `IndexChanged`, which applies to the whole index, and
//! `resync`, which tells a client whose receiver overflowed to refetch).

use std::convert::Infallible;
use std::sync::Arc;

use axum::{
    extract::State,
    response::sse::{Event, KeepAlive, Sse},
};
use serde_json::json;
use tokio_stream::wrappers::{BroadcastStream, IntervalStream};
use tokio_stream::StreamExt;

use crate::hub::auth::{AuthCtx, BearerToken};
use crate::hub::state::{AppState, SseEvent};
use crate::hub::tokens::Role;

/// How the per-event filter decides what this stream may carry.
///
/// The auth guard runs once, when the stream is opened; an SSE connection then
/// stays alive indefinitely. So in public mode *every* stream — owner and
/// invite alike — re-authorizes against the live token store on each event:
/// revoking an invite, or rotating the owner token, has to end its already-open
/// stream, not just its next request. Without a store handle (unit tests that
/// layer a `Role` without `AuthCtx`) it falls back to the scope frozen at
/// connect time.
enum StreamScope {
    /// Open (loopback) mode: no roles, nothing filtered.
    Unfiltered,
    /// A guest scope fixed at connect time (no live store available).
    Frozen(Vec<String>),
    /// Public mode. Re-resolved per event.
    Live {
        /// The exact credential this stream was opened with — never the invite
        /// name. Names are reusable after revocation: binding to one let a
        /// revoked stream resume under a replacement invite that happened to
        /// reuse the name, without ever presenting its secret.
        token: String,
        ctx: Arc<AuthCtx>,
    },
}

/// How often an open stream re-checks that its credential still resolves, so a
/// revoked guest is cut off even when the chamber is idle and no event would
/// otherwise trigger the check.
const REAUTH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);

impl StreamScope {
    /// Does the credential this stream was opened with still resolve? A stream
    /// whose token was revoked (or replaced) must *end*, not fall silent: a
    /// silent stream keeps the client believing it is signed in, on cached
    /// data, until it happens to make another request.
    fn still_authorized(&self) -> bool {
        match self {
            Self::Unfiltered | Self::Frozen(_) => true,
            Self::Live { token, ctx } => ctx.resolve(token).is_some(),
        }
    }

    /// May this stream carry `event`? Resolves the credential once and applies
    /// both rules: chamber scope (a guest sees only its chambers) and content
    /// class (a guest never sees log lines — tool output can carry paths or
    /// credentials that were never meant to leave the owner's screen).
    fn admits(&self, event: &SseEvent) -> bool {
        let chamber_id = match event {
            SseEvent::NewMessage { chamber_id, .. }
            | SseEvent::StatusChange { chamber_id }
            | SseEvent::LogLine { chamber_id, .. } => Some(chamber_id.as_str()),
            SseEvent::IndexChanged => None,
        };
        let is_log = matches!(event, SseEvent::LogLine { .. });
        let guest_scope: Option<Vec<String>> = match self {
            Self::Unfiltered => return true,
            Self::Frozen(chambers) => Some(chambers.clone()),
            Self::Live { token, ctx } => match ctx.resolve(token) {
                // Revoked, or replaced: nothing at all reaches this stream,
                // not even index events.
                None => return false,
                Some(Role::Owner) => None,
                Some(Role::Invite { chambers, .. }) => Some(chambers),
            },
        };
        match guest_scope {
            None => true,
            Some(chambers) => {
                !is_log && chamber_id.is_none_or(|id| crate::hub::auth::scope_covers(&chambers, id))
            }
        }
    }
}

/// One item of the merged stream: a broadcast event, a periodic tick that
/// exists only to re-run the authorization check on an otherwise idle stream,
/// or the receiver's report that it fell behind and events were evicted.
enum Tick {
    Event(SseEvent),
    Reauth,
    /// The broadcast buffer overflowed for this connection. Whatever was
    /// evicted is gone; the client is told to refetch rather than left
    /// believing it is current.
    Resync,
}

/// One SSE stream per client. An invite only sees events for the chambers its
/// token is scoped to — the stream is the one surface that would otherwise
/// push other chambers' message bodies to a guest without being asked.
/// `IndexChanged` carries no chamber content, so it reaches every live client.
pub async fn get_events(
    State(app): State<Arc<AppState>>,
    role: Option<axum::Extension<Role>>,
    ctx: Option<axum::Extension<Arc<AuthCtx>>>,
    token: Option<axum::Extension<BearerToken>>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let scope = match (role, ctx, token) {
        // Public mode: the guard supplied the store and the credential, so the
        // stream re-authorizes live — for the owner exactly as for a guest.
        (Some(_), Some(axum::Extension(ctx)), Some(axum::Extension(BearerToken(token)))) => {
            StreamScope::Live { token, ctx }
        }
        (Some(axum::Extension(Role::Invite { chambers, .. })), _, _) => {
            StreamScope::Frozen(chambers)
        }
        // Owner without a store (unit tests), or open (local) mode: unfiltered.
        _ => StreamScope::Unfiltered,
    };
    let rx = app.tx.subscribe();
    let events = BroadcastStream::new(rx).map(|result| match result {
        Ok(event) => Tick::Event(event),
        Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(_)) => Tick::Resync,
    });
    let mut interval = tokio::time::interval(REAUTH_INTERVAL);
    // The first tick of `interval` fires immediately; skipping it here keeps
    // the initial connect free of an extra check that the guard just did.
    interval.reset();
    let reauth = IntervalStream::new(interval).map(|_| Tick::Reauth);
    let scope_for_end = std::sync::Arc::new(scope);
    let scope = scope_for_end.clone();
    // `take_while` ends the response the first time the credential no longer
    // resolves — on the next event, or on the next re-auth tick if idle. The
    // client sees EOF, reconnects, and is told 401.
    let stream = events
        .merge(reauth)
        .take_while(move |_| scope_for_end.still_authorized())
        .filter_map(move |tick| {
            let event = match tick {
                Tick::Event(event) => event,
                Tick::Reauth => return None,
                Tick::Resync => {
                    return Some(Ok(Event::default().event("resync").data("{}")));
                }
            };
            if !scope.admits(&event) {
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
