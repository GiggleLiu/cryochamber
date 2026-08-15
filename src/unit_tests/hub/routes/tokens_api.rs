use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use crate::hub::auth::AuthCtx;
use crate::hub::state::AppState;
use crate::hub::tokens::{load_tokens, save_tokens, TokenFile};

/// Public-mode router over `tmp` seeded with an owner token and one invite
/// named "Alice". Returns `(router, owner_token, alice_token, tokens_path)`.
fn public_router(tmp: &tempfile::TempDir) -> (axum::Router, String, String, PathBuf) {
    let mut tf = TokenFile::default();
    let owner = tf.ensure_owner().unwrap();
    let alice = tf.create_invite("Alice", vec!["c1".into()]).unwrap();
    let path = tmp.path().join("tokens.json");
    save_tokens(&path, &tf).unwrap();
    let ctx = AuthCtx::load(&path).unwrap();
    let app = Arc::new(AppState::local_only(tmp.path().to_path_buf()));
    app.refresh();
    (
        crate::hub::build_router_public(app, ctx),
        owner,
        alice.token,
        path,
    )
}

/// Open (local, no-auth) router: the token routes have no store to talk to.
fn open_router(tmp: &tempfile::TempDir) -> axum::Router {
    let app = Arc::new(AppState::local_only(tmp.path().to_path_buf()));
    app.refresh();
    crate::hub::build_router_with_state(app)
}

async fn send(
    router: &axum::Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    body: Option<&str>,
) -> (StatusCode, String) {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", "127.0.0.1")
        .header("x-cryo-csrf", "1");
    if let Some(t) = token {
        req = req.header("authorization", format!("Bearer {t}"));
    }
    let req = match body {
        Some(b) => req
            .header("content-type", "application/json")
            .body(Body::from(b.to_string()))
            .unwrap(),
        None => req.body(Body::empty()).unwrap(),
    };
    let resp = router.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

fn json_of(raw: &str) -> serde_json::Value {
    serde_json::from_str(raw).unwrap_or_else(|e| panic!("not JSON ({e}): {raw}"))
}

#[tokio::test]
async fn whoami_reports_role() {
    let tmp = tempfile::tempdir().unwrap();
    let (router, owner, alice, _path) = public_router(&tmp);

    let (status, body) = send(&router, "GET", "/api/whoami", Some(&owner), None).await;
    assert_eq!(status, StatusCode::OK, "owner whoami: {body}");
    assert_eq!(json_of(&body)["role"], "owner");

    let (status, body) = send(&router, "GET", "/api/whoami", Some(&alice), None).await;
    assert_eq!(status, StatusCode::OK, "invite whoami: {body}");
    let v = json_of(&body);
    assert_eq!(v["role"], "invite");
    assert_eq!(v["name"], "Alice");
    assert_eq!(v["chambers"], serde_json::json!(["c1"]));

    // No token at all: whoami is AnyToken-classified, so the guard 401s.
    let (status, _) = send(&router, "GET", "/api/whoami", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn whoami_in_open_mode_reports_owner() {
    // Loopback-only mode has no token store; the local user is the owner.
    let tmp = tempfile::tempdir().unwrap();
    let router = open_router(&tmp);
    let (status, body) = send(&router, "GET", "/api/whoami", None, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(json_of(&body)["role"], "owner");
}

#[tokio::test]
async fn token_routes_report_unavailable_in_open_mode() {
    let tmp = tempfile::tempdir().unwrap();
    let router = open_router(&tmp);
    let (status, _) = send(&router, "GET", "/api/tokens", None, None).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    let (status, _) = send(
        &router,
        "POST",
        "/api/tokens",
        None,
        Some(r#"{"name":"Bob","chambers":[]}"#),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    let (status, _) = send(&router, "POST", "/api/tokens/Bob/revoke", None, None).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn token_lifecycle_via_api() {
    let tmp = tempfile::tempdir().unwrap();
    let (router, owner, alice, path) = public_router(&tmp);

    // --- create ---------------------------------------------------------
    let (status, body) = send(
        &router,
        "POST",
        "/api/tokens",
        Some(&owner),
        Some(r#"{"name":"Bob","chambers":["c1"]}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create Bob: {body}");
    let v = json_of(&body);
    assert_eq!(v["ok"], true);
    assert_eq!(v["name"], "Bob");
    let bob = v["token"].as_str().expect("token string").to_string();
    assert_eq!(bob.len(), 64, "expected 64 hex chars, got {bob:?}");
    assert!(bob.chars().all(|c| c.is_ascii_hexdigit()), "{bob:?}");
    assert_eq!(v["link_fragment"], format!("#invite={bob}"));

    // Duplicate active name → 400.
    let (status, _) = send(
        &router,
        "POST",
        "/api/tokens",
        Some(&owner),
        Some(r#"{"name":"Bob","chambers":["c1"]}"#),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "duplicate name must 400");

    // --- list -----------------------------------------------------------
    let (status, body) = send(&router, "GET", "/api/tokens", Some(&owner), None).await;
    assert_eq!(status, StatusCode::OK, "list: {body}");
    assert!(body.contains("Alice") && body.contains("Bob"), "{body}");
    assert!(
        !body.contains(&alice) && !body.contains(&bob),
        "list leaked a token string: {body}"
    );
    let invites = json_of(&body)["invites"].clone();
    let invites = invites.as_array().expect("invites array");
    assert_eq!(invites.len(), 2);
    for i in invites {
        assert!(i.get("token").is_none(), "token field present: {i}");
        assert!(i.get("created_at").is_some(), "{i}");
        assert!(i.get("chambers").is_some(), "{i}");
        assert!(i["revoked_at"].is_null(), "{i}");
    }

    // Bob's token works before revocation (proves the negative below is real).
    let (status, _) = send(&router, "GET", "/api/chambers", Some(&bob), None).await;
    assert_eq!(status, StatusCode::OK, "fresh invite should be accepted");

    // --- revoke ---------------------------------------------------------
    let (status, body) = send(
        &router,
        "POST",
        "/api/tokens/Bob/revoke",
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "revoke: {body}");
    assert_eq!(json_of(&body)["ok"], true);

    let (status, _) = send(&router, "GET", "/api/chambers", Some(&bob), None).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "revoked token must stop working"
    );

    let (status, _) = send(
        &router,
        "POST",
        "/api/tokens/Bob/revoke",
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "second revoke has no active invite"
    );

    // --- invites may not touch these routes at all -----------------------
    for (method, uri) in [
        ("GET", "/api/tokens"),
        ("POST", "/api/tokens"),
        ("POST", "/api/tokens/Alice/revoke"),
    ] {
        let payload = (method == "POST" && uri == "/api/tokens")
            .then_some(r#"{"name":"Mallory","chambers":[]}"#);
        let (status, _) = send(&router, method, uri, Some(&alice), payload).await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "invite must be forbidden from {method} {uri}"
        );
    }

    // --- durability ------------------------------------------------------
    let on_disk = load_tokens(&path).unwrap();
    let bob_rec = on_disk
        .invites
        .iter()
        .find(|i| i.name == "Bob")
        .expect("Bob persisted");
    assert!(bob_rec.revoked_at.is_some(), "revocation must reach disk");
    assert_eq!(bob_rec.token, bob, "created token must reach disk");
}
