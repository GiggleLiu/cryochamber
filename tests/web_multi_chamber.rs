use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use cryochamber::config;
use cryochamber::web::{build_router_with_state, discovery, state::AppState};
use tower::ServiceExt;

/// Build a workspace with two chambers. Populate the AppState index
/// *without* calling `registry::list()` so the test is isolated from whatever
/// daemons happen to be running on the developer's or CI machine.
fn setup_app(tmp: &tempfile::TempDir) -> Arc<AppState> {
    let chambers = tmp.path().join("chambers");
    for name in ["alpha", "beta"] {
        let d = chambers.join(name);
        std::fs::create_dir_all(&d).unwrap();
        let cfg = config::CryoConfig::default();
        config::save_config(&d.join("cryo.toml"), &cfg).unwrap();
    }
    let app = Arc::new(AppState::new(tmp.path().to_path_buf()));
    let mut idx = discovery::scan_workspace(tmp.path());
    discovery::populate_runtime(&mut idx);
    *app.chambers.write().unwrap() = idx;
    app
}

#[tokio::test]
async fn list_chambers_returns_both() {
    let tmp = tempfile::tempdir().unwrap();
    let app = setup_app(&tmp);
    let router = build_router_with_state(app);

    let resp = router
        .oneshot(Request::builder().uri("/api/chambers").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let list: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(list.as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn send_message_writes_to_correct_chamber() {
    let tmp = tempfile::tempdir().unwrap();
    let app = setup_app(&tmp);

    let id = {
        let idx = app.chambers.read().unwrap();
        idx.values().find(|e| e.name == "alpha").unwrap().id.clone()
    };

    let router = build_router_with_state(app);
    let body = serde_json::json!({"body": "hello alpha"}).to_string();
    let resp = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/chambers/{id}/send"))
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let alpha_dir = tmp.path().join("chambers").join("alpha").canonicalize().unwrap();
    let msgs = cryochamber::message::read_inbox(&alpha_dir).unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].1.body, "hello alpha");

    let beta_dir = tmp.path().join("chambers").join("beta").canonicalize().unwrap();
    let beta_msgs = cryochamber::message::read_inbox(&beta_dir).unwrap();
    assert_eq!(beta_msgs.len(), 0);
}

#[tokio::test]
async fn unknown_chamber_id_returns_404() {
    let tmp = tempfile::tempdir().unwrap();
    let app = setup_app(&tmp);
    let router = build_router_with_state(app);

    let resp = router
        .oneshot(
            Request::builder()
                .uri("/api/chambers/nonexistent/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
