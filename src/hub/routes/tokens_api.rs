//! Owner-only invite management + whoami. The auth middleware (default-deny)
//! already restricts `/api/tokens*` to the owner; handlers here only do the
//! work. Token strings appear in exactly one response: creation.

use std::sync::Arc;

use axum::{extract::Path as AxumPath, http::StatusCode, response::Json, Extension};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::hub::auth::AuthCtx;
use crate::hub::tokens::Role;

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
    let store = ctx.store.read().expect("token store poisoned");
    // Field-by-field, never `serde_json::to_value(invite)` — the struct carries
    // the secret and a blanket serialization would leak it into the list.
    let invites: Vec<Value> = store
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
        .collect();
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
    let invite = {
        let mut store = ctx.store.write().expect("token store poisoned");
        store
            .create_invite(&req.name, req.chambers)
            .map_err(|_| StatusCode::BAD_REQUEST)?
    };
    ctx.persist()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
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
    let revoked = {
        let mut store = ctx.store.write().expect("token store poisoned");
        store.revoke(&name)
    };
    if !revoked {
        return Err(StatusCode::NOT_FOUND);
    }
    ctx.persist()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({ "ok": true })))
}

#[cfg(test)]
#[path = "../../unit_tests/hub/routes/tokens_api.rs"]
mod tests;
