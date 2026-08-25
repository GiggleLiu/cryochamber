//! Host-level Cryohub configuration exposed to the owner console.

use std::sync::Arc;

use axum::{extract::State, http::StatusCode, response::Json};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::hub::state::AppState;

#[derive(Debug, Serialize)]
pub struct HostConfigResponse {
    pub default_agent: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateHostConfig {
    pub default_agent: String,
}

pub async fn get_config(State(app): State<Arc<AppState>>) -> Json<HostConfigResponse> {
    let default_agent = app
        .default_agent
        .read()
        .map(|agent| agent.clone())
        .unwrap_or_else(|_| crate::config::default_agent());
    Json(HostConfigResponse { default_agent })
}

pub async fn post_config(
    State(app): State<Arc<AppState>>,
    Json(payload): Json<UpdateHostConfig>,
) -> (StatusCode, Json<Value>) {
    let default_agent = payload.default_agent.trim();
    if default_agent.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "default agent is empty" })),
        );
    }
    if let Err(error) = crate::hub::lifecycle::validate_agent_command(default_agent) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": format!("invalid default agent: {error}") })),
        );
    }

    let mut config = match crate::hub::config::load_config() {
        Ok(config) => config,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": error.to_string() })),
            )
        }
    };
    config.default_agent = default_agent.to_string();
    if let Err(error) = crate::hub::config::save_config(&config) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": error.to_string() })),
        );
    }
    if let Ok(mut active) = app.default_agent.write() {
        *active = config.default_agent.clone();
    }

    (
        StatusCode::OK,
        Json(json!({ "default_agent": config.default_agent })),
    )
}

#[cfg(test)]
#[path = "../../unit_tests/hub/routes/config.rs"]
mod tests;
