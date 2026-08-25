use std::sync::{Arc, Mutex, MutexGuard};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use cryochamber::config;
use cryochamber::hub::config::HubConfig;
use cryochamber::hub::{build_router_with_config, discovery, state::AppState};
use tower::ServiceExt;

/// Global lock serializing env-var mutation so parallel tests don't race on
/// `CRYO_NO_SERVICE`. Matches the pattern in `src/unit_tests/sync_common.rs`.
static ENV_LOCK: Mutex<()> = Mutex::new(());

/// RAII guard: sets an env var on construction, removes it on drop — even on
/// panic. Holds the `ENV_LOCK` for its lifetime so only one test mutates the
/// process environment at a time.
struct EnvVarGuard<'a> {
    _lock: MutexGuard<'a, ()>,
    key: &'static str,
}

impl<'a> EnvVarGuard<'a> {
    fn set(key: &'static str, value: &str) -> Self {
        let lock = match ENV_LOCK.lock() {
            Ok(g) => g,
            Err(poison) => poison.into_inner(),
        };
        std::env::set_var(key, value);
        Self { _lock: lock, key }
    }
}

impl Drop for EnvVarGuard<'_> {
    fn drop(&mut self) {
        std::env::remove_var(self.key);
    }
}

/// Build a workspace dir with two chambers. Populate the AppState index
/// *without* calling `registry::list()` so the test is isolated from whatever
/// daemons happen to be running on the developer's or CI machine.
fn setup_app(tmp: &tempfile::TempDir) -> Arc<AppState> {
    for name in ["alpha", "beta"] {
        let d = tmp.path().join(name);
        std::fs::create_dir_all(&d).unwrap();
        let cfg = config::CryoConfig::default();
        config::save_config(&d.join("cryo.toml"), &cfg).unwrap();
    }
    let app = Arc::new(AppState::local_only(tmp.path().to_path_buf()));
    let mut idx = discovery::scan_workspace(tmp.path());
    discovery::populate_runtime(&mut idx);
    *app.chambers.write().unwrap() = idx;
    app.wire_watchers();
    app
}

#[tokio::test]
async fn list_chambers_returns_both() {
    let tmp = tempfile::tempdir().unwrap();
    let app = setup_app(&tmp);
    let router = build_router_with_config(app, HubConfig::default());

    let resp = router
        .oneshot(
            Request::builder()
                .uri("/api/chambers")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
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

    let router = build_router_with_config(app, HubConfig::default());
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

    let alpha_dir = tmp.path().join("alpha").canonicalize().unwrap();
    let msgs = cryochamber::message::read_inbox(&alpha_dir).unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].1.body, "hello alpha");

    let beta_dir = tmp.path().join("beta").canonicalize().unwrap();
    let beta_msgs = cryochamber::message::read_inbox(&beta_dir).unwrap();
    assert_eq!(beta_msgs.len(), 0);
}

#[tokio::test]
async fn unknown_chamber_id_returns_404() {
    let tmp = tempfile::tempdir().unwrap();
    let app = setup_app(&tmp);
    let router = build_router_with_config(app, HubConfig::default());

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
    // Guard the env mutation so it doesn't bleed into other tests when the
    // harness runs them in parallel, and is restored even if this test panics.
    let _env = EnvVarGuard::set("CRYO_NO_SERVICE", "1");

    let tmp = tempfile::tempdir().unwrap();
    let alpha = tmp.path().join("alpha");
    std::fs::create_dir_all(&alpha).unwrap();
    let cfg = config::CryoConfig {
        agent: "true".into(),
        ..Default::default()
    };
    config::save_config(&alpha.join("cryo.toml"), &cfg).unwrap();
    std::fs::write(alpha.join("plan.md"), "test plan").unwrap();

    let app = Arc::new(AppState::local_only(tmp.path().to_path_buf()));
    let mut idx = discovery::scan_workspace(tmp.path());
    discovery::populate_runtime(&mut idx);
    *app.chambers.write().unwrap() = idx;
    app.wire_watchers();
    let id = {
        let idx = app.chambers.read().unwrap();
        idx.values().find(|e| e.name == "alpha").unwrap().id.clone()
    };

    let router = build_router_with_config(app.clone(), HubConfig::default());
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
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
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
    let _ = cryochamber::hub::lifecycle::stop_chamber(&alpha.canonicalize().unwrap());
}

#[tokio::test]
async fn create_and_start_chamber_via_api_creates_background_daemon() {
    let _env = EnvVarGuard::set("CRYO_NO_SERVICE", "1");

    let tmp = tempfile::tempdir().unwrap();
    let app = Arc::new(AppState::local_only(tmp.path().to_path_buf()));
    app.wire_watchers();
    let hub_config = HubConfig {
        default_agent: "true".into(),
        ..Default::default()
    };
    let router = build_router_with_config(app, hub_config);

    let body = serde_json::json!({"name": "alpha", "start": true}).to_string();
    let resp = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/chambers/new")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        value["started"], true,
        "create-and-start should succeed: {value:?}"
    );
    assert!(value["start_error"].is_null());
    assert!(!value["id"].as_str().unwrap_or_default().is_empty());

    let alpha = tmp.path().join("alpha").canonicalize().unwrap();
    let state_path = cryochamber::state::state_path(&alpha);
    for _ in 0..30 {
        if state_path.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(state_path.exists(), "daemon should have written timer.json");

    let _ = cryochamber::hub::lifecycle::stop_chamber(&alpha);
}
