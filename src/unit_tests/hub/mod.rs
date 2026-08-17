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

#[test]
fn test_classify_relative_time_preserves_unit_boundaries() {
    assert_eq!(classify_relative_time(0), RelativeTimeDisplay::Now);
    assert_eq!(
        classify_relative_time(59_999),
        RelativeTimeDisplay::LessThanMinute
    );
    assert_eq!(
        classify_relative_time(3_599_999),
        RelativeTimeDisplay::Minutes(59)
    );
    assert_eq!(
        classify_relative_time(86_399_999),
        RelativeTimeDisplay::HoursMinutes {
            hours: 23,
            minutes: 59,
        }
    );
    assert_eq!(
        classify_relative_time(172_800_000),
        RelativeTimeDisplay::DaysHours { days: 2, hours: 0 }
    );
}

/// End-to-end coverage of the security layer wired in `build_router_with_state`.
mod router_security {
    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt; // for `oneshot`

    use crate::hub::build_router_with_config;
    use crate::hub::config::HubConfig;
    use crate::hub::state::AppState;

    fn router() -> axum::Router {
        let dir = tempfile::tempdir().unwrap();
        let app = Arc::new(AppState::local_only(dir.path().to_path_buf()));
        // The tempdir may drop here: the chamber index starts empty, so these
        // tests never read the workspace path (handlers 403 / 404 / list-empty).
        build_router_with_config(app, HubConfig::default())
    }

    #[tokio::test]
    async fn post_start_without_csrf_header_is_forbidden() {
        let req = Request::builder()
            .method("POST")
            .uri("/api/chambers/whatever/start")
            .header("host", "127.0.0.1:8765")
            .body(Body::empty())
            .unwrap();
        let resp = router().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn post_start_with_csrf_header_reaches_handler() {
        let req = Request::builder()
            .method("POST")
            .uri("/api/chambers/whatever/start")
            .header("host", "127.0.0.1:8765")
            .header("x-cryo-csrf", "1")
            .body(Body::empty())
            .unwrap();
        let resp = router().oneshot(req).await.unwrap();
        // Guard passed, so routing reached the handler; the unknown chamber id
        // resolves to 404 — the point is that it is not the middleware's 403.
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn post_reset_with_same_origin_fetch_metadata_reaches_handler() {
        let req = Request::builder()
            .method("POST")
            .uri("/api/chambers/whatever/reset")
            .header("host", "127.0.0.1:8765")
            .header("sec-fetch-site", "same-origin")
            .body(Body::empty())
            .unwrap();
        let resp = router().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn post_reset_cross_site_is_forbidden() {
        let req = Request::builder()
            .method("POST")
            .uri("/api/chambers/whatever/reset")
            .header("host", "127.0.0.1:8765")
            .header("sec-fetch-site", "cross-site")
            .body(Body::empty())
            .unwrap();
        let resp = router().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn foreign_host_header_is_forbidden() {
        let req = Request::builder()
            .method("GET")
            .uri("/api/chambers")
            .header("host", "evil.example.com")
            .body(Body::empty())
            .unwrap();
        let resp = router().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn hostless_post_reaches_handler() {
        // A request with no Host header can only come from a local non-browser
        // client (the oneshot harness, curl, scripts). It is not a CSRF or
        // DNS-rebinding vector — a browser cannot omit Host — so both guards are
        // skipped and it reaches the handler (404 for the unknown chamber).
        let req = Request::builder()
            .method("POST")
            .uri("/api/chambers/whatever/start")
            .body(Body::empty())
            .unwrap();
        let resp = router().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn loopback_get_is_allowed() {
        let req = Request::builder()
            .method("GET")
            .uri("/api/chambers")
            .header("host", "localhost:8765")
            .body(Body::empty())
            .unwrap();
        let resp = router().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
