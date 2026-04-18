pub mod discovery;
pub mod state;
pub mod watchers;
pub mod lifecycle;
pub mod routes;

pub use state::{AppState, SseEvent};

use std::path::PathBuf;

/// Placeholder: the real router is built in Task 14. Present now so CLI and
/// tests can already import `serve` / `build_router` with a stable signature.
pub fn build_router(workspace_dir: PathBuf) -> axum::Router {
    let _ = workspace_dir;
    axum::Router::new()
}

pub async fn serve(workspace_dir: PathBuf, host: &str, port: u16) -> anyhow::Result<()> {
    crate::message::ensure_dirs(&workspace_dir)?;
    let app = build_router(workspace_dir);
    let addr = format!("{host}:{port}");
    println!("Cryochamber web UI: http://{addr}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
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
