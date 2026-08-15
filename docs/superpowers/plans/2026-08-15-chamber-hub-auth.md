# Chamber Hub Auth (Plan A: cryochamber, Rust) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add owner/invite bearer-token auth, per-chamber scoping (including SSE filtering), attachments over HTTP, token management API/CLI, and a `--public` mode to cryohub.

**Architecture:** A token store (`src/hub/tokens.rs`) resolves `Authorization: Bearer` headers to a `Role` (Owner or Invite with a chamber scope). An auth middleware (`src/hub/auth.rs`) classifies each route (public / any-token / chamber-scoped / owner-only), enforces it, and injects the `Role` into request extensions; handlers use the role to filter the chamber list, stamp message senders, and filter the SSE stream. Attachments get two new scoped routes. Auth is only applied in `--public` mode; the current loopback no-auth behavior is untouched otherwise.

**Tech Stack:** Rust, axum 0.8 (add the `multipart` feature), tokio, serde, tempfile+tower for tests. No new crypto deps: tokens come from `/dev/urandom`.

**Working repo:** `~/rcode/cryochamber` (NOT this repo — this plan file travels with the spec).

**Spec:** `docs/superpowers/specs/2026-08-15-chamber-hub-design.md` (in the zulip-app repo: `~/agentic/zulip-app`).

## Global Constraints

- Token file: `~/.config/cryo/cryohub-tokens.json`, file mode `0600`.
- Invite tokens: ≥32 bytes CSPRNG, hex-encoded (64 chars). Revocation is a tombstone (`revoked_at`), never deletion.
- Out-of-scope chamber access returns **404, not 403** (no id enumeration).
- Owner-only is the **default** for unclassified `/api` routes (default-deny).
- Upload cap: 25 MB (matches chat-bridge `MAX_ATTACHMENT_BYTES`).
- `--public` refuses to start without an owner token.
- Follow repo conventions: unit tests via `#[cfg(test)] #[path = "../unit_tests/..."]`, integration tests in `tests/`, tower `oneshot` for router tests.
- Run tests with `cargo test`; run a focused test with `cargo test <name>`.

---

### Task 1: Token store (`src/hub/tokens.rs`)

**Files:**
- Create: `src/hub/tokens.rs`
- Create: `src/unit_tests/hub/tokens.rs`
- Modify: `src/hub/mod.rs` (add `pub mod tokens;`)
- Modify: `src/unit_tests/hub/mod.rs` (register the test module if the directory uses a mod file; mirror how `state.rs` is wired)

**Interfaces:**
- Produces: `TokenFile { owner: Option<String>, invites: Vec<Invite> }`, `Invite { token, name, chambers, created_at, revoked_at }`, `Role::Owner | Role::Invite { name: String, chambers: Vec<String> }`, `TokenFile::resolve(&self, bearer: &str) -> Option<Role>`, `TokenFile::create_invite(&mut self, name: &str, chambers: Vec<String>) -> anyhow::Result<Invite>`, `TokenFile::revoke(&mut self, name: &str) -> bool`, `TokenFile::ensure_owner(&mut self) -> anyhow::Result<String>`, `load_tokens(path) -> anyhow::Result<TokenFile>`, `save_tokens(path, &TokenFile) -> anyhow::Result<()>`, `default_tokens_path() -> PathBuf`, `generate_token() -> anyhow::Result<String>`

- [ ] **Step 1: Write the failing tests**

```rust
// src/unit_tests/hub/tokens.rs
use crate::hub::tokens::*;

#[test]
fn generate_token_is_64_hex_and_unique() {
    let a = generate_token().unwrap();
    let b = generate_token().unwrap();
    assert_eq!(a.len(), 64);
    assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    assert_ne!(a, b);
}

#[test]
fn resolve_owner_invite_revoked_and_unknown() {
    let mut tf = TokenFile::default();
    let owner = tf.ensure_owner().unwrap();
    let inv = tf.create_invite("Alice", vec!["autoresearch".into()]).unwrap();
    assert_eq!(tf.resolve(&owner), Some(Role::Owner));
    assert_eq!(
        tf.resolve(&inv.token),
        Some(Role::Invite { name: "Alice".into(), chambers: vec!["autoresearch".into()] })
    );
    assert_eq!(tf.resolve("deadbeef"), None);
    assert!(tf.revoke("Alice"));
    assert_eq!(tf.resolve(&inv.token), None, "revoked token must not resolve");
    // tombstone, not deletion
    assert!(tf.invites[0].revoked_at.is_some());
    assert!(!tf.revoke("Alice"), "second revoke is a no-op");
}

#[test]
fn ensure_owner_is_idempotent() {
    let mut tf = TokenFile::default();
    let a = tf.ensure_owner().unwrap();
    let b = tf.ensure_owner().unwrap();
    assert_eq!(a, b);
}

#[test]
fn duplicate_invite_name_is_rejected() {
    let mut tf = TokenFile::default();
    tf.create_invite("Alice", vec![]).unwrap();
    assert!(tf.create_invite("Alice", vec![]).is_err());
}

#[test]
fn save_load_roundtrip_with_0600() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("tokens.json");
    let mut tf = TokenFile::default();
    tf.ensure_owner().unwrap();
    tf.create_invite("Bob", vec!["x".into()]).unwrap();
    save_tokens(&path, &tf).unwrap();
    let loaded = load_tokens(&path).unwrap();
    assert_eq!(loaded.owner, tf.owner);
    assert_eq!(loaded.invites.len(), 1);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}

#[test]
fn load_missing_file_yields_default() {
    let tmp = tempfile::tempdir().unwrap();
    let tf = load_tokens(&tmp.path().join("nope.json")).unwrap();
    assert!(tf.owner.is_none());
    assert!(tf.invites.is_empty());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test hub::tokens`
