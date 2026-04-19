pub mod discovery;
pub mod lifecycle;
pub mod routes;
pub mod state;
pub mod watchers;

pub use state::{AppState, SseEvent};

use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    routing::{get, post},
    Router,
};

use crate::hub::state::AppState as WebAppState;

pub fn build_router(workspace_dir: PathBuf) -> Router {
    let app = Arc::new(WebAppState::new(workspace_dir));
    app.refresh();
    build_router_with_state(app)
}

/// Separate entry point so integration tests can inject their own `AppState`.
pub fn build_router_with_state(app: Arc<WebAppState>) -> Router {
    Router::new()
        .route("/", get(crate::hub::routes::pages::get_index))
        .route("/c/{id}", get(crate::hub::routes::pages::get_index))
        .route("/assets/web.css", get(crate::hub::routes::pages::get_css))
        .route(
            "/api/chambers",
            get(crate::hub::routes::chambers::get_chambers),
        )
        .route(
            "/api/chambers/refresh",
            post(crate::hub::routes::chambers::post_refresh),
        )
        .route(
            "/api/chambers/{id}/status",
            get(crate::hub::routes::chamber::get_status),
        )
        .route(
            "/api/chambers/{id}/messages",
            get(crate::hub::routes::chamber::get_messages),
        )
        .route(
            "/api/chambers/{id}/todos",
            get(crate::hub::routes::chamber::get_todos),
        )
        .route(
            "/api/chambers/{id}/send",
            post(crate::hub::routes::chamber::post_send),
        )
        .route(
            "/api/chambers/{id}/wake",
            post(crate::hub::routes::chamber::post_wake),
        )
        .route(
            "/api/chambers/{id}/start",
            post(crate::hub::routes::chamber::post_start),
        )
        .route(
            "/api/chambers/{id}/stop",
            post(crate::hub::routes::chamber::post_stop),
        )
        .route(
            "/api/chambers/{id}/restart",
            post(crate::hub::routes::chamber::post_restart),
        )
        .route(
            "/api/chambers/{id}/reset",
            post(crate::hub::routes::chamber::post_reset),
        )
        .route(
            "/api/chambers/{id}/sync",
            get(crate::hub::routes::sync::get_sync),
        )
        .route(
            "/api/chambers/{id}/sync/{backend}/{verb}",
            post(crate::hub::routes::sync::post_sync_action),
        )
        .route("/api/events", get(crate::hub::routes::events::get_events))
        .with_state(app)
}

pub async fn serve(workspace_dir: PathBuf, host: &str, port: u16) -> anyhow::Result<()> {
    let app = Arc::new(WebAppState::new(workspace_dir));
    app.refresh();
    let router = build_router_with_state(app);
    let addr = format!("{host}:{port}");
    if !host.starts_with("127.") && host != "localhost" {
        eprintln!(
            "Warning: cryohub is binding on {host} — lifecycle actions (start/stop/restart) are exposed without auth. Use 127.0.0.1 unless you know what you're doing."
        );
    }
    println!("Cryochamber hub: http://{addr}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, router).await?;
    Ok(())
}

/// Format a duration in milliseconds as a human-readable relative string.
/// Negative or zero values mean the time has passed.
pub fn format_relative_time(diff_ms: i64) -> String {
    if diff_ms <= 0 {
        return "now".to_string();
    }
    let mins = diff_ms / 60_000;
    let hours = diff_ms / 3_600_000;
    let days = diff_ms / 86_400_000;
    if mins < 1 {
        "<1m".to_string()
    } else if hours < 1 {
        format!("{mins}m")
    } else if days < 1 {
        let rem_m = (diff_ms % 3_600_000) / 60_000;
        format!("{hours}h {rem_m}m")
    } else {
        let rem_h = (diff_ms % 86_400_000) / 3_600_000;
        format!("{days}d {rem_h}h")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_relative_time_basic() {
        assert_eq!(format_relative_time(0), "now");
        assert_eq!(format_relative_time(-5000), "now");
        assert_eq!(format_relative_time(30_000), "<1m");
        assert_eq!(format_relative_time(60_000), "1m");
        assert_eq!(format_relative_time(3_600_000), "1h 0m");
        assert_eq!(format_relative_time(86_400_000), "1d 0h");
    }
}
