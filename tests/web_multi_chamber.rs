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

#[tokio::test]
async fn start_chamber_via_api_creates_background_daemon() {
    // Force the background-process launch path so no service install happens.
    std::env::set_var("CRYO_NO_SERVICE", "1");

    let tmp = tempfile::tempdir().unwrap();
    let chambers = tmp.path().join("chambers");
    let alpha = chambers.join("alpha");
    std::fs::create_dir_all(&alpha).unwrap();
    let cfg = config::CryoConfig {
        agent: "true".into(),
        ..Default::default()
    };
    config::save_config(&alpha.join("cryo.toml"), &cfg).unwrap();
    std::fs::write(alpha.join("plan.md"), "test plan").unwrap();

    let app = Arc::new(AppState::new(tmp.path().to_path_buf()));
    let mut idx = discovery::scan_workspace(tmp.path());
    discovery::populate_runtime(&mut idx);
    *app.chambers.write().unwrap() = idx;
    let id = {
        let idx = app.chambers.read().unwrap();
        idx.values().find(|e| e.name == "alpha").unwrap().id.clone()
    };

    let router = build_router_with_state(app.clone());
    let resp = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/chambers/{id}/start"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["ok"], true, "start should succeed: {v:?}");

    // Daemon writes timer.json fairly quickly. Poll briefly, then assert.
    let state_path = cryochamber::state::state_path(&alpha.canonicalize().unwrap());
    for _ in 0..30 {
        if state_path.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(state_path.exists(), "daemon should have written timer.json");

    // Clean up: stop the daemon we spawned
    let _ = cryochamber::web::lifecycle::stop_chamber(&alpha.canonicalize().unwrap());
}
