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