Expected: compile error — module `tokens` not found.

- [ ] **Step 3: Implement `src/hub/tokens.rs`**

```rust
//! Bearer-token store for public-mode cryohub: one owner token, N named
//! invite tokens scoped to chamber ids. Backing file is JSON at
//! `~/.config/cryo/cryohub-tokens.json`, mode 0600. Revocation tombstones
//! (`revoked_at`) rather than deletes, so the audit trail survives.

use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenFile {
    pub owner: Option<String>,
    #[serde(default)]
    pub invites: Vec<Invite>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Invite {
    pub token: String,
    pub name: String,
    pub chambers: Vec<String>,
    pub created_at: String,
    #[serde(default)]
    pub revoked_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Role {
    Owner,
    Invite { name: String, chambers: Vec<String> },
}

/// 32 bytes from the OS CSPRNG, hex-encoded. No extra dependency: the hub
/// only runs on unix hosts (it already manages systemd/launchd services).
pub fn generate_token() -> Result<String> {
    let mut buf = [0u8; 32];
    std::fs::File::open("/dev/urandom")
        .context("open /dev/urandom")?
        .read_exact(&mut buf)
        .context("read /dev/urandom")?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

/// Constant-time byte comparison — cheap insurance against timing probes.
fn ct_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

impl TokenFile {
    pub fn resolve(&self, bearer: &str) -> Option<Role> {
        if let Some(owner) = &self.owner {
            if ct_eq(owner, bearer) {
                return Some(Role::Owner);
            }
        }
        self.invites
            .iter()
            .filter(|i| i.revoked_at.is_none())
            .find(|i| ct_eq(&i.token, bearer))
            .map(|i| Role::Invite { name: i.name.clone(), chambers: i.chambers.clone() })
    }

    /// Create the owner token if absent; return it either way.
    pub fn ensure_owner(&mut self) -> Result<String> {
        if let Some(owner) = &self.owner {
            return Ok(owner.clone());
        }
        let token = generate_token()?;
        self.owner = Some(token.clone());
        Ok(token)
    }

    pub fn create_invite(&mut self, name: &str, chambers: Vec<String>) -> Result<Invite> {
        if name.trim().is_empty() {
            bail!("invite name is empty");
        }
        if self.invites.iter().any(|i| i.name == name && i.revoked_at.is_none()) {
            bail!("an active invite named '{name}' already exists");
        }
        let invite = Invite {
            token: generate_token()?,
            name: name.to_string(),
            chambers,
            created_at: chrono::Utc::now().to_rfc3339(),
            revoked_at: None,
        };
        self.invites.push(invite.clone());
        Ok(invite)
    }

    /// Tombstone the active invite with this name. Returns false if none.
    pub fn revoke(&mut self, name: &str) -> bool {
        for i in &mut self.invites {
            if i.name == name && i.revoked_at.is_none() {
                i.revoked_at = Some(chrono::Utc::now().to_rfc3339());
                return true;
            }
        }
        false
    }
}

pub fn default_tokens_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("cryo")
        .join("cryohub-tokens.json")
}

pub fn load_tokens(path: &Path) -> Result<TokenFile> {
    if !path.exists() {
        return Ok(TokenFile::default());
    }
    let raw = std::fs::read_to_string(path).with_context(|| format!("read {path:?}"))?;
    serde_json::from_str(&raw).with_context(|| format!("parse {path:?}"))
}

pub fn save_tokens(path: &Path, tokens: &TokenFile) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let raw = serde_json::to_string_pretty(tokens)?;
    std::fs::write(path, raw)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "../unit_tests/hub/tokens.rs"]
mod tests;
```

