use std::sync::Arc;

use axum::{extract::State, response::Json};

use super::{get_config, post_config, UpdateHostConfig};
use crate::hub::state::AppState;

#[tokio::test]
async fn host_default_agent_round_trips_to_disk_and_live_state() {
    let config_home = tempfile::tempdir().unwrap();
    let _config = crate::test_support::EnvVarGuard::set_path("XDG_CONFIG_HOME", config_home.path());
    let workspace = tempfile::tempdir().unwrap();
    let app = Arc::new(AppState::local_only(workspace.path().to_path_buf()));

    let (status, Json(body)) = post_config(
        State(app.clone()),
        Json(UpdateHostConfig {
            default_agent: "pi --thinking high".to_string(),
        }),
    )
    .await;

    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(body["default_agent"], "pi --thinking high");
    assert_eq!(
        get_config(State(app)).await.0.default_agent,
        "pi --thinking high"
    );
    assert_eq!(
        crate::hub::config::load_config().unwrap().default_agent,
        "pi --thinking high"
    );
}

#[tokio::test]
async fn host_default_agent_rejects_empty_or_invalid_commands() {
    let config_home = tempfile::tempdir().unwrap();
    let _config = crate::test_support::EnvVarGuard::set_path("XDG_CONFIG_HOME", config_home.path());
    let workspace = tempfile::tempdir().unwrap();
    let app = Arc::new(AppState::local_only(workspace.path().to_path_buf()));

    for value in ["   ", "'unterminated"] {
        let (status, _) = post_config(
            State(app.clone()),
            Json(UpdateHostConfig {
                default_agent: value.to_string(),
            }),
        )
        .await;
        assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
    }
    assert!(!crate::hub::paths::hub_config_path().exists());
}
