pub mod auth;
pub mod config;
pub mod discovery;
pub mod lifecycle;
pub mod paths;
pub mod routes;
pub mod security;
pub mod state;
pub mod tokens;
pub mod watchers;

pub use state::{AppState, SseEvent};

use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    routing::{get, post},
    Router,
};

use crate::hub::state::AppState as WebAppState;

pub fn build_router() -> Router {
    let app = Arc::new(WebAppState::global());
    app.refresh();
    build_router_with_state(app)
}

pub fn build_router_local_only(workspace_dir: PathBuf) -> Router {
    let app = Arc::new(WebAppState::local_only(workspace_dir));
    app.refresh();
    build_router_with_state(app)
}

/// Separate entry point so integration tests can inject their own `AppState`.
pub fn build_router_with_state(app: Arc<WebAppState>) -> Router {
    // Read the configured bind host once (loopback by default) so the security
    // layer can allow it in addition to loopback. Falling back to loopback-only
    // if the hub config is unreadable is safe — the default bind is loopback.
    let configured_host = crate::hub::config::load_config().ok().map(|cfg| cfg.host);
    let router = Router::new()
        .route("/", get(crate::hub::routes::pages::get_index))
        .route("/c/{id}", get(crate::hub::routes::pages::get_index))
        .route("/assets/web.css", get(crate::hub::routes::pages::get_css))
        .route("/assets/logo.svg", get(crate::hub::routes::pages::get_logo))
        .route("/assets/mark.svg", get(crate::hub::routes::pages::get_mark))
        .route(
            "/api/chambers",
            get(crate::hub::routes::chambers::get_chambers),
        )
        .route(
            "/api/chambers/refresh",
            post(crate::hub::routes::chambers::post_refresh),
        )
        .route(
            "/api/chambers/new",
            post(crate::hub::routes::chambers::post_new),
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
            "/api/chambers/{id}/archive",
            post(crate::hub::routes::chamber::post_archive),
        )
        .route(
            "/api/chambers/{id}/unarchive",
            post(crate::hub::routes::chamber::post_unarchive),
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
        .with_state(app);
    crate::hub::security::apply(router, configured_host)
}

/// Public-mode router: same routes, wrapped in the bearer-token guard.
pub fn build_router_public(app: Arc<WebAppState>, ctx: Arc<crate::hub::auth::AuthCtx>) -> Router {
    let router = build_router_with_state(app.clone());
    crate::hub::auth::apply_auth(router, app, ctx)
}

pub async fn serve(host: &str, port: u16) -> anyhow::Result<()> {
    let app = Arc::new(WebAppState::global());
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
    match classify_relative_time(diff_ms) {
        RelativeTimeDisplay::Now => "now".to_string(),
        RelativeTimeDisplay::LessThanMinute => "<1m".to_string(),
        RelativeTimeDisplay::Minutes(minutes) => format!("{minutes}m"),
        RelativeTimeDisplay::HoursMinutes { hours, minutes } => format!("{hours}h {minutes}m"),
        RelativeTimeDisplay::DaysHours { days, hours } => format!("{days}d {hours}h"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RelativeTimeDisplay {
    Now,
    LessThanMinute,
    Minutes(i64),
    HoursMinutes { hours: i64, minutes: i64 },
    DaysHours { days: i64, hours: i64 },
}

fn classify_relative_time(diff_ms: i64) -> RelativeTimeDisplay {
    match diff_ms {
        ..=0 => RelativeTimeDisplay::Now,
        1..=59_999 => RelativeTimeDisplay::LessThanMinute,
        60_000..=3_599_999 => RelativeTimeDisplay::Minutes(diff_ms / 60_000),
        3_600_000..=86_399_999 => RelativeTimeDisplay::HoursMinutes {
            hours: diff_ms / 3_600_000,
            minutes: (diff_ms % 3_600_000) / 60_000,
        },
        _ => RelativeTimeDisplay::DaysHours {
            days: diff_ms / 86_400_000,
            hours: (diff_ms % 86_400_000) / 3_600_000,
        },
    }
}

#[cfg(test)]
#[path = "../unit_tests/hub/mod.rs"]
mod tests;
