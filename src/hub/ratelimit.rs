//! Per-credential write throttle for public mode.
//!
//! Every accepted `send` wakes an agent (an LLM session the owner pays for) and
//! every accepted upload lands on the owner's disk, so a guest with an invite
//! link must not be able to do either without bound. A token bucket per
//! credential — small burst, slow refill — is enough: it leaves conversation
//! pace untouched and turns a loop into a `429`.
//!
//! The clock is a parameter (`check_at`) so the tests are exact and sleepless.
//!
//! The bucket is stored as a single deadline rather than a running float
//! balance. A float balance has to be re-based on every call, and re-basing
//! accumulates rounding error: with a 10/min refill, five 1-second steps plus
//! one more land on `0.9999999999999999` tokens, so a token that is exactly due
//! reads as not-yet-due. `Duration` arithmetic has no such drift.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::{
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};

use crate::hub::tokens::Role;

/// Sends/uploads a fresh credential may make at once.
pub const WRITE_BURST: u32 = 5;
/// Sustained rate after the burst is spent.
pub const WRITE_REFILL_PER_MIN: u32 = 10;

/// What the limiter decided for one call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow,
    /// Refused; a retry is pointless before `retry_after` has elapsed.
    Deny {
        retry_after: Duration,
    },
}

pub struct RateLimiter {
    /// Seconds' worth of one token — the sustained spacing between calls.
    interval: Duration,
    /// How far ahead of `now` a bucket may run: `(capacity - 1)` intervals,
    /// which is exactly what lets `capacity` calls through back to back.
    burst: Duration,
    /// Per key: the earliest instant at which the bucket is empty again. A key
    /// absent from the map is a full bucket.
    deadlines: Mutex<HashMap<String, Instant>>,
}

impl RateLimiter {
    /// `capacity` calls at once, then `refill_per_min` calls per minute. Both
    /// arguments are clamped to at least 1 — a limiter that refuses everything
    /// or refills never is not a configuration this hub has any use for.
    pub fn new(capacity: u32, refill_per_min: u32) -> Self {
        let interval = Duration::from_secs(60) / refill_per_min.max(1);
        Self {
            interval,
            burst: interval * (capacity.max(1) - 1),
            deadlines: Mutex::new(HashMap::new()),
        }
    }

    /// The gate the write routes share: spend one token for `role` and return
    /// `None`, or return the `429` the handler must answer with. Roleless
    /// (open-mode) callers have no bucket and are never refused — see
    /// [`write_key`].
    pub fn refuse(&self, role: Option<&Role>) -> Option<Response> {
        match self.check(&write_key(role)?) {
            Decision::Allow => None,
            Decision::Deny { retry_after } => Some(too_many_requests(retry_after)),
        }
    }

    /// Take one token for `key` now.
    pub fn check(&self, key: &str) -> Decision {
        self.check_at(key, Instant::now())
    }

    /// Take one token for `key` as of `now`. Buckets start full.
    pub fn check_at(&self, key: &str, now: Instant) -> Decision {
        let mut deadlines = self.deadlines.lock().unwrap_or_else(|p| p.into_inner());
        let empty_at = deadlines.entry(key.to_string()).or_insert(now);
        // The bucket may run up to `burst` ahead of the wall clock; past that,
        // the caller has spent tokens it has not earned yet.
        let ceiling = now + self.burst;
        if *empty_at <= ceiling {
            // `max(now)` discards credit for idle time beyond a full bucket.
            *empty_at = (*empty_at).max(now) + self.interval;
            Decision::Allow
        } else {
            Decision::Deny {
                retry_after: empty_at.duration_since(ceiling),
            }
        }
    }
}

/// The bucket a request draws from. Open (loopback) mode has no role layer and
/// no bucket: that user already has shell access to the chamber, and throttling
/// the operator's own console would only annoy them. The limiter exists for
/// public mode, where a guest's send is somebody else's LLM bill.
pub fn write_key(role: Option<&Role>) -> Option<String> {
    match role {
        Some(Role::Owner) => Some("owner".to_string()),
        Some(Role::Invite { name, .. }) => Some(format!("invite:{name}")),
        None => None,
    }
}

/// `429` with a whole-second `Retry-After` and the shared `{error}` body shape.
///
/// The header is never `0`: a sub-second wait rounds up to one second, because
/// telling a client to retry immediately is an invitation to spin.
pub fn too_many_requests(retry_after: Duration) -> Response {
    let secs = (retry_after.as_secs_f64().ceil() as u64).max(1);
    (
        StatusCode::TOO_MANY_REQUESTS,
        [(header::RETRY_AFTER, secs.to_string())],
        Json(serde_json::json!({"error": "rate limited"})),
    )
        .into_response()
}

#[cfg(test)]
#[path = "../unit_tests/hub/ratelimit.rs"]
mod tests;