Add `pub mod tokens;` to `src/hub/mod.rs`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test hub::tokens`
Expected: all 6 PASS.

- [ ] **Step 5: Commit**

```bash
git add src/hub/tokens.rs src/unit_tests/hub/tokens.rs src/hub/mod.rs
git commit -m "feat(hub): bearer token store (owner + scoped invites, tombstone revoke)"
```

---

### Task 2: Auth middleware and route classification (`src/hub/auth.rs`)

**Files:**
- Create: `src/hub/auth.rs`
- Create: `src/unit_tests/hub/auth.rs`
- Modify: `src/hub/mod.rs` (add `pub mod auth;` and a `build_router_public` entry point)

**Interfaces:**
- Consumes: `tokens::{TokenFile, Role, load_tokens, save_tokens}` (Task 1), `AppState::resolve`, `discovery::encode_id`.
- Produces:
  - `pub struct AuthCtx { pub store: std::sync::RwLock<TokenFile>, pub path: PathBuf }` with `AuthCtx::load(path) -> anyhow::Result<Arc<AuthCtx>>` and `AuthCtx::persist(&self) -> anyhow::Result<()>` (writes the current store back to `path`).
  - `pub fn apply_auth(router: Router, app: Arc<AppState>, ctx: Arc<AuthCtx>) -> Router` — layers the guard; inserts `Role` into request extensions on success.
  - `pub enum Access { Public, AnyToken, Chamber(String), OwnerOnly }` and `pub fn classify(method: &Method, path: &str) -> Access` (pure function; unit-testable).
  - Handlers later read `Option<axum::Extension<Role>>`: `None` means auth is not applied (open/loopback mode) and is treated as full access.
- Scope matching: an invite is in scope for chamber path-param `id` when its `chambers` list contains `id` **or** `discovery::encode_id(Path::new(id))` (mirrors `AppState::resolve`'s dual-form lookup).

Route classification table (must match the spec's authorization matrix):

| Access | Routes |
|---|---|
| `Public` | anything not under `/api` (pages `/`, `/c/{id}`, `/assets/*`) |
| `AnyToken` | `GET /api/chambers`, `GET /api/events`, `GET /api/whoami` |
| `Chamber(id)` | `GET /api/chambers/{id}/messages`, `GET .../status`, `GET .../todos`, `POST .../send`, `POST .../uploads`, `GET .../files/{name}` |
| `OwnerOnly` | **everything else under `/api`** (refresh, new, lifecycle, sync, tokens) — default-deny |

- [ ] **Step 1: Write the failing tests**

```rust
// src/unit_tests/hub/auth.rs
use axum::http::Method;
use crate::hub::auth::{classify, Access};

#[test]
fn classification_matches_spec_matrix() {
    use Access::*;
    let cases = [
        (Method::GET, "/", Public),
        (Method::GET, "/c/abc", Public),
        (Method::GET, "/assets/web.css", Public),
        (Method::GET, "/api/chambers", AnyToken),
        (Method::GET, "/api/events", AnyToken),
        (Method::GET, "/api/whoami", AnyToken),
        (Method::GET, "/api/chambers/x1/messages", Chamber("x1".into())),
        (Method::GET, "/api/chambers/x1/status", Chamber("x1".into())),
        (Method::GET, "/api/chambers/x1/todos", Chamber("x1".into())),
        (Method::POST, "/api/chambers/x1/send", Chamber("x1".into())),
        (Method::POST, "/api/chambers/x1/uploads", Chamber("x1".into())),
        (Method::GET, "/api/chambers/x1/files/a.pdf", Chamber("x1".into())),
        // owner-only by default
        (Method::POST, "/api/chambers/refresh", OwnerOnly),
        (Method::POST, "/api/chambers/new", OwnerOnly),
        (Method::POST, "/api/chambers/x1/start", OwnerOnly),
        (Method::POST, "/api/chambers/x1/stop", OwnerOnly),
        (Method::POST, "/api/chambers/x1/reset", OwnerOnly),
        (Method::GET, "/api/chambers/x1/sync", OwnerOnly),
        (Method::POST, "/api/tokens", OwnerOnly),
        (Method::GET, "/api/anything-new", OwnerOnly),
    ];
    for (method, path, want) in cases {
        assert_eq!(classify(&method, path), want, "{method} {path}");
    }
}
```

And an end-to-end guard test using the repo's tower-oneshot pattern (append to the same file):

```rust
use std::sync::Arc;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;
use crate::hub::auth::{apply_auth, AuthCtx};
use crate::hub::state::AppState;
use crate::hub::tokens::{save_tokens, TokenFile};

fn public_router(tmp: &tempfile::TempDir) -> (axum::Router, String, String) {
    let mut tf = TokenFile::default();
    let owner = tf.ensure_owner().unwrap();
    let invite = tf.create_invite("Alice", vec!["scoped-id".into()]).unwrap();
    let path = tmp.path().join("tokens.json");
    save_tokens(&path, &tf).unwrap();
    let ctx = AuthCtx::load(&path).unwrap();
    let app = Arc::new(AppState::local_only(tmp.path().to_path_buf()));
    let router = crate::hub::build_router_with_state(app.clone());
    (apply_auth(router, app, ctx), owner, invite.token)
}

async fn status_for(router: &axum::Router, method: &str, uri: &str, token: Option<&str>) -> StatusCode {
    let mut req = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        req = req.header("authorization", format!("Bearer {t}"));
    }
    let req = req.header("host", "127.0.0.1").body(Body::empty()).unwrap();
    router.clone().oneshot(req).await.unwrap().status()
}

#[tokio::test]
async fn guard_enforces_401_403_404() {
    let tmp = tempfile::tempdir().unwrap();
    let (router, owner, invite) = public_router(&tmp);
    // no token → 401 on any /api route
    assert_eq!(status_for(&router, "GET", "/api/chambers", None).await, StatusCode::UNAUTHORIZED);
    // bad token → 401
    assert_eq!(status_for(&router, "GET", "/api/chambers", Some("ffff")).await, StatusCode::UNAUTHORIZED);
    // invite on owner-only → 403
    assert_eq!(
        status_for(&router, "POST", "/api/chambers/refresh", Some(&invite)).await,
        StatusCode::FORBIDDEN
    );
    // invite on out-of-scope chamber → 404 (never 403)
    assert_eq!(
        status_for(&router, "GET", "/api/chambers/other-id/messages", Some(&invite)).await,
        StatusCode::NOT_FOUND
    );
    // owner passes the guard (404 here only because the chamber doesn't exist)
    assert_eq!(
        status_for(&router, "GET", "/api/chambers/other-id/messages", Some(&owner)).await,
        StatusCode::NOT_FOUND
    );
    // static pages stay public
    assert_eq!(status_for(&router, "GET", "/", None).await, StatusCode::OK);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test hub::auth`
Expected: compile error — module `auth` not found.

- [ ] **Step 3: Implement `src/hub/auth.rs`**

```rust
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
    if !path.starts_with("/api") {
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
pub fn scope_covers(chambers: &[String], id: &str) -> bool {
    if chambers.iter().any(|c| c == id) {
        return true;
    }
    let re_encoded = crate::hub::discovery::encode_id(std::path::Path::new(id));
    chambers.iter().any(|c| *c == re_encoded)
}

pub fn apply_auth(router: Router, _app: Arc<AppState>, ctx: Arc<AuthCtx>) -> Router {
    router.layer(axum::middleware::from_fn(move |req: Request, next: Next| {
        let ctx = ctx.clone();
        async move { guard(&ctx, req, next).await }
    }))
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
    next.run(req).await
}

#[cfg(test)]
#[path = "../unit_tests/hub/auth.rs"]
mod tests;
```

In `src/hub/mod.rs` add `pub mod auth;` and:

```rust
/// Public-mode router: same routes, wrapped in the bearer-token guard.
pub fn build_router_public(app: Arc<WebAppState>, ctx: Arc<crate::hub::auth::AuthCtx>) -> Router {
    let router = build_router_with_state(app.clone());
    crate::hub::auth::apply_auth(router, app, ctx)
}
```

Note: `apply_auth` wraps the router *after* `security::apply` (which is inside
`build_router_with_state`), so axum runs the auth layer first, then
host/CSRF. Both guards stay active in public mode. `POST` requests through
the API now need the `X-Cryo-CSRF` header the existing middleware requires —
that is intentional and the PWA client (Plan B) always sends it.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test hub::auth`
Expected: both tests PASS.

- [ ] **Step 5: Commit**

```bash
git add src/hub/auth.rs src/unit_tests/hub/auth.rs src/hub/mod.rs
git commit -m "feat(hub): bearer auth middleware with default-deny route classification"
```

---

### Task 3: Role-aware handlers — chamber list filtering and sender stamping

**Files:**
- Modify: `src/hub/routes/chambers.rs` (`get_chambers`)
- Modify: `src/hub/routes/chamber.rs` (`post_send`)
- Modify: `src/unit_tests/hub/routes/chambers.rs`, `src/unit_tests/hub/routes/chamber.rs` (add tests)

**Interfaces:**
- Consumes: `Role` extension (Task 2), `scope_covers` (Task 2).
- Produces: `get_chambers` returns only in-scope chambers for invites; `post_send` ignores the client `from` for invites and stamps the invite name; owner/no-role keeps today's behavior (`from` defaults to `"human"`).

- [ ] **Step 1: Write the failing tests**

In `src/unit_tests/hub/routes/chambers.rs` (follow the existing test setup in that file for building an `AppState` with chambers `alpha`/`beta`):

```rust
#[tokio::test]
async fn chamber_list_is_filtered_for_invites() {
    // build AppState with chambers alpha+beta exactly as existing tests do,
    // resolve beta's index id, then call get_chambers with an invite Role
    // scoped to beta only:
    // let role = Role::Invite { name: "Alice".into(), chambers: vec![beta_id.clone()] };
    // let resp = get_chambers(State(app.clone()), Some(Extension(role))).await;
    // assert only beta remains in the returned array; owner Role / None returns both.
}
```

Write it concretely against the file's existing helpers — the assertion core:

```rust
let ids: Vec<String> = resp.0.as_array().unwrap().iter()
    .map(|c| c["id"].as_str().unwrap().to_string()).collect();
assert_eq!(ids, vec![beta_id]);
```

In `src/unit_tests/hub/routes/chamber.rs`:

```rust
#[tokio::test]
async fn send_stamps_invite_name_ignoring_client_from() {
    // chamber dir via tempfile + MessageStore, as existing send tests do.
    // Call post_send with Role::Invite{name:"Alice",..} and body
    // {"body":"hi","from":"owner-imposter"}.
    // Then read the newest inbox file and assert its frontmatter `from` == "Alice".
}

#[tokio::test]
async fn send_without_role_keeps_default_human() {
    // post_send with no Role extension and no `from` → inbox file from == "human".
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test routes::chambers routes::chamber`
Expected: compile error — handlers don't take the `Extension` parameter yet.

- [ ] **Step 3: Modify the handlers**

`get_chambers` — add the optional role parameter and filter the snapshot:

```rust
use axum::Extension;
use crate::hub::tokens::Role;

pub async fn get_chambers(
    State(app): State<Arc<AppState>>,
    role: Option<Extension<Role>>,
) -> Json<Value> {
    // ... existing spawn_blocking body unchanged, producing `value` ...
    let value = match role {
        Some(Extension(Role::Invite { chambers, .. })) => match value {
            Value::Array(items) => Value::Array(
                items
                    .into_iter()
                    .filter(|c| {
                        c["id"]
                            .as_str()
                            .map(|id| crate::hub::auth::scope_covers(&chambers, id))
                            .unwrap_or(false)
                    })
                    .collect(),
            ),
            other => other,
        },
        _ => value,
    };
    Json(value)
}
```

`post_send` — stamp the sender from the role:

```rust
pub async fn post_send(
    State(app): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
    role: Option<Extension<Role>>,
    Json(req): Json<SendRequest>,
) -> Result<Json<Value>, StatusCode> {
    let (path, entry) = app.resolve(&id).ok_or(StatusCode::NOT_FOUND)?;
    let from = match role {
        Some(Extension(Role::Invite { name, .. })) => name,
        _ => req.from.unwrap_or_else(|| "human".into()),
    };
    let store = MessageStore::new(path.clone());
    let msg = crate::message::Message {
        from,
        // ... rest of the existing body unchanged (subject, body, timestamp,
        // metadata, is_question, send_in, SseEvent broadcast) ...
```

(Note the argument order: axum requires the `Json` body extractor last.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test routes::chambers routes::chamber`
Expected: new tests PASS; all pre-existing tests still PASS (they call the handlers with `None` role — update their call sites to pass `None` where the signature changed).

- [ ] **Step 5: Commit**

```bash
git add src/hub/routes/chambers.rs src/hub/routes/chamber.rs src/unit_tests/hub/routes/
git commit -m "feat(hub): scope chamber list and stamp sender identity by role"
```

---

### Task 4: SSE filtering by scope

**Files:**
- Modify: `src/hub/routes/events.rs`
- Modify: `src/unit_tests/hub/routes/events.rs`

**Interfaces:**
- Consumes: `Role` extension, `scope_covers`.
- Produces: `get_events` yields only events whose `chamber_id` is in the invite's scope; `IndexChanged` (no chamber_id) passes to everyone; owner/no-role sees everything.

- [ ] **Step 1: Write the failing test**

Follow the existing tests in `src/unit_tests/hub/routes/events.rs` for how a stream is consumed; the new test:

```rust
#[tokio::test]
async fn invite_stream_only_carries_scoped_chambers() {
    let app = /* AppState as in existing events tests */;
    let role = Role::Invite { name: "Alice".into(), chambers: vec!["mine".into()] };
    let sse = get_events(State(app.clone()), Some(Extension(role))).await;
    app.tx.send(SseEvent::NewMessage {
        chamber_id: "mine".into(), direction: "inbox".into(), from: "x".into(),
        subject: "".into(), body: "visible".into(), timestamp: "t".into(), is_question: false,
    }).unwrap();
    app.tx.send(SseEvent::NewMessage {
        chamber_id: "other".into(), direction: "inbox".into(), from: "x".into(),
        subject: "".into(), body: "SECRET".into(), timestamp: "t".into(), is_question: false,
    }).unwrap();
    app.tx.send(SseEvent::IndexChanged).unwrap();
    // Drain 2 events from the stream (with a timeout): assert the first body
    // contains "visible", the second is the `index` event, and "SECRET"
    // never appears.
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test routes::events`
Expected: compile error — `get_events` has no role parameter.

- [ ] **Step 3: Implement the filter**

```rust
pub async fn get_events(
    State(app): State<Arc<AppState>>,
    role: Option<axum::Extension<crate::hub::tokens::Role>>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let scope: Option<Vec<String>> = match role {
        Some(axum::Extension(crate::hub::tokens::Role::Invite { chambers, .. })) => Some(chambers),
        _ => None, // owner or open mode: unfiltered
    };
    let rx = app.tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(move |result: Result<SseEvent, _>| {
        let event = result.ok()?;
        if let Some(scope) = &scope {
            let chamber_id = match &event {
                SseEvent::NewMessage { chamber_id, .. }
                | SseEvent::StatusChange { chamber_id }
                | SseEvent::LogLine { chamber_id, .. } => Some(chamber_id.as_str()),
                SseEvent::IndexChanged => None, // index-level, carries no content
            };
            if let Some(id) = chamber_id {
                if !crate::hub::auth::scope_covers(scope, id) {
                    return None;
                }
            }
        }
        // ... existing match building the Event stays unchanged ...
        Some(Ok(ev))
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test routes::events`
Expected: PASS (existing tests updated to pass `None`).

- [ ] **Step 5: Commit**

```bash
git add src/hub/routes/events.rs src/unit_tests/hub/routes/events.rs
git commit -m "feat(hub): filter SSE stream to invite scope"
```

---

### Task 5: Attachments — upload and download routes

**Files:**
- Modify: `Cargo.toml` (axum `multipart` feature: `axum = { version = "0.8", features = ["multipart"] }`)
- Create: `src/hub/routes/files.rs`
- Create: `src/unit_tests/hub/routes/files.rs`
- Modify: `src/hub/routes/mod.rs` (add `pub mod files;`), `src/hub/mod.rs` (two new routes)

**Interfaces:**
- Consumes: `AppState::resolve`.
- Produces:
  - `POST /api/chambers/{id}/uploads` (multipart field `file`) → `{ "ok": true, "name": "<stored-name>", "markdown": "[orig.pdf](/api/chambers/{id}/files/<stored-name>)" }`. Stored under `<chamber>/messages/attachments/<sha256[..12]>_<sanitized-name>`. 25 MB cap → 413. No file field → 400.
  - `GET /api/chambers/{id}/files/{name}` → file bytes, `Content-Disposition: attachment`, mime by extension. `{name}` must be a single sanitized segment: any `/`, `\`, or leading `.` → 404 before touching the filesystem.
  - `pub const MAX_ATTACHMENT_BYTES: usize = 25 * 1024 * 1024;`
  - `fn safe_name(name: &str) -> String` — keep `[A-Za-z0-9._-]`, replace others with `_`, strip leading dots, fallback `"attachment"`.

- [ ] **Step 1: Write the failing tests**

```rust
// src/unit_tests/hub/routes/files.rs — router-level via tower oneshot,
// AppState::local_only over a tempdir with one chamber (copy the setup used
// in unit_tests/hub/routes/chamber.rs).

#[tokio::test]
async fn upload_then_download_roundtrip() {
    // multipart body with field "file", filename "report.pdf", bytes b"%PDF-fake".
    // POST /api/chambers/{id}/uploads (with X-Cryo-CSRF: 1 and host header)
    // → 200, json.markdown starts with "[report.pdf](/api/chambers/" and
    //   contains "/files/".
    // GET the returned files URL → 200, body == b"%PDF-fake",
    //   content-disposition contains "attachment".
}

#[tokio::test]
async fn traversal_names_404_without_fs_access() {
    // GET /api/chambers/{id}/files/..%2Fcryo.toml  → 404
    // GET /api/chambers/{id}/files/.hidden         → 404
}

#[tokio::test]
async fn oversized_upload_is_413() {
    // multipart with MAX_ATTACHMENT_BYTES + 1 bytes → 413.
}

#[tokio::test]
async fn missing_file_field_is_400() {
    // multipart with only a text field → 400.
}
```

Write these as real code: build multipart bodies by hand (the repo has no
multipart client helper) —

```rust
fn multipart_body(boundary: &str, filename: &str, bytes: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(format!(
        "--{boundary}\r\ncontent-disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\ncontent-type: application/octet-stream\r\n\r\n"
    ).as_bytes());
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test routes::files`
Expected: compile error — module `files` not found.

- [ ] **Step 3: Implement `src/hub/routes/files.rs`**

```rust
//! Chamber attachments over HTTP: uploads land in
//! `<chamber>/messages/attachments/` (where chat-bridge also materializes
//! platform attachments) and are served back with a containment check that
//! never lets a request name escape that directory.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::{
    extract::{Multipart, Path as AxumPath, State},
    http::{header, StatusCode},
    response::{IntoResponse, Json, Response},
};
use serde_json::json;

use crate::hub::state::AppState;

pub const MAX_ATTACHMENT_BYTES: usize = 25 * 1024 * 1024;

pub fn safe_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') { c } else { '_' })
        .collect();
    let cleaned = cleaned.trim_start_matches('.').to_string();
    if cleaned.is_empty() { "attachment".into() } else { cleaned }
}

fn attachments_dir(chamber: &Path) -> PathBuf {
    chamber.join("messages").join("attachments")
}

fn sha12(bytes: &[u8]) -> String {
    // No sha2 dependency: FNV-1a folded twice is enough for a collision-
    // avoiding storage prefix (not a security boundary — names are served
    // only from the attachments dir).
    let mut h1: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h1 ^= *b as u64;
        h1 = h1.wrapping_mul(0x100000001b3);
    }
    let mut h2: u64 = 0xcbf29ce484222325;
    for b in bytes.iter().rev() {
        h2 ^= *b as u64;
        h2 = h2.wrapping_mul(0x100000001b3);
    }
    format!("{h1:08x}{h2:08x}")[..12].to_string()
}

pub async fn post_upload(
    State(app): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let (chamber, entry) = app.resolve(&id).ok_or(StatusCode::NOT_FOUND)?;
    while let Some(field) = multipart.next_field().await.map_err(|_| StatusCode::BAD_REQUEST)? {
        if field.name() != Some("file") {
            continue;
        }
        let original = field.file_name().unwrap_or("attachment").to_string();
        let bytes = field.bytes().await.map_err(|_| StatusCode::PAYLOAD_TOO_LARGE)?;
        if bytes.len() > MAX_ATTACHMENT_BYTES {
            return Err(StatusCode::PAYLOAD_TOO_LARGE);
        }
        let stored = format!("{}_{}", sha12(&bytes), safe_name(&original));
        let dir = attachments_dir(&chamber);
        std::fs::create_dir_all(&dir).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        std::fs::write(dir.join(&stored), &bytes).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let url = format!("/api/chambers/{}/files/{}", entry.id, stored);
        return Ok(Json(json!({
            "ok": true,
            "name": stored,
            "markdown": format!("[{original}]({url})"),
        })));
    }
    Err(StatusCode::BAD_REQUEST)
}

fn mime_for(name: &str) -> &'static str {
    match name.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "txt" | "md" | "log" => "text/plain; charset=utf-8",
        "json" => "application/json",
        _ => "application/octet-stream",
    }
}

