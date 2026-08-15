//! Bearer-token guard for public-mode cryohub.
//!
//! Route classification is a pure function over (method, path); enforcement
//! happens in one middleware so no handler can be reached unclassified.
//! Out-of-scope chambers 404 (an invite must not learn which ids exist);
//! wrong-role access 403; missing/invalid token 401. On success the resolved
//! `Role` is inserted into request extensions for handlers to consume.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use axum::{
    extract::Request,
    http::{Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Router,
};

use crate::hub::state::AppState;
use crate::hub::tokens::{load_tokens, save_tokens, Role, TokenFile};

pub struct AuthCtx {
    pub store: RwLock<TokenFile>,
    pub path: PathBuf,
}

impl AuthCtx {
    pub fn load(path: &Path) -> anyhow::Result<Arc<Self>> {
        Ok(Arc::new(Self {
            store: RwLock::new(load_tokens(path)?),
            path: path.to_path_buf(),
        }))
    }

    pub fn persist(&self) -> anyhow::Result<()> {
        let tokens = self.store.read().expect("token store poisoned").clone();
        save_tokens(&self.path, &tokens)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Access {
    Public,
    AnyToken,
    Chamber(String),
    OwnerOnly,
}

/// Classify a request. Chamber-scoped routes are the exact chat surface an
/// invite may use; every other `/api` route is owner-only BY DEFAULT so a
/// future route added without thinking about auth fails closed.
pub fn classify(method: &Method, path: &str) -> Access {
    // Segment-exact prefix: `/api` and `/api/...` are guarded, but `/apiary`
    // and `/api-v2` are ordinary public paths (a substring match would guard
    // — and 401 — every page whose name merely begins with "api").
    if path != "/api" && !path.starts_with("/api/") {
        return Access::Public;
    }
    let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    match (method.as_str(), segments.as_slice()) {
        ("GET", ["api", "chambers"]) => Access::AnyToken,
        ("GET", ["api", "events"]) => Access::AnyToken,
        ("GET", ["api", "whoami"]) => Access::AnyToken,
        ("GET", ["api", "chambers", id, "messages" | "status" | "todos"]) => {
            Access::Chamber((*id).to_string())
        }
        ("POST", ["api", "chambers", id, "send" | "uploads"]) => Access::Chamber((*id).to_string()),
        ("GET", ["api", "chambers", id, "files", _name]) => Access::Chamber((*id).to_string()),
        _ => Access::OwnerOnly,
    }
}

/// Does this invite scope cover chamber path-param `id`? Ids may arrive
/// percent-decoded (axum decodes path params), so try the re-encoded form
/// too — mirrors `AppState::resolve`.
///
/// `id` must already be in decoded form; callers reading it straight off the
/// raw URI go through [`decode_chamber_id`] first.
pub fn scope_covers(chambers: &[String], id: &str) -> bool {
    if chambers.iter().any(|c| c == id) {
        return true;
    }
    let re_encoded = crate::hub::discovery::encode_id(std::path::Path::new(id));
    chambers.iter().any(|c| *c == re_encoded)
}

/// Percent-decode a chamber id captured from the raw request URI, exactly
/// once, so it reaches `scope_covers` in the same form axum's `Path`
/// extractor hands to handlers (and `AppState::resolve`). Re-encoding an
/// already-encoded id would double-encode it (`%2F` → `%252F`) and never
/// match a stored scope. Ids with invalid percent escapes are left as-is.
pub fn decode_chamber_id(id: &str) -> String {
    urlencoding::decode(id)
        .map(|s| s.into_owned())
        .unwrap_or_else(|_| id.to_string())
}

pub fn apply_auth(router: Router, _app: Arc<AppState>, ctx: Arc<AuthCtx>) -> Router {
    router.layer(axum::middleware::from_fn(
        move |req: Request, next: Next| {
            let ctx = ctx.clone();
            async move { guard(&ctx, req, next).await }
        },
    ))
}

async fn guard(ctx: &AuthCtx, mut req: Request, next: Next) -> Response {
    let access = classify(req.method(), req.uri().path());
    if access == Access::Public {
        return next.run(req).await;
    }
    let bearer = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::to_string);
    let role = bearer.and_then(|t| ctx.store.read().expect("token store poisoned").resolve(&t));
    let Some(role) = role else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let allowed = match (&access, &role) {
        (_, Role::Owner) => true,
        (Access::AnyToken, _) => true,
        (Access::Chamber(id), Role::Invite { chambers, .. }) => {
            if scope_covers(chambers, &decode_chamber_id(id)) {
                true
            } else {
                // 404, not 403: invites must not be able to probe chamber ids.
                return StatusCode::NOT_FOUND.into_response();
            }
        }
        (Access::OwnerOnly, Role::Invite { .. }) => {
            return StatusCode::FORBIDDEN.into_response();
        }
        (Access::Public, _) => unreachable!("handled above"),
    };
    debug_assert!(allowed);
    req.extensions_mut().insert(role);
    next.run(req).await
}

#[cfg(test)]
#[path = "../unit_tests/hub/auth.rs"]
mod tests;
