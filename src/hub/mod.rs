pub mod auth;
pub mod config;
pub mod discovery;
pub mod lifecycle;
pub mod mime;
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
///
/// Reads the hub config once. The security layer needs the bind host and any
/// reverse-proxy hostnames; `post_send` needs the owner sender name. Falling
/// back to defaults if the config is unreadable is safe — the default bind
/// is loopback and the default owner name is `human`.
pub fn build_router_with_state(app: Arc<WebAppState>) -> Router {
    build_router_with_config(app, crate::hub::config::load_config().unwrap_or_default())
}

/// As [`build_router_with_state`], with the config supplied rather than read
/// from disk, so tests can exercise a configuration without installing one on
/// the machine running them.
pub fn build_router_with_config(
    app: Arc<WebAppState>,
    config: crate::hub::config::HubConfig,
) -> Router {
    let mut configured_hosts = vec![config.host.clone()];
    configured_hosts.extend(config.public_hosts.iter().cloned());
    let router = Router::new();
    // With a console configured, the chat UI owns `/` (it is what invite
    // links open) and the bundled control panel moves to `/admin` — both from
    // one hub, not either-or. The panel's exact asset paths stay registered:
    // they shadow the console fallback for just those three names, which a
    // Vite build can never emit (its bundles are content-hashed, its PWA
    // icons live under `/icons/`). Without a console, the panel keeps the
    // root as it always has.
    let pages = |router: Router<Arc<WebAppState>>| {
        router
            .route("/c/{id}", get(crate::hub::routes::pages::get_index))
            .route("/assets/web.css", get(crate::hub::routes::pages::get_css))
            .route("/assets/logo.svg", get(crate::hub::routes::pages::get_logo))
            .route("/assets/mark.svg", get(crate::hub::routes::pages::get_mark))
    };
    let router = match &config.console_dir {
        Some(_) => pages(router).route("/admin", get(crate::hub::routes::pages::get_index)),
        None => pages(router).route("/", get(crate::hub::routes::pages::get_index)),
    };
    let router = router
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
        .route(
            "/api/chambers/{id}/uploads",
            post(crate::hub::routes::files::post_upload),
        )
        .route(
            "/api/chambers/{id}/files/{name}",
            get(crate::hub::routes::files::get_file),
        )
        .route("/api/events", get(crate::hub::routes::events::get_events))
        .route(
            "/api/whoami",
            get(crate::hub::routes::tokens_api::get_whoami),
        )
        .route(
            "/api/tokens",
            get(crate::hub::routes::tokens_api::get_tokens)
                .post(crate::hub::routes::tokens_api::post_token),
        )
        .route(
            "/api/tokens/{name}/revoke",
            post(crate::hub::routes::tokens_api::post_revoke),
        )
        .with_state(app);
    // The console fallback needs no `AppState`, so it is attached after
    // `with_state` and only ever sees paths no hub route claimed.
    let router = match config.console_dir {
        Some(dir) => {
            router.fallback(move |req| crate::hub::routes::console::serve(dir.clone(), req))
        }
        None => router,
    };
    let router = router
        // Bound the buffered body so the 25 MB attachment cap binds before an
        // unbounded upload is read into memory. The slack covers multipart
        // framing overhead; the exact cap is enforced in `post_upload`.
        .layer(axum::extract::DefaultBodyLimit::max(
            crate::hub::routes::files::MAX_ATTACHMENT_BYTES + 1024 * 1024,
        ))
        .layer(axum::Extension(crate::hub::config::OwnerName(
            config.owner_name,
        )));
    crate::hub::security::apply(router, configured_hosts)
}

/// Public-mode router: same routes, wrapped in the bearer-token guard.
///
/// The `Extension(ctx)` layer sits *inside* the guard, so by the time a handler
/// runs the request carries both the resolved `Role` (inserted by the guard)
/// and the live token store. Open (loopback) mode builds the router without it,
/// which is how the token-management handlers know to answer 503.
pub fn build_router_public(app: Arc<WebAppState>, ctx: Arc<crate::hub::auth::AuthCtx>) -> Router {
    build_router_public_with_config(
        app,
        ctx,
        crate::hub::config::load_config().unwrap_or_default(),
    )
}

/// As [`build_router_public`], with the config supplied rather than read from
/// disk. See [`build_router_with_config`].
pub fn build_router_public_with_config(
    app: Arc<WebAppState>,
    ctx: Arc<crate::hub::auth::AuthCtx>,
    config: crate::hub::config::HubConfig,
) -> Router {
    let router = build_router_with_config(app.clone(), config).layer(axum::Extension(ctx.clone()));
    crate::hub::auth::apply_auth(router, app, ctx)
}

/// Refuse public mode unless an owner token exists.
///
/// A public hub without one could never be administered — no invites, no
/// revocation — so starting is worse than not starting. Both entry points
/// check it: `serve` before binding a socket, and `cryohub start` before
/// *installing a service*, since a KeepAlive unit that fails on boot would
/// otherwise restart forever.
pub fn require_owner_token() -> anyhow::Result<()> {
    let path = crate::hub::tokens::default_tokens_path();
    if crate::hub::tokens::load_tokens(&path)?.owner.is_none() {
        anyhow::bail!(
            "public mode requires an owner token — run `cryohub token owner` first \
             (store: {})",
            path.display()
        );
    }
    Ok(())
}

/// Serve the hub. In `public` mode every `/api` route is behind the bearer
/// guard; otherwise the hub is the loopback-only dashboard it has always been.
///
/// The owner-token precondition is checked *first*, before any workspace scan
/// or socket bind: a public hub without an owner token could never be
/// administered, and failing after the port is open would be worse than not
/// starting at all.
pub async fn serve(host: &str, port: u16, public: bool) -> anyhow::Result<()> {
    let ctx = if public {
        require_owner_token()?;
        Some(crate::hub::auth::AuthCtx::load(
            &crate::hub::tokens::default_tokens_path(),
        )?)
    } else {
        None
    };

    let app = Arc::new(WebAppState::global());
    app.refresh();
    let router = match ctx {
        Some(ctx) => {
            println!("Cryochamber hub: PUBLIC mode (bearer auth enforced)");
            build_router_public(app, ctx)
        }
        None => build_router_with_state(app),
    };
    let addr = format!("{host}:{port}");
    // The warning is about binding a non-loopback interface *unauthenticated*.
    // In public mode the bearer guard is exactly the fix it recommends, so
    // repeating it there would train operators to ignore it.
    if !public && !host.starts_with("127.") && host != "localhost" {
        eprintln!(
            "Warning: cryohub is binding on {host} — lifecycle actions (start/stop/restart) are exposed without auth. Use 127.0.0.1, or start with --public, unless you know what you're doing."
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