pub async fn get_file(
    State(app): State<Arc<AppState>>,
    AxumPath((id, name)): AxumPath<(String, String)>,
) -> Result<Response, StatusCode> {
    let (chamber, _) = app.resolve(&id).ok_or(StatusCode::NOT_FOUND)?;
    // Containment: exactly one sanitized segment, no separators, no dotfiles.
    if name.contains('/') || name.contains('\\') || name.starts_with('.') || name != safe_name(&name) {
        return Err(StatusCode::NOT_FOUND);
    }
    let path = attachments_dir(&chamber).join(&name);
    let bytes = std::fs::read(&path).map_err(|_| StatusCode::NOT_FOUND)?;
    Ok((
        [
            (header::CONTENT_TYPE, mime_for(&name).to_string()),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\"")),
        ],
        bytes,
    )
        .into_response())
}

#[cfg(test)]
#[path = "../../unit_tests/hub/routes/files.rs"]
mod tests;
```

Add to the router in `src/hub/mod.rs`:

```rust
.route("/api/chambers/{id}/uploads", post(crate::hub::routes::files::post_upload))
.route("/api/chambers/{id}/files/{name}", get(crate::hub::routes::files::get_file))
```

and set a body limit so the 25 MB cap binds before buffering unbounded input:

```rust
.layer(axum::extract::DefaultBodyLimit::max(crate::hub::routes::files::MAX_ATTACHMENT_BYTES + 1024 * 1024))
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test routes::files`
Expected: all 4 PASS.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock src/hub/routes/files.rs src/hub/routes/mod.rs src/hub/mod.rs src/unit_tests/hub/routes/files.rs
git commit -m "feat(hub): chamber attachment upload/download with containment"
```

