//! Authorization-matrix sweep for public-mode cryohub.
//!
//! Every row of the spec's matrix, end to end through `build_router_public`:
//! a real two-chamber workspace, a real token store, real HTTP requests.
//! The unit tests cover the classifier in isolation; this file is the check
//! that the classifier, the guard, the route table, and the handlers agree.
//!
//! Two rules this file holds itself to:
//!
//! 1. **Every negative assertion is preceded by its positive.** A 404 for an
//!    out-of-scope invite proves nothing unless the same route 200s for the
//!    owner — otherwise a typo'd URL passes the test for the wrong reason.
//! 2. **403 and 404 are never interchangeable.** Out-of-scope is 404 (an invite
//!    must not be able to probe which chamber ids exist); wrong-role is 403.
//!    Each assertion names its route so a failure says which row broke.

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use cryochamber::config;
use cryochamber::hub::auth::AuthCtx;
use cryochamber::hub::config::HubConfig;
use cryochamber::hub::state::{AppState, SseEvent};
use cryochamber::hub::tokens::{save_tokens, TokenFile};
use cryochamber::hub::{build_router_public_with_config, discovery};
use tokio_stream::StreamExt;
use tower::ServiceExt;

struct Matrix {
    _tmp: tempfile::TempDir,
    workspace: PathBuf,
    app: Arc<AppState>,
    router: axum::Router,
    owner: String,
    /// Alice — scoped to `alpha` only.
    invite: String,
    alpha: String,
    beta: String,
}

/// As [`setup`], but the invite's scope is written in the given form. `setup`
/// itself uses the canonical (encoded) id.
fn setup_with_scope(scope_of: impl Fn(&str) -> String) -> Matrix {
    let tmp = tempfile::tempdir().unwrap();
    for name in ["alpha", "beta"] {
        let d = tmp.path().join(name);
        std::fs::create_dir_all(&d).unwrap();
        config::save_config(&d.join("cryo.toml"), &config::CryoConfig::default()).unwrap();
    }

    let app = Arc::new(AppState::local_only(tmp.path().to_path_buf()));
    let mut idx = discovery::scan_workspace(tmp.path());
    discovery::populate_runtime(&mut idx);
    *app.chambers.write().unwrap() = idx;

    let id_of = |name: &str| -> String {
        app.chambers
            .read()
            .unwrap()
            .values()
            .find(|e| e.name == name)
            .unwrap_or_else(|| panic!("chamber {name} should have been discovered"))
            .id
            .clone()
    };
    let alpha = id_of("alpha");
    let beta = id_of("beta");
    assert_ne!(alpha, beta, "the two chambers must have distinct ids");

    let mut tf = TokenFile::default();
    let owner = tf.ensure_owner().unwrap();
    let invite = tf
        .create_invite("Alice", vec![scope_of(&alpha)])
        .unwrap()
        .token;
    let tokens_path = tmp.path().join("tokens.json");
    save_tokens(&tokens_path, &tf).unwrap();
    let ctx = AuthCtx::load(&tokens_path).unwrap();

    // A minimal installed console, so the public page surface is the same on
    // every machine: the default console root would be whatever the developer
    // running these tests has in ~/.cryo/console.
    let console = tmp.path().join("console");
    std::fs::create_dir_all(console.join("assets")).unwrap();
    std::fs::write(
        console.join("index.html"),
        "<!doctype html><h1>console</h1>",
    )
    .unwrap();
    std::fs::write(console.join("assets/app.js"), "export const x = 1;").unwrap();
    let config = HubConfig {
        console_dir: Some(console),
        ..HubConfig::default()
    };
    let router = build_router_public_with_config(app.clone(), ctx, config);
    Matrix {
        workspace: tmp.path().to_path_buf(),
        _tmp: tmp,
        app,
        router,
        owner,
        invite,
        alpha,
        beta,
    }
}

/// Two chambers (`alpha`, `beta`), an owner token, and one invite scoped to
/// `alpha`. The index is populated from a workspace scan rather than
/// `registry::list()`, so the test is isolated from whatever chambers exist on
/// the developer's machine (same approach as `tests/hub_multi_chamber.rs`).
///
/// Watchers are deliberately *not* wired: the SSE rows drive `app.tx` directly,
/// and a filesystem watcher racing in with its own events would make those
/// assertions nondeterministic.
fn setup() -> Matrix {
    setup_with_scope(|alpha| alpha.to_string())
}

/// Who is making the request.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum As {
    Owner,
    Invite,
    Anonymous,
}

impl Matrix {
    fn token(&self, who: As) -> Option<&str> {
        match who {
            As::Owner => Some(&self.owner),
            As::Invite => Some(&self.invite),
            As::Anonymous => None,
        }
    }

