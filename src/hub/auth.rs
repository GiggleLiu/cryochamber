//! Bearer-token guard for public-mode cryohub.
//!
//! Route classification is a pure function over (method, path); enforcement
//! happens in one middleware so no handler can be reached unclassified.
//! Out-of-scope chambers 404 (an invite must not learn which ids exist);
//! wrong-role access 403; missing/invalid token 401. On success the resolved
//! `Role` is inserted into request extensions for handlers to consume.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::SystemTime;

use axum::{
    extract::Request,
    http::{Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Router,
};

use crate::hub::state::AppState;
use crate::hub::tokens::{load_tokens, save_tokens, Role, TokenFile};

/// Cheap fingerprint of the tokens file, used to notice writes made by another
/// process. `None` means the file does not exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileStamp {
    mtime: Option<SystemTime>,
    len: u64,
}

fn stamp_of(path: &Path) -> Option<FileStamp> {
    let meta = std::fs::metadata(path).ok()?;
    Some(FileStamp {
        mtime: meta.modified().ok(),
        len: meta.len(),
    })
}

/// The exact bearer token a request presented, inserted into request extensions
/// next to the resolved [`Role`].
///
/// Long-lived surfaces (the SSE stream) must re-authorize against *this*
/// credential rather than against the invite name it resolved to at connect
/// time: names are reusable after revocation, secrets are not.
#[derive(Debug, Clone)]
pub struct BearerToken(pub String);

/// Why a token mutation did not happen.
#[derive(Debug)]
pub enum MutateError {
    /// The store rejected it (duplicate name, no such active invite). Nothing
    /// was written and nothing changed.
    Rejected(anyhow::Error),
    /// It was accepted but could not be persisted. Memory is unchanged, so a
    /// retry sees exactly the state the caller started from.
    Persist(anyhow::Error),
}

/// The live token store plus the file it is mirrored from.
///
/// Two invariants make this the single source of truth for authorization:
///
/// 1. **It follows the file.** `cryohub token create/revoke` runs in a separate
///    process and only rewrites the file, so every resolve re-stats it and
///    reloads when it changed. Without that, CLI revocation would not take
///    effect until the next server restart.
/// 2. **Mutations are transactional.** A change is applied to a copy, persisted,
///    and only then published in memory — under one write lock, so concurrent
///    mutations can neither interleave their saves nor lose each other.
pub struct AuthCtx {
    pub store: RwLock<TokenFile>,
    pub path: PathBuf,
    /// Stamp of `path` as of the last load or save.
    stamp: Mutex<Option<FileStamp>>,
}

impl AuthCtx {
    pub fn load(path: &Path) -> anyhow::Result<Arc<Self>> {
        Ok(Arc::new(Self {
            store: RwLock::new(load_tokens(path)?),
            stamp: Mutex::new(stamp_of(path)),
            path: path.to_path_buf(),
        }))
    }

    /// Pull in any out-of-band edit to the tokens file. Cheap in the common
    /// case: one `stat`, and the write lock is taken only when it changed.
    pub fn sync(&self) {
        if *self.stamp.lock().expect("token stamp poisoned") == stamp_of(&self.path) {
            return;
        }
        let mut store = self.store.write().expect("token store poisoned");
        self.sync_locked(&mut store);
    }

    /// `sync` for a caller that already holds the write lock.
    fn sync_locked(&self, store: &mut TokenFile) {
        let current = stamp_of(&self.path);
        let mut stamp = self.stamp.lock().expect("token stamp poisoned");
        if *stamp == current {
            return;
        }
        // A half-written or unreadable file is no reason to drop the
        // credentials already in hand: keep the last good store and retry on
        // the next request.
        if let Ok(fresh) = load_tokens(&self.path) {
            *store = fresh;
            *stamp = current;
        }
    }

    /// Resolve a bearer token against the *live* store.
    pub fn resolve(&self, bearer: &str) -> Option<Role> {
        self.sync();
        self.store
            .read()
            .expect("token store poisoned")
            .resolve(bearer)
    }

    /// Read the live store.
    pub fn with_store<R>(&self, f: impl FnOnce(&TokenFile) -> R) -> R {
        self.sync();
        f(&self.store.read().expect("token store poisoned"))
    }

    /// Apply `f` as one transaction: mutate a copy of the live store, persist
    /// it, then publish it in memory. The write lock is held throughout, so a
    /// concurrent mutation cannot persist an older snapshot on top of this one.
    pub fn mutate<T>(
        &self,
        f: impl FnOnce(&mut TokenFile) -> anyhow::Result<T>,
    ) -> Result<T, MutateError> {
        let mut store = self.store.write().expect("token store poisoned");
        self.sync_locked(&mut store);
        let mut draft = store.clone();
        let out = f(&mut draft).map_err(MutateError::Rejected)?;
        save_tokens(&self.path, &draft).map_err(MutateError::Persist)?;
        *store = draft;
        *self.stamp.lock().expect("token stamp poisoned") = stamp_of(&self.path);
        Ok(out)
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
        // A guest gets the conversation and its files — nothing else. `status`
        // (log tail, plan, notes, settings, session summaries) and `todos` are
        // the chamber's working state and stay with the owner; the console
        // never asks for them on a guest's behalf.
        ("GET", ["api", "chambers", id, "messages"]) => Access::Chamber((*id).to_string()),
        ("POST", ["api", "chambers", id, "send" | "uploads"]) => Access::Chamber((*id).to_string()),
        ("GET", ["api", "chambers", id, "files", _name]) => Access::Chamber((*id).to_string()),
        // Chamber-local artifacts (articles/, .knowledge/) — the same guest
        // surface as attachments: the conversation shows links to them.
        ("GET", ["api", "chambers", id, "file"]) => Access::Chamber((*id).to_string()),
        _ => Access::OwnerOnly,
    }
}

/// Does this invite scope cover chamber `id`?
///
/// Both sides are reduced to the canonical (encoded) form first, so it does not
/// matter whether the caller passes an encoded index id (the chamber list, the
/// SSE filter, the raw URI segment the guard reads) or a decoded path (what
/// axum's `Path` extractor hands a handler), nor which form the scope was
/// written in. `create_invite` canonicalizes scopes on the way in; stored
/// scopes are canonicalized here too so invites minted before that change keep
/// working.
pub fn scope_covers(chambers: &[String], id: &str) -> bool {
    let wanted = crate::hub::discovery::canonical_scope(&decode_chamber_id(id));
    chambers
        .iter()
        .any(|c| crate::hub::discovery::canonical_scope(c) == wanted)
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
    let Some((bearer, role)) = bearer.and_then(|t| ctx.resolve(&t).map(|r| (t, r))) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let allowed = match (&access, &role) {
        (_, Role::Owner) => true,
        (Access::AnyToken, _) => true,
        (Access::Chamber(id), Role::Invite { chambers, .. }) => {
            if scope_covers(chambers, id) {
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
    // The credential itself, so a handler holding a connection open can
    // re-check *it* rather than the name it resolved to (see `get_events`).
    req.extensions_mut().insert(BearerToken(bearer));
    next.run(req).await
}

#[cfg(test)]
#[path = "../unit_tests/hub/auth.rs"]
mod tests;