---

### Task 6: whoami + token management API

**Files:**
- Create: `src/hub/routes/tokens_api.rs`
- Create: `src/unit_tests/hub/routes/tokens_api.rs`
- Modify: `src/hub/routes/mod.rs`, `src/hub/mod.rs` (routes; `build_router_public` gains an `Extension(ctx)` layer so these handlers can reach the store)

**Interfaces:**
- Consumes: `AuthCtx` (Task 2 — via `Extension<Arc<AuthCtx>>`), `Role` extension.
- Produces:
  - `GET /api/whoami` → `{ "role": "owner" }` or `{ "role": "invite", "name": "Alice", "chambers": [...] }`; in open (no-auth) mode → `{ "role": "owner" }` (loopback user is the owner).
  - `GET /api/tokens` → `{ "invites": [ { "name", "chambers", "created_at", "revoked_at" } ] }` — **never** the token strings.
  - `POST /api/tokens` body `{ "name": "Alice", "chambers": ["id1"] }` → `{ "ok": true, "name", "token", "link_fragment": "#invite=<token>" }` (the only moment the token is visible). Duplicate active name → 400.
  - `POST /api/tokens/{name}/revoke` → `{ "ok": true }` or 404 if no active invite with that name.
  - All three token routes are owner-only via Task 2's default-deny (no extra guard code needed) — but they return 503 if no `Extension<Arc<AuthCtx>>` is present (open mode has no token store).