    fn request(&self, who: As, method: &str, uri: &str, body: Option<&str>) -> Request<Body> {
        // `host: 127.0.0.1` satisfies the DNS-rebinding guard; `X-Cryo-CSRF`
        // satisfies the CSRF guard on non-GET. Both are unconditional here so
        // that any rejection we observe is an *authorization* decision.
        let mut req = Request::builder()
            .method(method)
            .uri(uri)
            .header("host", "127.0.0.1")
            .header("x-cryo-csrf", "1");
        if let Some(t) = self.token(who) {
            req = req.header("authorization", format!("Bearer {t}"));
        }
        match body {
            Some(b) => req
                .header("content-type", "application/json")
                .body(Body::from(b.to_string()))
                .unwrap(),
            None => req.body(Body::empty()).unwrap(),
        }
    }

    async fn call(
        &self,
        who: As,
        method: &str,
        uri: &str,
        body: Option<&str>,
    ) -> (StatusCode, String) {
        let resp = self
            .router
            .clone()
            .oneshot(self.request(who, method, uri, body))
            .await
            .unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, String::from_utf8_lossy(&bytes).into_owned())
    }

    async fn status(&self, who: As, method: &str, uri: &str) -> StatusCode {
        self.call(who, method, uri, None).await.0
    }

    /// Open an SSE stream as `who` and return its body stream.
    async fn events(&self, who: As) -> axum::body::BodyDataStream {
        let resp = self
            .router
            .clone()
            .oneshot(self.request(who, "GET", "/api/events", None))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "SSE stream should open");
        resp.into_body().into_data_stream()
    }

    /// `POST /api/chambers/{id}/uploads` with one multipart `file` field.
    async fn upload(
        &self,
        who: As,
        chamber: &str,
        filename: &str,
        bytes: &[u8],
    ) -> (StatusCode, String) {
        let mut req = Request::builder()
            .method("POST")
            .uri(format!("/api/chambers/{chamber}/uploads"))
            .header("host", "127.0.0.1")
            .header("x-cryo-csrf", "1")
            .header(
                "content-type",
                format!("multipart/form-data; boundary={BOUNDARY}"),
            );
        if let Some(t) = self.token(who) {
            req = req.header("authorization", format!("Bearer {t}"));
        }
        let resp = self
            .router
            .clone()
            .oneshot(
                req.body(Body::from(multipart_body(filename, bytes)))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, String::from_utf8_lossy(&bytes).into_owned())
    }

    fn inbox(&self, name: &str) -> Vec<cryochamber::message::Message> {
        let dir = self.workspace.join(name).canonicalize().unwrap();
        cryochamber::message::read_inbox(&dir)
            .unwrap()
            .into_iter()
            .map(|(_, m)| m)
            .collect()
    }
}

const BOUNDARY: &str = "cryoboundary123";

