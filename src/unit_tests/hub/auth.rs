use crate::hub::auth::{classify, Access};
use axum::http::Method;

#[test]
fn classification_matches_spec_matrix() {
    use Access::*;
    let cases = [
        (Method::GET, "/", Public),
        (Method::GET, "/c/abc", Public),
        (Method::GET, "/assets/index-abc123.js", Public),
        (Method::GET, "/api/chambers", AnyToken),
        (Method::GET, "/api/events", AnyToken),
        (Method::GET, "/api/whoami", AnyToken),
        (
            Method::GET,
            "/api/chambers/x1/messages",
            Chamber("x1".into()),
        ),
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
        // Working state, not conversation: owner-only even in scope.
        (Method::GET, "/api/chambers/x1/status", OwnerOnly),
        (Method::GET, "/api/chambers/x1/todos", OwnerOnly),
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
use crate::hub::tokens::{save_tokens, Role, TokenFile};
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
    // An empty console root, so page routes answer the same way on every
    // machine: the default would resolve to whatever the developer running
    // these tests happens to have installed in ~/.cryo/console.
    let config = crate::hub::config::HubConfig {
        console_dir: Some(tmp.path().join("console")),
        ..crate::hub::config::HubConfig::default()
    };
    let router = crate::hub::build_router_with_config(app.clone(), config);
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
    // Pages stay public: the guard hands `/` straight to the console fallback
    // instead of 401ing it. No console is installed under this temp root, so
    // that fallback answers with the setup page — which is the point, since an
    // auth failure would be 401/403 no matter what is installed.
    assert_eq!(
        status_for(&router, "GET", "/", None).await,
        StatusCode::SERVICE_UNAVAILABLE
    );
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

#[test]
fn auth_ctx_picks_up_out_of_band_token_file_edits() {
    // `cryohub token create/revoke` runs in a *separate process* and only
    // rewrites the store file. A running server has to see those edits without
    // a restart, otherwise CLI revocation is not the immediate kill switch the
    // CLI reference promises, and CLI-minted invites are unusable until the
    // next restart.
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("tokens.json");
    let mut tf = TokenFile::default();
    tf.ensure_owner().unwrap();
    save_tokens(&path, &tf).unwrap();
    let ctx = AuthCtx::load(&path).unwrap();

    // Created out of band → live immediately.
    let bob = tf.create_invite("Bob", vec!["c1".into()]).unwrap().token;
    save_tokens(&path, &tf).unwrap();
    assert!(
        matches!(ctx.resolve(&bob), Some(Role::Invite { .. })),
        "an invite created by the CLI must resolve without a server restart"
    );

    // Revoked out of band → dead immediately.
    assert!(tf.revoke("Bob"));
    save_tokens(&path, &tf).unwrap();
    assert_eq!(
        ctx.resolve(&bob),
        None,
        "an invite revoked by the CLI must stop resolving without a restart"
    );
}

/// Make `dir` unwritable and report whether that actually took effect. Running
/// as root ignores the mode bits, in which case the caller must skip.
#[cfg(unix)]
fn make_unwritable(dir: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o500)).unwrap();
    let probe = dir.join(".write-probe");
    match std::fs::write(&probe, b"x") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            false
        }
        Err(_) => true,
    }
}

#[cfg(unix)]
fn make_writable(dir: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
#[cfg(unix)]
fn a_mutation_that_cannot_be_persisted_does_not_take_effect() {
    // Mutation and persistence are one transaction: the change is published in
    // memory only after it reaches disk. Otherwise a failed revoke returns 500,
    // 404s on retry ("already revoked"), and the token comes back after the
    // next restart because the tombstone never landed.
    let tmp = tempfile::tempdir().unwrap();
    let store_dir = tmp.path().join("store");
    std::fs::create_dir(&store_dir).unwrap();
    let path = store_dir.join("tokens.json");

    let mut tf = TokenFile::default();
    tf.ensure_owner().unwrap();
    let alice = tf.create_invite("Alice", vec!["c1".into()]).unwrap().token;
    save_tokens(&path, &tf).unwrap();
    let ctx = AuthCtx::load(&path).unwrap();

    if !make_unwritable(&store_dir) {
        return; // running as root: the mode bits prove nothing
    }
    let outcome = ctx.mutate(|store| {
        anyhow::ensure!(store.revoke("Alice"), "no active invite named 'Alice'");
        Ok(())
    });
    assert!(
        matches!(outcome, Err(crate::hub::auth::MutateError::Persist(_))),
        "an unpersistable mutation must report a persistence failure"
    );
    assert!(
        matches!(ctx.resolve(&alice), Some(Role::Invite { .. })),
        "a mutation that could not be persisted must not take effect in memory"
    );

    // Control: once the store is writable again the very same call succeeds,
    // so the failure above was the save and not a broken revoke.
    make_writable(&store_dir);
    assert!(ctx
        .mutate(|store| {
            anyhow::ensure!(store.revoke("Alice"), "no active invite named 'Alice'");
            Ok(())
        })
        .is_ok());
    assert_eq!(ctx.resolve(&alice), None);
    assert!(
        crate::hub::tokens::load_tokens(&path).unwrap().invites[0]
            .revoked_at
            .is_some(),
        "the successful revoke must reach disk"
    );
}

#[tokio::test]
async fn guard_leaves_non_api_paths_public_even_when_they_start_with_api() {
    let tmp = tempfile::tempdir().unwrap();
    let (router, _owner, _invite) = public_router(&tmp);
    // `/apiary` is not under `/api/`: it is an ordinary page, so the guard must
    // let it through to the console fallback rather than 401 it. With no
    // console installed here, the fallback's answer is the setup page — the
    // point being that auth never entered into it.
    assert_eq!(
        status_for(&router, "GET", "/apiary", None).await,
        StatusCode::SERVICE_UNAVAILABLE
    );
    // `/api` itself stays guarded.
    assert_eq!(
        status_for(&router, "GET", "/api", None).await,
        StatusCode::UNAUTHORIZED
    );
}