- [ ] **Step 1: Write the failing tests**

```rust
// src/unit_tests/hub/routes/tokens_api.rs — router-level with apply_auth,
// reusing Task 2's public_router-style setup (owner + invite "Alice").

#[tokio::test]
async fn whoami_reports_role() {
    // GET /api/whoami with owner token → {"role":"owner"}
    // GET /api/whoami with Alice's token → {"role":"invite","name":"Alice",...}
}

#[tokio::test]
async fn token_lifecycle_via_api() {
    // POST /api/tokens {"name":"Bob","chambers":["c1"]} with owner token +
    //   X-Cryo-CSRF → 200, response contains a 64-hex token and
    //   link_fragment "#invite=<token>".
    // GET /api/tokens with owner → lists Alice and Bob, and the raw JSON
    //   body does NOT contain either token string.
    // POST /api/tokens/Bob/revoke → ok; Bob's token now 401s on /api/chambers.
    // POST /api/tokens/Bob/revoke again → 404.
    // Invite token on any /api/tokens route → 403 (Task 2 default-deny).
    // The tokens file on disk reflects the changes (reload with load_tokens).
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test routes::tokens_api`
Expected: compile error.

- [ ] **Step 3: Implement `src/hub/routes/tokens_api.rs`**

```rust
//! Owner-only invite management + whoami. The auth middleware (default-deny)
//! already restricts /api/tokens* to the owner; handlers here only do the
//! work. Token strings appear in exactly one response: creation.

use std::sync::Arc;

use axum::{
    extract::Path as AxumPath,
    http::StatusCode,
    response::Json,
    Extension,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::hub::auth::AuthCtx;
use crate::hub::tokens::Role;

pub async fn get_whoami(role: Option<Extension<Role>>) -> Json<Value> {
    match role {
        Some(Extension(Role::Invite { name, chambers })) => {
            Json(json!({ "role": "invite", "name": name, "chambers": chambers }))
        }
        // Owner token, or open loopback mode (no auth layer): full access.
        _ => Json(json!({ "role": "owner" })),
    }
}

pub async fn get_tokens(ctx: Option<Extension<Arc<AuthCtx>>>) -> Result<Json<Value>, StatusCode> {
    let ctx = ctx.ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let store = ctx.store.read().expect("token store poisoned");
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
        store.create_invite(&req.name, req.chambers).map_err(|_| StatusCode::BAD_REQUEST)?
    };
    ctx.persist().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
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
    ctx.persist().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({ "ok": true })))
}

#[cfg(test)]
#[path = "../../unit_tests/hub/routes/tokens_api.rs"]
mod tests;
```