/// The multipart wire format for a single `file` field — the repo ships no
/// multipart client, so build it by hand.
fn multipart_body(filename: &str, bytes: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(
        format!(
            "--{BOUNDARY}\r\ncontent-disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\ncontent-type: application/octet-stream\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());
    body
}

/// The stored name an upload response advertises.
fn stored_name(body: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    v["name"].as_str().expect("name in upload response").into()
}

fn new_message(chamber_id: &str, body: &str) -> SseEvent {
    SseEvent::NewMessage {
        id: "inbox/2026-08-15T10-00-00_hi_0001.md".into(),
        chamber_id: chamber_id.into(),
        direction: "inbox".into(),
        from: "tester".into(),
        subject: String::new(),
        body: body.into(),
        timestamp: "t".into(),
        is_question: false,
    }
}

/// Drain an SSE body until `frames` `\n\n`-terminated frames arrive or the
/// deadline passes, returning the raw wire text.
async fn drain(stream: &mut axum::body::BodyDataStream, frames: usize, ms: u64) -> String {
    let mut text = String::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(ms);
    while text.matches("\n\n").count() < frames {
        match tokio::time::timeout_at(deadline, stream.next()).await {
            Ok(Some(Ok(bytes))) => text.push_str(&String::from_utf8_lossy(&bytes)),
            Ok(Some(Err(e))) => panic!("stream error: {e}"),
            Ok(None) => break,
            Err(_) => break,
        }
    }
    text
}

// ---------------------------------------------------------------------------
// Row group 1: per-chamber read + write routes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn matrix_chamber_routes() {
    let m = setup();

    // Only `messages` is chamber-scoped: `status`/`todos` are working state
    // and belong to the owner-only matrix below.
    for verb in ["messages"] {
        let alpha = format!("/api/chambers/{}/{verb}", m.alpha);
        let beta = format!("/api/chambers/{}/{verb}", m.beta);

        // Owner reaches both — this is the positive path the negatives below
        // are measured against. Without it, a mistyped URI would 404 for
        // everyone and the invite-out-of-scope row would pass vacuously.
        assert_eq!(
            m.status(As::Owner, "GET", &alpha).await,
            StatusCode::OK,
            "owner GET {alpha}"
        );
        assert_eq!(
            m.status(As::Owner, "GET", &beta).await,
            StatusCode::OK,
            "owner GET {beta}"
        );

        // In-scope invite: served. Out-of-scope: 404, never 403 — a guest must
        // not be able to tell an existing chamber from a nonexistent one.
        assert_eq!(
            m.status(As::Invite, "GET", &alpha).await,
            StatusCode::OK,
            "in-scope invite GET {alpha}"
        );
        assert_eq!(
            m.status(As::Invite, "GET", &beta).await,
            StatusCode::NOT_FOUND,
            "out-of-scope invite GET {beta} must be 404, not 403"
        );

        // No token at all.
        assert_eq!(
            m.status(As::Anonymous, "GET", &alpha).await,
            StatusCode::UNAUTHORIZED,
            "anonymous GET {alpha}"
        );
        assert_eq!(
            m.status(As::Anonymous, "GET", &beta).await,
            StatusCode::UNAUTHORIZED,
            "anonymous GET {beta}"
        );
    }

    // --- sending ---------------------------------------------------------
    let alpha_send = format!("/api/chambers/{}/send", m.alpha);
    let beta_send = format!("/api/chambers/{}/send", m.beta);

    // The invite's message is stamped with its *token identity*, not with
    // whatever the request body claims. Claiming to be someone else here is
    // the exact attack the stamping exists to stop.
    let (status, body) = m
        .call(
            As::Invite,
            "POST",
            &alpha_send,
            Some(r#"{"body":"hello from alice","from":"Mallory"}"#),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "invite POST {alpha_send}: {body}");

    let alpha_inbox = m.inbox("alpha");
    assert_eq!(alpha_inbox.len(), 1, "invite's message should have landed");
    assert_eq!(
        alpha_inbox[0].from, "Alice",
        "invite must be stamped with its token name, not the body's `from`"
    );
    assert_eq!(alpha_inbox[0].body, "hello from alice");

    // Out-of-scope write: 404, and nothing written.
    let (status, _) = m
        .call(
            As::Invite,
            "POST",
            &beta_send,
            Some(r#"{"body":"SHOULD-NOT-LAND"}"#),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "out-of-scope invite POST {beta_send} must be 404"
    );
    assert!(
        m.inbox("beta").is_empty(),
        "out-of-scope send must not write to beta"
    );

    // Owner may write to either — but in public mode the sender is the
    // server-configured owner name, not whatever the body claims. Comparing
    // against the loaded config rather than a literal keeps this honest on a
    // machine whose `owner_name` is customized.
    let (status, _) = m
        .call(
            As::Owner,
            "POST",
            &beta_send,
            Some(r#"{"body":"owner note","from":"Mallory"}"#),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "owner POST {beta_send}");
    let beta_inbox = m.inbox("beta");
    assert_eq!(beta_inbox.len(), 1);
    assert_ne!(
        beta_inbox[0].from, "Mallory",
        "the owner's client-supplied `from` must be ignored in public mode"
    );
    assert_eq!(
        beta_inbox[0].from,
        cryochamber::hub::config::load_config()
            .unwrap_or_default()
            .owner_name,
        "the owner sends as the configured owner_name (default `human`)"
    );

    assert_eq!(
        m.status(As::Anonymous, "POST", &alpha_send).await,
        StatusCode::UNAUTHORIZED,
        "anonymous POST {alpha_send}"
    );
}

// ---------------------------------------------------------------------------
// Row group 2: index listing and the SSE stream
// ---------------------------------------------------------------------------

#[tokio::test]
async fn matrix_list_and_events() {
    let m = setup();

    let (status, body) = m.call(As::Owner, "GET", "/api/chambers", None).await;
    assert_eq!(status, StatusCode::OK, "owner GET /api/chambers");
    let owner_list: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        owner_list.as_array().unwrap().len(),
        2,
        "owner sees the whole index: {body}"
    );

    let (status, body) = m.call(As::Invite, "GET", "/api/chambers", None).await;
    assert_eq!(status, StatusCode::OK, "invite GET /api/chambers");
    let invite_list: serde_json::Value = serde_json::from_str(&body).unwrap();
    let invite_list = invite_list.as_array().unwrap();
    assert_eq!(
        invite_list.len(),
        1,
        "invite's sidebar must not reveal beta exists: {body}"
    );
    assert_eq!(invite_list[0]["id"], m.alpha);
    assert!(
        !body.contains(&m.beta),
        "beta's id leaked into the invite's chamber list: {body}"
    );

    assert_eq!(
        m.status(As::Anonymous, "GET", "/api/chambers").await,
        StatusCode::UNAUTHORIZED,
        "anonymous GET /api/chambers"
    );

    // --- SSE -------------------------------------------------------------
    // Owner stream first: it establishes that both events really are broadcast,
    // so the invite's missing beta event below is filtering, not a dead channel.
    let mut owner_stream = m.events(As::Owner).await;
    m.app.tx.send(new_message(&m.alpha, "ALPHA-EVENT")).unwrap();
    m.app.tx.send(new_message(&m.beta, "BETA-SECRET")).unwrap();
    let owner_text = drain(&mut owner_stream, 2, 2000).await;
    assert!(
        owner_text.contains("ALPHA-EVENT") && owner_text.contains("BETA-SECRET"),
        "owner stream must be unfiltered: {owner_text}"
    );

    let mut invite_stream = m.events(As::Invite).await;
    m.app.tx.send(new_message(&m.beta, "BETA-SECRET")).unwrap();
    m.app.tx.send(new_message(&m.alpha, "ALPHA-EVENT")).unwrap();
    let invite_text = drain(&mut invite_stream, 1, 2000).await;
    assert!(
        invite_text.contains("ALPHA-EVENT"),
        "in-scope event must reach the invite: {invite_text}"
    );
    assert!(
        !invite_text.contains("BETA-SECRET"),
        "out-of-scope chamber leaked into the invite's stream: {invite_text}"
    );

    // Log lines never reach a guest, even for a chamber in scope: the owner
    // stream proves they are broadcast, the invite stream proves they are
    // filtered by *kind*, not only by chamber.
    let mut owner_log_stream = m.events(As::Owner).await;
    let mut invite_log_stream = m.events(As::Invite).await;
    m.app
        .tx
        .send(SseEvent::LogLine {
            chamber_id: m.alpha.clone(),
            line: "LOG-SENTINEL sk-secret".into(),
        })
        .unwrap();
    m.app
        .tx
        .send(new_message(&m.alpha, "ALPHA-AFTER-LOG"))
        .unwrap();
    let owner_log_text = drain(&mut owner_log_stream, 2, 2000).await;
    assert!(
        owner_log_text.contains("LOG-SENTINEL"),
        "owner must receive log lines: {owner_log_text}"
    );
    let invite_log_text = drain(&mut invite_log_stream, 1, 2000).await;
    assert!(
        invite_log_text.contains("ALPHA-AFTER-LOG"),
        "the invite stream is live (message delivered): {invite_log_text}"
    );
    assert!(
        !invite_log_text.contains("LOG-SENTINEL"),
        "an in-scope log line leaked to a guest: {invite_log_text}"
    );

    assert_eq!(
        m.status(As::Anonymous, "GET", "/api/events").await,
        StatusCode::UNAUTHORIZED,
        "anonymous GET /api/events"
    );
}

// ---------------------------------------------------------------------------
// Row group 2b: attachments (upload + download)
// ---------------------------------------------------------------------------

/// Attachments are chamber-scoped like messages, and they carry file *content*
/// — the one surface where a scope leak hands over bytes rather than metadata.
/// Both verbs go through the production router here, not the open one.
#[tokio::test]
async fn matrix_attachment_routes() {
    let m = setup();

    // --- owner: the positive control for every negative below -------------
    let (status, body) = m
        .upload(As::Owner, &m.alpha, "owner.txt", b"owner-in-alpha")
        .await;
    assert_eq!(status, StatusCode::OK, "owner upload to alpha: {body}");
    let alpha_file = stored_name(&body);

    let (status, body) = m
        .upload(As::Owner, &m.beta, "secret.txt", b"BETA-SECRET-BYTES")
        .await;
    assert_eq!(status, StatusCode::OK, "owner upload to beta: {body}");
    let beta_file = stored_name(&body);

    let alpha_url = format!("/api/chambers/{}/files/{alpha_file}", m.alpha);
    let beta_url = format!("/api/chambers/{}/files/{beta_file}", m.beta);

    let (status, body) = m.call(As::Owner, "GET", &beta_url, None).await;
    assert_eq!(status, StatusCode::OK, "owner GET {beta_url}");
    assert_eq!(
        body, "BETA-SECRET-BYTES",
        "the file really exists, so the invite's 404 below is a scope decision"
    );

    // --- in-scope invite: may upload to and download from alpha -----------
    let (status, body) = m
        .upload(As::Invite, &m.alpha, "guest.txt", b"guest-bytes")
        .await;
    assert_eq!(status, StatusCode::OK, "in-scope invite upload: {body}");
    let guest_file = stored_name(&body);

    let (status, body) = m
        .call(
            As::Invite,
            "GET",
            &format!("/api/chambers/{}/files/{guest_file}", m.alpha),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "invite reads back its own upload");
    assert_eq!(body, "guest-bytes");

    let (status, _) = m.call(As::Invite, "GET", &alpha_url, None).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "an in-scope invite shares the chamber's attachments with the owner"
    );

    // --- out-of-scope invite: 404 on both verbs, and nothing is written ---
    let (status, _) = m
        .upload(As::Invite, &m.beta, "sneak.txt", b"SHOULD-NOT-LAND")
        .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "out-of-scope upload must be 404, never 403"
    );
    let (status, body) = m.call(As::Invite, "GET", &beta_url, None).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "out-of-scope download must be 404"
    );
    assert!(
        !body.contains("BETA-SECRET-BYTES"),
        "beta's file content leaked: {body}"
    );

    // --- anonymous: 401 on both -------------------------------------------
    let (status, _) = m.upload(As::Anonymous, &m.alpha, "anon.txt", b"anon").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "anonymous upload");
    assert_eq!(
        m.status(As::Anonymous, "GET", &alpha_url).await,
        StatusCode::UNAUTHORIZED,
        "anonymous GET {alpha_url}"
    );
}

// ---------------------------------------------------------------------------
// Row group 3: owner-only routes (default-deny)
// ---------------------------------------------------------------------------

/// Every route an invite must never reach. Listed as `(method, path)` where the
/// path is a format string with `{id}` standing in for the alpha chamber id —
/// alpha is the chamber the invite *is* scoped to, so a 403 here is about the
/// role, never about the scope.
const OWNER_ONLY: &[(&str, &str)] = &[
    // Sync actions drive a background daemon for the whole chamber; an invite
    // has no business starting or stopping one. The guard runs outside the
    // router, so a non-owner is rejected before `post_sync_action` can act —
    // which is why listing it here triggers no side effect.
    ("POST", "/api/chambers/{id}/sync/zulip/pull"),
    ("POST", "/api/chambers/{id}/start"),
    ("POST", "/api/chambers/{id}/stop"),
    ("POST", "/api/chambers/{id}/restart"),
    ("POST", "/api/chambers/{id}/reset"),
    ("POST", "/api/chambers/{id}/archive"),
    ("POST", "/api/chambers/{id}/unarchive"),
    ("GET", "/api/chambers/{id}/sync"),
    // Working state — log tail, plan/notes, settings, todos — is the owner's,
    // even for a chamber the invite can chat in.
    ("GET", "/api/chambers/{id}/status"),
    ("GET", "/api/chambers/{id}/todos"),
    ("POST", "/api/chambers/refresh"),
    ("POST", "/api/chambers/new"),
    ("GET", "/api/tokens"),
    ("POST", "/api/tokens"),
    ("POST", "/api/tokens/Alice/revoke"),
];

#[tokio::test]
async fn matrix_owner_only_routes() {
    let m = setup();

    for (method, template) in OWNER_ONLY {
        let uri = template.replace("{id}", &m.alpha);

        // Positive path: prove the route EXISTS before asserting anyone is
        // refused it. This matters more here than anywhere else in the file,
        // because the guard runs *outside* the router: a typo'd path would be
        // classified owner-only by default-deny and 403 the invite exactly like
        // a real route does. So the invite-403 row alone is worthless.
        //
        // The proof is method-shaped rather than a real invocation, because
        // half these routes start daemons or mutate global state. An owner GET
        // against a registered path yields 200 (the path also serves GET) or
        // 405 (registered, but POST-only); an *unregistered* path yields 404.
        // `owner_get_probe_distinguishes_missing_routes` is the control that
        // makes this discrimination meaningful.
        let existence = m.status(As::Owner, "GET", &uri).await;
        assert!(
            matches!(existence, StatusCode::OK | StatusCode::METHOD_NOT_ALLOWED),
            "route {method} {uri} should exist, but an owner GET probe returned {existence} \
             (404 here means the path is not registered and the invite-403 row below \
             would pass vacuously)"
        );

        assert_eq!(
            m.status(As::Invite, method, &uri).await,
            StatusCode::FORBIDDEN,
            "invite {method} {uri} must be 403 (wrong role, in-scope chamber)"
        );
        assert_eq!(
            m.status(As::Anonymous, method, &uri).await,
            StatusCode::UNAUTHORIZED,
            "anonymous {method} {uri} must be 401"
        );
    }
}

#[tokio::test]
async fn owner_get_probe_distinguishes_missing_routes() {
    // Control for `matrix_owner_only_routes`: an owner GET against a path that
    // is NOT registered returns 404, not 405. Without this, the 405 probe above
    // could be asserting on a status the router hands out indiscriminately.
    let m = setup();
    for uri in [
        format!("/api/chambers/{}/not-a-real-verb", m.alpha),
        "/api/chambers/not-a-real-collection".to_string(),
        "/api/tokens/Alice/not-a-real-verb".to_string(),
    ] {
        assert_eq!(
            m.status(As::Owner, "GET", &uri).await,
            StatusCode::NOT_FOUND,
            "unregistered path {uri} should 404 for the owner, not 405"
        );
    }
}

#[tokio::test]
async fn matrix_unknown_api_routes_are_owner_only_by_default() {
    // Default-deny: a route nobody classified is owner-only. This is the
    // property that makes a future `/api/whatever` fail closed.
    let m = setup();
    for uri in ["/api/brand-new-feature", "/api", "/api/chambers/x/y/z"] {
        assert_eq!(
            m.status(As::Invite, "GET", uri).await,
            StatusCode::FORBIDDEN,
            "unclassified {uri} must be forbidden to an invite"
        );
        assert_eq!(
            m.status(As::Anonymous, "GET", uri).await,
            StatusCode::UNAUTHORIZED,
            "unclassified {uri} must be 401 without a token"
        );
    }
}

// ---------------------------------------------------------------------------
// Row group 4: the public surface
// ---------------------------------------------------------------------------

#[tokio::test]
async fn matrix_public_surface() {
    let m = setup();

    // `/c/{chamber-id}` is deliberately absent: that was the retired
    // dashboard's per-chamber URL. The console has no page routes of its own —
    // any non-file path is the app shell, which then asks for a token.
    for uri in ["/", "/c/anything", "/assets/app.js"] {
        assert_eq!(
            m.status(As::Anonymous, "GET", uri).await,
            StatusCode::OK,
            "{uri} must stay reachable without a token (it is how a guest gets the app shell that then asks for a token)"
        );
    }

    // The contrast that makes the block above meaningful: the guard is on, it
    // simply does not apply outside `/api`.
    assert_eq!(
        m.status(As::Anonymous, "GET", "/api/whoami").await,
        StatusCode::UNAUTHORIZED,
        "the guard must still be active"
    );

    // whoami is the one /api route every authenticated role may read.
    let (status, body) = m.call(As::Owner, "GET", "/api/whoami", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("\"owner\""), "{body}");
    let (status, body) = m.call(As::Invite, "GET", "/api/whoami", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Alice"), "{body}");
}

// ---------------------------------------------------------------------------
// Row group 5: revocation binds to the credential, not the name
// ---------------------------------------------------------------------------

/// An SSE stream is authorized once, at connect, and then lives indefinitely.
/// It must therefore be bound to the exact bearer token that opened it. Binding
/// it to the invite *name* let this sequence resurrect a revoked stream:
/// revoke `Alice`, mint a replacement invite also called `Alice`, and the old
/// connection — which never presented the new secret — resumed under the new
/// invite's scope.
#[tokio::test]
async fn a_revoked_stream_is_not_resurrected_by_a_same_named_replacement_invite() {
    let m = setup();

    let mut stream = m.events(As::Invite).await;
    m.app
        .tx
        .send(new_message(&m.alpha, "BEFORE-REVOKE"))
        .unwrap();
    let text = drain(&mut stream, 1, 2000).await;
    assert!(
        text.contains("BEFORE-REVOKE"),
        "the stream must be delivering before revocation, or the negative below is vacuous: {text}"
    );

    let (status, body) = m
        .call(As::Owner, "POST", "/api/tokens/Alice/revoke", None)
        .await;
    assert_eq!(status, StatusCode::OK, "revoke Alice: {body}");

    // The replacement: same name, same scope, brand-new secret.
    let (status, body) = m
        .call(
            As::Owner,
            "POST",
            "/api/tokens",
            Some(&format!(r#"{{"name":"Alice","chambers":["{}"]}}"#, m.alpha)),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "recreate Alice: {body}");

    m.app
        .tx
        .send(new_message(&m.alpha, "AFTER-REVOKE"))
        .unwrap();
    let tail = drain(&mut stream, 1, 500).await;
    assert!(
        !tail.contains("AFTER-REVOKE"),
        "a revoked stream was resurrected by a same-named replacement invite: {tail}"
    );
    // ...and the stream has ENDED, not merely fallen silent: a silent stream
    // would keep the guest's console looking signed in on cached data until
    // its next request. EOF makes the client reconnect and learn about the
    // 401 right away.
    let ended = tokio::time::timeout(std::time::Duration::from_millis(500), stream.next()).await;
    assert!(
        matches!(ended, Ok(None)),
        "a revoked stream must end (EOF), got {ended:?}"
    );

    // ...and the revoked token is dead on ordinary routes too.
    assert_eq!(
        m.status(
            As::Invite,
            "GET",
            &format!("/api/chambers/{}/messages", m.alpha)
        )
        .await,
        StatusCode::UNAUTHORIZED,
        "the revoked token must not authenticate any request"
    );
}

// ---------------------------------------------------------------------------
// Row group 6: scope forms
// ---------------------------------------------------------------------------

/// A scope may be handed to `token create` as the decoded chamber path
/// (`/home/me/chambers/alpha`) rather than the encoded index id. Before the
/// canonicalization fix, such an invite passed the route guard (which decoded
/// the path param) but saw an empty chamber list and received no SSE events,
/// because those two filters compared the encoded id directly. All three must
/// now agree.
#[tokio::test]
async fn a_decoded_form_scope_is_honored_by_guard_list_and_stream() {
    let m = setup_with_scope(|alpha| {
        let decoded = discovery::decode_id(alpha)
            .expect("chamber ids are percent-encoded paths")
            .to_string_lossy()
            .into_owned();
        assert_ne!(
            decoded, alpha,
            "test needs an id that actually gets encoded"
        );
        decoded
    });

    // 1. route guard
    let uri = format!("/api/chambers/{}/messages", m.alpha);
    assert_eq!(
        m.status(As::Invite, "GET", &uri).await,
        StatusCode::OK,
        "decoded-form scope must pass the route guard"
    );

    // 2. chamber list
    let (status, body) = m.call(As::Invite, "GET", "/api/chambers", None).await;
    assert_eq!(status, StatusCode::OK);
    let list: serde_json::Value = serde_json::from_str(&body).unwrap();
    let list = list.as_array().unwrap();
    assert_eq!(
        list.len(),
        1,
        "decoded-form scope must still list its chamber: {body}"
    );
    assert_eq!(list[0]["id"], m.alpha);

    // 3. SSE stream
    let mut stream = m.events(As::Invite).await;
    m.app.tx.send(new_message(&m.beta, "BETA-SECRET")).unwrap();
    m.app.tx.send(new_message(&m.alpha, "ALPHA-EVENT")).unwrap();
    let text = drain(&mut stream, 1, 2000).await;
    assert!(
        text.contains("ALPHA-EVENT"),
        "decoded-form scope must still receive its chamber's events: {text}"
    );
    assert!(
        !text.contains("BETA-SECRET"),
        "out-of-scope chamber leaked: {text}"
    );
}

/// The console is the *login page* of a public hub: it has to load before the
/// visitor has a token to load it with. `classify` calls every non-`/api` path
/// Public, so this is really a check that serving the console from disk did
/// not accidentally move it behind the guard.
#[tokio::test]
async fn the_console_is_reachable_without_a_token_in_public_mode() {
    let tmp = tempfile::tempdir().unwrap();
    let dist = tmp.path().join("dist");
    std::fs::create_dir_all(dist.join("assets")).unwrap();
    std::fs::write(dist.join("index.html"), "<!doctype html><h1>console</h1>").unwrap();
    std::fs::write(dist.join("assets/index-abc123.js"), "export const x = 1;").unwrap();

    let mut tf = TokenFile::default();
    tf.ensure_owner().unwrap();
    let tokens_path = tmp.path().join("tokens.json");
    save_tokens(&tokens_path, &tf).unwrap();
    let ctx = AuthCtx::load(&tokens_path).unwrap();

    let app = Arc::new(AppState::local_only(tmp.path().to_path_buf()));
    let config = cryochamber::hub::config::HubConfig {
        console_dir: Some(dist.clone()),
        ..cryochamber::hub::config::HubConfig::default()
    };
    let router = cryochamber::hub::build_router_public_with_config(app, ctx, config);

    // Entry point, a client-side route, and a build asset — the three requests
    // a cold browser makes before it can present any credential at all.
    for (uri, needle) in [
        ("/", "<h1>console</h1>"),
        ("/c/anything", "<h1>console</h1>"),
        ("/assets/index-abc123.js", "export const x = 1;"),
    ] {
        let req = Request::builder()
            .method("GET")
            .uri(uri)
            .header("host", "127.0.0.1")
            .body(Body::empty())
            .unwrap();
        let resp = router.clone().oneshot(req).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "{uri} must be served without an Authorization header"
        );
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body = String::from_utf8_lossy(&bytes);
        assert!(body.contains(needle), "{uri} body was {body}");
    }

    // The API is still guarded — serving static files must not have widened
    // the Public class beyond the pages themselves.
    let req = Request::builder()
        .method("GET")
        .uri("/api/chambers")
        .header("host", "127.0.0.1")
        .body(Body::empty())
        .unwrap();
    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "the API must still require a token when the console is served"
    );
}

#[tokio::test]
async fn a_guest_who_floods_send_is_throttled_with_retry_after() {
    // Every accepted send wakes an agent on the owner's bill; an invite must
    // run out of tokens, and be told when to try again.
    let m = setup();
    let alpha_send = format!("/api/chambers/{}/send", m.alpha);
    let body = Some(r#"{"body":"hi"}"#);

    for i in 0..5 {
        let (status, text) = m.call(As::Invite, "POST", &alpha_send, body).await;
        assert_eq!(status, StatusCode::OK, "burst send {i}: {text}");
    }
    let (status, text) = m.call(As::Invite, "POST", &alpha_send, body).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "sixth send: {text}");
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["error"], "rate limited");
    assert_eq!(
        m.inbox("alpha").len(),
        5,
        "the refused send must not be written"
    );

    // Uploads draw from the same bucket.
    let (status, _) = m.upload(As::Invite, &m.alpha, "x.txt", b"hello").await;
    assert_eq!(
        status,
        StatusCode::TOO_MANY_REQUESTS,
        "upload after the burst"
    );

    // The owner has a bucket of their own — unaffected by the guest's flood.
    let (status, text) = m.call(As::Owner, "POST", &alpha_send, body).await;
    assert_eq!(status, StatusCode::OK, "owner send: {text}");
}

#[tokio::test]
async fn throttled_send_carries_a_whole_second_retry_after_header() {
    let m = setup();
    let alpha_send = format!("/api/chambers/{}/send", m.alpha);
    for _ in 0..5 {
        m.call(As::Invite, "POST", &alpha_send, Some(r#"{"body":"hi"}"#))
            .await;
    }
    let req = m.request(As::Invite, "POST", &alpha_send, Some(r#"{"body":"hi"}"#));
    let resp = m.router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
    let retry: u64 = resp
        .headers()
        .get(axum::http::header::RETRY_AFTER)
        .expect("Retry-After present")
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!((1..=6).contains(&retry), "got Retry-After {retry}");
}

#[tokio::test]
async fn an_exhausted_bucket_never_speaks_before_the_authorization_decision() {
    // The throttle answers only for requests that were going to be served.
    // If it ran first, a spent credential would learn "429" where it should
    // have learned "404" — turning the limiter into the chamber-enumeration
    // oracle the whole scope design exists to deny — and would mask the
    // owner-only 403 rows for anyone who happened to be out of tokens.
    let m = setup();
    let body = Some(r#"{"body":"hi"}"#);
    let missing = "/api/chambers/definitely-not-a-chamber/send".to_string();
    let beta_send = format!("/api/chambers/{}/send", m.beta);

    // Spend the invite's bucket, then probe past it.
    let alpha_send = format!("/api/chambers/{}/send", m.alpha);
    for _ in 0..5 {
        m.call(As::Invite, "POST", &alpha_send, body).await;
    }
    assert_eq!(
        m.call(As::Invite, "POST", &alpha_send, body).await.0,
        StatusCode::TOO_MANY_REQUESTS,
        "the invite's bucket must be empty for the rest of this test to mean anything"
    );
    assert_eq!(
        m.call(As::Invite, "POST", &beta_send, body).await.0,
        StatusCode::NOT_FOUND,
        "out-of-scope send stays 404 even with an empty bucket"
    );
    assert!(m.inbox("beta").is_empty(), "nothing may reach beta");
    assert_eq!(
        m.call(As::Invite, "POST", &missing, body).await.0,
        StatusCode::NOT_FOUND,
        "nonexistent chamber stays 404 even with an empty bucket"
    );
    assert_eq!(
        m.status(
            As::Invite,
            "GET",
            &format!("/api/chambers/{}/status", m.alpha)
        )
        .await,
        StatusCode::FORBIDDEN,
        "an owner-only route stays 403 for a throttled invite"
    );

    // The owner clears the scope guard, so their probe reaches the handler:
    // this is the row that pins `resolve` *inside* `post_send` running before
    // the gate, not merely the guard running before the router.
    for _ in 0..5 {
        m.call(As::Owner, "POST", &alpha_send, body).await;
    }
    assert_eq!(
        m.call(As::Owner, "POST", &alpha_send, body).await.0,
        StatusCode::TOO_MANY_REQUESTS,
        "the owner's bucket must be empty too"
    );
    assert_eq!(
        m.call(As::Owner, "POST", &missing, body).await.0,
        StatusCode::NOT_FOUND,
        "an unresolvable id is 404 from the handler, never 429"
    );
    let (status, _) = m
        .upload(As::Owner, "definitely-not-a-chamber", "x.txt", b"hi")
        .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "same ordering in the upload handler"
    );
}
