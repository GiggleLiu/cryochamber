use crate::hub::auth::{classify, Access};
use axum::http::Method;

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
        (
            Method::GET,
            "/api/chambers/x1/messages",
            Chamber("x1".into()),
        ),
        (Method::GET, "/api/chambers/x1/status", Chamber("x1".into())),
        (Method::GET, "/api/chambers/x1/todos", Chamber("x1".into())),
        (Method::POST, "/api/chambers/x1/send", Chamber("x1".into())),
        (
            Method::POST,
            "/api/chambers/x1/uploads",
            Chamber("x1".into()),
        ),
        (
            Method::GET,
            "/api/chambers/x1/files/a.pdf",
            Chamber("x1".into()),
        ),
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

#[test]
fn api_prefix_matches_on_segment_boundary_only() {
    use Access::*;
    // `/api` and anything under `/api/` is guarded; a path that merely *starts
    // with* the letters "api" is an ordinary public page.
    let cases = [
        (Method::GET, "/api", OwnerOnly),
        (Method::GET, "/api/", OwnerOnly),
        (Method::GET, "/apiary", Public),
        (Method::GET, "/apiary/chambers", Public),
        (Method::GET, "/api-v2/chambers", Public),
        (Method::POST, "/apiary", Public),
    ];
    for (method, path, want) in cases {
        assert_eq!(classify(&method, path), want, "{method} {path}");
    }
}

use crate::hub::auth::{apply_auth, AuthCtx};
use crate::hub::state::AppState;
use crate::hub::tokens::{save_tokens, TokenFile};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use std::sync::Arc;
use tower::ServiceExt;

/// Build a public-mode router over `tmp` whose single invite is scoped to
/// `scope`. Returns `(router, owner_token, invite_token)`.
fn router_with_scope(
    tmp: &tempfile::TempDir,
    scope: Vec<String>,
) -> (axum::Router, String, String) {
    let mut tf = TokenFile::default();
    let owner = tf.ensure_owner().unwrap();
    let invite = tf.create_invite("Alice", scope).unwrap();
    let path = tmp.path().join("tokens.json");
    save_tokens(&path, &tf).unwrap();
    let ctx = AuthCtx::load(&path).unwrap();
    let app = Arc::new(AppState::local_only(tmp.path().to_path_buf()));
    app.refresh();
    let router = crate::hub::build_router_with_state(app.clone());
    (apply_auth(router, app, ctx), owner, invite.token)
}

fn public_router(tmp: &tempfile::TempDir) -> (axum::Router, String, String) {
    router_with_scope(tmp, vec!["scoped-id".into()])
}

/// Create a discoverable chamber under `tmp` and return its index id (the
/// percent-encoded absolute path that the hub routes on).
fn chamber_id(tmp: &tempfile::TempDir, name: &str) -> String {
    let dir = tmp.path().join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let cfg = crate::config::CryoConfig::default();
    crate::config::save_config(&dir.join("cryo.toml"), &cfg).unwrap();
    crate::hub::discovery::encode_id(&dir.canonicalize().unwrap())
}

async fn status_for(
    router: &axum::Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
) -> StatusCode {
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
    assert_eq!(
        status_for(&router, "GET", "/api/chambers", None).await,
        StatusCode::UNAUTHORIZED
    );
    // bad token → 401
    assert_eq!(
        status_for(&router, "GET", "/api/chambers", Some("ffff")).await,
        StatusCode::UNAUTHORIZED
    );
    // invite on owner-only → 403
    assert_eq!(
        status_for(&router, "POST", "/api/chambers/refresh", Some(&invite)).await,
        StatusCode::FORBIDDEN
    );
    // invite on out-of-scope chamber → 404 (never 403)
    assert_eq!(
        status_for(
            &router,
            "GET",
            "/api/chambers/other-id/messages",
            Some(&invite)
        )
        .await,
        StatusCode::NOT_FOUND
    );
    // owner passes the guard (404 here only because the chamber doesn't exist)
    assert_eq!(
        status_for(
            &router,
            "GET",
            "/api/chambers/other-id/messages",
            Some(&owner)
        )
        .await,
        StatusCode::NOT_FOUND
    );
    // static pages stay public
    assert_eq!(status_for(&router, "GET", "/", None).await, StatusCode::OK);
}

#[tokio::test]
async fn guard_lets_an_in_scope_invite_reach_a_real_chamber_route() {
    // Regression: the guard must not 404 a chamber the invite legitimately
    // owns. The scope stores the id in the same (encoded) form the index uses.
    let tmp = tempfile::tempdir().unwrap();
    let id = chamber_id(&tmp, "alpha");
    let (router, _owner, invite) = router_with_scope(&tmp, vec![id.clone()]);

    let status = status_for(
        &router,
        "GET",
        &format!("/api/chambers/{id}/messages"),
        Some(&invite),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "in-scope invite should be served");
}

#[tokio::test]
async fn guard_matches_a_scope_stored_in_decoded_path_form() {
    // Regression: a percent-encoded id in the URL must be decoded once before
    // the scope comparison, otherwise a scope holding the plain path form
    // (`/tmp/alpha`) never matches and the invite gets a bogus 404.
    let tmp = tempfile::tempdir().unwrap();
    let id = chamber_id(&tmp, "alpha");
    let decoded = crate::hub::discovery::decode_id(&id)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert_ne!(decoded, id, "test needs an id that actually gets encoded");
    let (router, _owner, invite) = router_with_scope(&tmp, vec![decoded]);

    let status = status_for(
        &router,
        "GET",
        &format!("/api/chambers/{id}/messages"),
        Some(&invite),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "decoded-form scope should cover the encoded request id"
    );
}

#[tokio::test]
async fn guard_leaves_non_api_paths_public_even_when_they_start_with_api() {
    let tmp = tempfile::tempdir().unwrap();
    let (router, _owner, _invite) = public_router(&tmp);
    // `/apiary` is not under `/api/`: it is an ordinary (unknown) page, so the
    // guard must let it through to the router — which 404s it, not 401s it.
    assert_eq!(
        status_for(&router, "GET", "/apiary", None).await,
        StatusCode::NOT_FOUND
    );
    // `/api` itself stays guarded.
    assert_eq!(
        status_for(&router, "GET", "/api", None).await,
        StatusCode::UNAUTHORIZED
    );
}
