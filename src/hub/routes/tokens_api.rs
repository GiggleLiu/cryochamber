//! Owner-only invite management + whoami. The auth middleware (default-deny)
//! already restricts `/api/tokens*` to the owner; handlers here only do the
//! work. Token strings appear in exactly one response: creation.

use std::sync::Arc;

use axum::{extract::Path as AxumPath, http::StatusCode, response::Json, Extension};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::hub::auth::{AuthCtx, MutateError};
use crate::hub::tokens::Role;

/// Map a failed transaction onto a status code. `rejected` is the handler's own
/// "this request was invalid" code; a persistence failure is always 500 — and,
/// because the mutation never reached memory either, a retry starts from the
/// same state rather than from a half-applied one.
fn mutate_status(err: MutateError, rejected: StatusCode) -> StatusCode {
    match err {
        MutateError::Rejected(_) => rejected,
        MutateError::Persist(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

/// Who am I? Drives the UI's owner-vs-guest chrome. In open (loopback) mode no
/// auth layer runs, so there is no `Role` extension and the local user — who
/// already has shell access to the machine — is the owner.
pub async fn get_whoami(role: Option<Extension<Role>>) -> Json<Value> {
    match role {
        Some(Extension(Role::Invite { name, chambers })) => {
            Json(json!({ "role": "invite", "name": name, "chambers": chambers }))
        }
        _ => Json(json!({ "role": "owner" })),
    }
}

pub async fn get_tokens(ctx: Option<Extension<Arc<AuthCtx>>>) -> Result<Json<Value>, StatusCode> {
    let ctx = ctx.ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    // `with_store` picks up invites the CLI created in another process, so the
    // list is not a stale snapshot from server start.
    let invites = ctx.with_store(|store| {
        // Field-by-field, never `serde_json::to_value(invite)` — the struct
        // carries the secret and a blanket serialization would leak it here.
        store
            .invites
            .iter()
            .map(|i| {
                json!({
                    "name": i.name,
                    "chambers": i.chambers,
                    "created_at": i.created_at,
                    "revoked_at": i.revoked_at,
                })
            })
            .collect::<Vec<Value>>()
    });
    Ok(Json(json!({ "invites": invites })))
}

#[derive(Deserialize)]
pub struct CreateTokenRequest {
    name: String,
    #[serde(default)]
    chambers: Vec<String>,
}

pub async fn post_token(
    ctx: Option<Extension<Arc<AuthCtx>>>,
    Json(req): Json<CreateTokenRequest>,
) -> Result<Json<Value>, StatusCode> {
    let ctx = ctx.ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let invite = ctx
        .mutate(|store| store.create_invite(&req.name, req.chambers))
        .map_err(|e| mutate_status(e, StatusCode::BAD_REQUEST))?;
    Ok(Json(json!({
        "ok": true,
        "name": invite.name,
        "token": invite.token,
        "link_fragment": format!("#invite={}", invite.token),
    })))
}

pub async fn post_revoke(
    ctx: Option<Extension<Arc<AuthCtx>>>,
    AxumPath(name): AxumPath<String>,
) -> Result<Json<Value>, StatusCode> {
    let ctx = ctx.ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    ctx.mutate(|store| {
        anyhow::ensure!(store.revoke(&name), "no active invite named '{name}'");
        Ok(())
    })
    .map_err(|e| mutate_status(e, StatusCode::NOT_FOUND))?;
    Ok(Json(json!({ "ok": true })))
}

#[cfg(test)]
#[path = "../../unit_tests/hub/routes/tokens_api.rs"]
mod tests;