Routes in `src/hub/mod.rs`:

```rust
.route("/api/whoami", get(crate::hub::routes::tokens_api::get_whoami))
.route("/api/tokens", get(crate::hub::routes::tokens_api::get_tokens))
.route("/api/tokens", post(crate::hub::routes::tokens_api::post_token))
.route("/api/tokens/{name}/revoke", post(crate::hub::routes::tokens_api::post_revoke))
```

and in `build_router_public`, layer the store handle before the auth wrap:

```rust
pub fn build_router_public(app: Arc<WebAppState>, ctx: Arc<crate::hub::auth::AuthCtx>) -> Router {
    let router = build_router_with_state(app.clone()).layer(axum::Extension(ctx.clone()));
    crate::hub::auth::apply_auth(router, app, ctx)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test routes::tokens_api`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/hub/routes/tokens_api.rs src/hub/routes/mod.rs src/hub/mod.rs src/unit_tests/hub/routes/tokens_api.rs
git commit -m "feat(hub): whoami + owner-only invite management API"
```

---

### Task 7: CLI (`cryohub token …`, `cryohub start --public`) and public serve path

**Files:**
- Modify: `src/bin/cryohub.rs` (new `Token` subcommand; `--public` flag on `Start`)
- Modify: `src/hub/mod.rs` (`serve` gains a `public: bool` parameter and builds the public router)
- Modify: `src/hub/lifecycle.rs` if the service unit encodes the start command (propagate `--public` into the installed service invocation — read the file first; mirror how existing flags travel)
- Test: `tests/cli_hub.rs` (extend)

**Interfaces:**
- Consumes: everything above.
- Produces:
  - `cryohub token owner` → creates-if-absent and prints the owner token.
  - `cryohub token create --name Alice --chambers id1,id2` → prints the token and the `#invite=` fragment.
  - `cryohub token list` → table of name/chambers/created/revoked (no token strings).
  - `cryohub token revoke Alice` → tombstones.
  - `cryohub start --public` (and `serve(host, port, public=true)`): loads `default_tokens_path()`, **exits with an error if no owner token exists** ("run `cryohub token owner` first"), serves `build_router_public`.

- [ ] **Step 1: Write the failing CLI tests**

In `tests/cli_hub.rs`, following its existing `assert_cmd` style:

```rust
#[test]
fn token_owner_create_list_revoke_roundtrip() {
    // Use a tempdir as XDG_CONFIG_HOME (or the crate's config override env
    // var if one exists — check hub/config.rs) so the test never touches the
    // real ~/.config/cryo.
    // cryohub token owner            → stdout contains 64-hex token; second
    //                                  run prints the same token.
    // cryohub token create --name Alice --chambers c1 → prints "#invite="
    // cryohub token list             → contains "Alice", NOT the token string
    // cryohub token revoke Alice     → exit 0
    // cryohub token revoke Alice     → nonzero exit (already revoked)
}

#[test]
fn start_public_without_owner_token_fails_fast() {
    // With an empty config home: `cryohub start --public --foreground` must
    // exit nonzero quickly with a message mentioning `cryohub token owner`.
    // (Use --foreground so no OS service is installed; kill after the check.)
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test cli_hub token_`
Expected: FAIL — unknown subcommand.

