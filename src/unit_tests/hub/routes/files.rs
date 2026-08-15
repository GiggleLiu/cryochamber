use super::*;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

const BOUNDARY: &str = "cryoboundary123";

/// Workspace with one discoverable chamber, wrapped in the real router so the
/// tests exercise routing, the body limit and the security layer too.
fn setup() -> (tempfile::TempDir, axum::Router, String) {
    let tmp = tempfile::tempdir().unwrap();
    let chamber = tmp.path().join("alpha");
    std::fs::create_dir_all(&chamber).unwrap();
    crate::config::save_config(
        &chamber.join("cryo.toml"),
        &crate::config::CryoConfig::default(),
    )
    .unwrap();
    let app = Arc::new(AppState::local_only(tmp.path().to_path_buf()));
    app.refresh();
    let id = app.chambers.read().unwrap().keys().next().unwrap().clone();
    (tmp, crate::hub::build_router_with_state(app), id)
}

/// The repo has no multipart client helper, so build the wire format by hand.
fn multipart_body(boundary: &str, filename: &str, bytes: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(
        format!(
            "--{boundary}\r\ncontent-disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\ncontent-type: application/octet-stream\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

fn text_only_body(boundary: &str) -> Vec<u8> {
    format!("--{boundary}\r\ncontent-disposition: form-data; name=\"note\"\r\n\r\nhello\r\n--{boundary}--\r\n")
        .into_bytes()
}

async fn upload(router: &axum::Router, id: &str, body: Vec<u8>) -> (StatusCode, Vec<u8>) {
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/chambers/{id}/uploads"))
        .header("host", "127.0.0.1")
        .header("x-cryo-csrf", "1")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .body(Body::from(body))
        .unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, bytes.to_vec())
}

async fn get(router: &axum::Router, uri: &str) -> axum::response::Response {
    let req = Request::builder()
        .method("GET")
        .uri(uri)
        .header("host", "127.0.0.1")
        .body(Body::empty())
        .unwrap();
    router.clone().oneshot(req).await.unwrap()
}

#[tokio::test]
async fn upload_then_download_roundtrip() {
    let (_tmp, router, id) = setup();
    let (status, body) = upload(
        &router,
        &id,
        multipart_body(BOUNDARY, "report.pdf", b"%PDF-fake"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["ok"], true);
    let markdown = v["markdown"].as_str().expect("markdown");
    assert!(
        markdown.starts_with("[report.pdf](/api/chambers/"),
        "got {markdown}"
    );
    assert!(markdown.contains("/files/"), "got {markdown}");

    let url = markdown
        .rsplit_once("](")
        .and_then(|(_, rest)| rest.strip_suffix(')'))
        .expect("url in markdown");
    let resp = get(&router, url).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(resp
        .headers()
        .get("content-disposition")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .contains("attachment"));
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(bytes.as_ref(), b"%PDF-fake");
}

#[tokio::test]
async fn traversal_names_404_without_fs_access() {
    let (tmp, router, id) = setup();
    // Upload once so `<chamber>/messages/attachments/` actually exists —
    // otherwise every traversal would 404 on the missing directory instead of
    // on the containment check, and the test would prove nothing.
    let (status, _) = upload(&router, &id, multipart_body(BOUNDARY, "seed.txt", b"seed")).await;
    assert_eq!(status, StatusCode::OK);
    // `<chamber>/cryo.toml` is exactly two levels above the attachments dir,
    // so `../../cryo.toml` would read a real secret-bearing file (it can hold
    // a provider API key) if the name were joined naively.
    assert!(tmp.path().join("alpha").join("cryo.toml").exists());

    for name in [
        "..%2F..%2Fcryo.toml",
        "..%2Fcryo.toml",
        ".hidden",
        "..%2F..%2F..%2Fetc%2Fpasswd",
    ] {
        let resp = get(&router, &format!("/api/chambers/{id}/files/{name}")).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND, "name {name}");
    }
}

#[tokio::test]
async fn oversized_upload_is_413() {
    let (_tmp, router, id) = setup();
    let big = vec![b'x'; crate::hub::routes::files::MAX_ATTACHMENT_BYTES + 1];
    let (status, _body) = upload(&router, &id, multipart_body(BOUNDARY, "big.bin", &big)).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn missing_file_field_is_400() {
    let (_tmp, router, id) = setup();
    let (status, _body) = upload(&router, &id, text_only_body(BOUNDARY)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn upload_to_unknown_chamber_is_404() {
    let (_tmp, router, _id) = setup();
    let (status, _body) = upload(
        &router,
        "no-such-chamber",
        multipart_body(BOUNDARY, "report.pdf", b"x"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[test]
fn safe_name_sanitizes_and_falls_back() {
    use crate::hub::routes::files::safe_name;
    assert_eq!(safe_name("report.pdf"), "report.pdf");
    assert_eq!(safe_name("a b/c.txt"), "a_b_c.txt");
    assert_eq!(safe_name(".hidden"), "hidden");
    // Whatever a traversal-shaped name maps to, it can never be one.
    let escaped = safe_name("../../etc/passwd");
    assert!(!escaped.contains('/'), "got {escaped}");
    assert!(!escaped.starts_with('.'), "got {escaped}");
    assert_eq!(safe_name("..."), "attachment");
    assert_eq!(safe_name(""), "attachment");
}