- [ ] **Step 3: Implement**

In `src/bin/cryohub.rs` add to `Commands`:

```rust
/// Manage access tokens for --public mode
Token {
    #[command(subcommand)]
    action: TokenAction,
},
```

```rust
#[derive(Subcommand)]
enum TokenAction {
    /// Create (if absent) and print the owner token
    Owner,
    /// Create a named invite scoped to chamber ids
    Create {
        #[arg(long)]
        name: String,
        /// Comma-separated chamber ids
        #[arg(long, value_delimiter = ',')]
        chambers: Vec<String>,
    },
    /// List invites (never prints token strings)
    List,
    /// Revoke an invite by name
    Revoke { name: String },
}
```

Handler (in `main`'s match):

```rust
Commands::Token { action } => {
    let path = cryochamber::hub::tokens::default_tokens_path();
    let mut tf = cryochamber::hub::tokens::load_tokens(&path)?;
    match action {
        TokenAction::Owner => {
            let token = tf.ensure_owner()?;
            cryochamber::hub::tokens::save_tokens(&path, &tf)?;
            println!("{token}");
        }
        TokenAction::Create { name, chambers } => {
            let invite = tf.create_invite(&name, chambers)?;
            cryochamber::hub::tokens::save_tokens(&path, &tf)?;
            println!("token: {}", invite.token);
            println!("link fragment: #invite={}", invite.token);
        }
        TokenAction::List => {
            for i in &tf.invites {
                let status = if i.revoked_at.is_some() { "revoked" } else { "active" };
                println!("{}\t{}\t{}\t{}", i.name, status, i.chambers.join(","), i.created_at);
            }
        }
        TokenAction::Revoke { name } => {
            if !tf.revoke(&name) {
                anyhow::bail!("no active invite named '{name}'");
            }
            cryochamber::hub::tokens::save_tokens(&path, &tf)?;
            println!("revoked {name}");
        }
    }
    Ok(())
}
```

Add `#[arg(long)] public: bool` to the `Start` command and thread it to `serve`. In `src/hub/mod.rs`:

```rust
pub async fn serve(host: &str, port: u16, public: bool) -> anyhow::Result<()> {
    let app = Arc::new(WebAppState::global());
    app.refresh();
    let router = if public {
        let path = crate::hub::tokens::default_tokens_path();
        let ctx = crate::hub::auth::AuthCtx::load(&path)?;
        {
            let store = ctx.store.read().expect("token store poisoned");
            if store.owner.is_none() {
                anyhow::bail!(
                    "public mode requires an owner token — run `cryohub token owner` first"
                );
            }
        }
        println!("Cryochamber hub: PUBLIC mode (bearer auth enforced)");
        build_router_public(app, ctx)
    } else {
        build_router_with_state(app)
    };
    // ... existing bind/serve code unchanged ...
}
```

Update the existing `serve` call sites (grep for `hub::serve`) to pass `false`/the flag.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test cli_hub`
Expected: PASS (old and new).

- [ ] **Step 5: Commit**

```bash
git add src/bin/cryohub.rs src/hub/mod.rs src/hub/lifecycle.rs tests/cli_hub.rs
git commit -m "feat(hub): cryohub token subcommands and --public serve mode"
```

---

### Task 8: Authorization-matrix integration sweep

**Files:**
- Create: `tests/hub_auth.rs`

**Interfaces:**
- Consumes: everything above; test setup copied from `tests/hub_multi_chamber.rs` (chambers `alpha`/`beta`, `AppState::local_only`, `scan_workspace`, `EnvVarGuard`).

- [ ] **Step 1: Write the sweep (this IS the test — every row of the spec's matrix)**

```rust
// tests/hub_auth.rs
// Owner + invite (scoped to alpha only) against a two-chamber workspace.
// One #[tokio::test] per row group:

#[tokio::test]
async fn matrix_chamber_routes() {
    // owner: GET alpha/messages 200, GET beta/messages 200
    // invite: GET alpha/messages 200, GET beta/messages 404
    //         POST alpha/send 200 and the inbox file says from: Alice
    //         POST beta/send 404
    // none:  GET alpha/messages 401
}

#[tokio::test]
async fn matrix_list_and_events() {
    // owner: GET /api/chambers → 2 entries
    // invite: GET /api/chambers → 1 entry (alpha)
    // invite SSE: NewMessage{beta} never arrives, NewMessage{alpha} does
}

#[tokio::test]
async fn matrix_owner_only_routes() {
    // invite: POST alpha/start 403, POST alpha/stop 403, POST alpha/restart 403,
    //         POST alpha/reset 403, POST alpha/archive 403, GET alpha/sync 403,
    //         POST /api/chambers/refresh 403, POST /api/chambers/new 403,
    //         GET /api/tokens 403
    // none on the same set: 401
}

#[tokio::test]
async fn matrix_public_surface() {
    // no token: GET / 200, GET /assets/web.css 200, GET /c/anything 200
}
```

Every request goes through `build_router_public` + tower `oneshot`, with
`host: 127.0.0.1` and (for POSTs) `X-Cryo-CSRF: 1` headers. Assert exact
status codes; for the 403-vs-404 distinction add a message to each assert
naming the route.

- [ ] **Step 2: Run to verify current state**

Run: `cargo test --test hub_auth`
Expected: PASS if Tasks 1–7 are correct — any failure here is a real
matrix violation; fix the offending task's code, not the test.

- [ ] **Step 3: Full suite**

Run: `cargo test`
Expected: entire workspace green.

- [ ] **Step 4: Commit**

```bash
git add tests/hub_auth.rs
git commit -m "test(hub): authorization matrix integration sweep"
```
