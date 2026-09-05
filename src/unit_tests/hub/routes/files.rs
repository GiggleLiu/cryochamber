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
    (
        tmp,
        crate::hub::build_router_with_config(app, crate::hub::config::HubConfig::default()),
        id,
    )
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
async fn upload_with_a_255_byte_filename_keeps_its_extension() {
    let (_tmp, router, id) = setup();
    let original = format!("{}.pdf", "a".repeat(251));
    assert_eq!(original.len(), 255);
    let (status, body) = upload(
        &router,
        &id,
        multipart_body(BOUNDARY, &original, b"long name contents"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let stored = stored_name(&body);
    assert!(stored.len() <= MAX_STORED_NAME_BYTES);
    assert!(stored.ends_with(".pdf"));
    assert_eq!(safe_name(&stored), stored);

    let response = get(&router, &format!("/api/chambers/{id}/files/{stored}")).await;
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(bytes.as_ref(), b"long name contents");
}

#[tokio::test]
async fn retry_repairs_an_existing_truncated_attachment() {
    let (tmp, router, id) = setup();
    let body = multipart_body(BOUNDARY, "report.pdf", b"complete contents");
    let (status, response) = upload(&router, &id, body.clone()).await;
    assert_eq!(status, StatusCode::OK);
    let stored = stored_name(&response);
    let path = tmp.path().join("alpha/messages/attachments").join(&stored);
    std::fs::write(&path, b"truncated").unwrap();

    let (status, _) = upload(&router, &id, body).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(std::fs::read(path).unwrap(), b"complete contents");
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

/// The stored name the upload response advertises.
fn stored_name(body: &[u8]) -> String {
    let v: serde_json::Value = serde_json::from_slice(body).unwrap();
    v["name"].as_str().expect("name").to_string()
}

#[tokio::test]
#[cfg(unix)]
async fn a_symlinked_attachment_cannot_read_outside_the_chamber() {
    // Sanitizing the *name* stops textual `../` traversal but says nothing
    // about what an existing entry points at. `cryo.toml` may hold a provider
    // API key, and a scoped invite must not be able to fetch it through a
    // symlink someone dropped into the attachments directory.
    let (tmp, router, id) = setup();
    let (status, _) = upload(&router, &id, multipart_body(BOUNDARY, "seed.txt", b"seed")).await;
    assert_eq!(status, StatusCode::OK);

    let chamber = tmp.path().join("alpha");
    let secret = chamber.join("cryo.toml");
    assert!(secret.exists(), "the test needs a real file to point at");
    let attachments = chamber.join("messages").join("attachments");
    std::os::unix::fs::symlink(&secret, attachments.join("leak.toml")).unwrap();

    // The name itself is a legal single segment, so the refusal below is the
    // containment check and not the sanitizer.
    assert_eq!(
        crate::hub::routes::files::safe_name("leak.toml"),
        "leak.toml"
    );
    let resp = get(&router, &format!("/api/chambers/{id}/files/leak.toml")).await;
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "a symlink out of the attachments directory must not be served"
    );
}

#[tokio::test]
#[cfg(unix)]
async fn a_symlinked_attachments_directory_is_refused_both_ways() {
    // The whole directory can be swapped for a symlink, which would make every
    // read and every write land wherever it points.
    let (tmp, router, id) = setup();
    let (status, body) = upload(&router, &id, multipart_body(BOUNDARY, "seed.txt", b"seed")).await;
    assert_eq!(status, StatusCode::OK);
    let seeded = stored_name(&body);

    // Positive control: the download works while the directory is real.
    let resp = get(&router, &format!("/api/chambers/{id}/files/{seeded}")).await;
    assert_eq!(resp.status(), StatusCode::OK);

    // Now move the store outside the chamber and symlink the directory to it.
    let chamber = tmp.path().join("alpha");
    let attachments = chamber.join("messages").join("attachments");
    let elsewhere = tmp.path().join("elsewhere");
    std::fs::rename(&attachments, &elsewhere).unwrap();
    std::os::unix::fs::symlink(&elsewhere, &attachments).unwrap();
    assert!(
        elsewhere.join(&seeded).exists(),
        "the file is still reachable through the link, so a naive read would succeed"
    );

    let resp = get(&router, &format!("/api/chambers/{id}/files/{seeded}")).await;
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "download through a symlinked attachments directory must be refused"
    );
    let (status, _) = upload(&router, &id, multipart_body(BOUNDARY, "new.txt", b"new")).await;
    assert_eq!(
        status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "upload into a symlinked attachments directory must be refused"
    );
}

#[tokio::test]
async fn upload_past_the_chamber_quota_is_507() {
    // A leaked invite otherwise has an unbounded disk-exhaustion primitive:
    // the per-file cap says nothing about how many files may be stored.
    let (tmp, router, id) = setup();
    let (status, _) = upload(&router, &id, multipart_body(BOUNDARY, "seed.txt", b"seed")).await;
    assert_eq!(status, StatusCode::OK, "positive control before the quota");

    // A sparse file: quota-sized on paper, ~nothing on disk.
    let attachments = tmp
        .path()
        .join("alpha")
        .join("messages")
        .join("attachments");
    let hog = std::fs::File::create(attachments.join("hog.bin")).unwrap();
    hog.set_len(crate::hub::routes::files::MAX_ATTACHMENTS_DIR_BYTES)
        .unwrap();
    drop(hog);

    let (status, _) = upload(&router, &id, multipart_body(BOUNDARY, "next.txt", b"next")).await;
    assert_eq!(
        status,
        StatusCode::INSUFFICIENT_STORAGE,
        "a chamber at its quota must refuse further uploads"
    );
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

#[test]
fn active_content_types_are_downgraded_to_octet_stream_for_attachments() {
    for name in [
        "page.html",
        "page.htm",
        "page.xhtml",
        "app.js",
        "app.mjs",
        "pic.svg",
    ] {
        assert_eq!(
            attachment_content_type(name),
            "application/octet-stream",
            "{name}"
        );
    }
    assert_eq!(attachment_content_type("pic.png"), "image/png");
    assert_eq!(attachment_content_type("doc.pdf"), "application/pdf");
    // Case is not a loophole, and only the *last* extension decides.
    assert_eq!(
        attachment_content_type("pic.SVG"),
        "application/octet-stream"
    );
    assert_eq!(
        attachment_content_type("PAGE.HTML"),
        "application/octet-stream"
    );
    assert_eq!(attachment_content_type("x.svg.png"), "image/png");
    assert_eq!(
        attachment_content_type("x.png.svg"),
        "application/octet-stream"
    );
}

/// The header the route actually emits. Downgrading in a helper is worth
/// nothing if the call site drifts back to `mime_for`, so this drives the real
/// router: upload, then fetch the stored attachment and read its headers.
#[tokio::test]
async fn served_attachments_never_carry_an_active_content_type() {
    let (_tmp, router, id) = setup();
    for (upload_name, expected_type) in [
        ("page.html", "application/octet-stream"),
        ("pic.SVG", "application/octet-stream"),
        ("pic.png", "image/png"),
    ] {
        let (status, body) = upload(
            &router,
            &id,
            multipart_body(BOUNDARY, upload_name, upload_name.as_bytes()),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "upload {upload_name}");
        let stored = stored_name(&body);
        assert!(stored.ends_with(upload_name), "stored {stored}");

        let resp = get(&router, &format!("/api/chambers/{id}/files/{stored}")).await;
        assert_eq!(resp.status(), StatusCode::OK, "get {stored}");
        assert_eq!(
            resp.headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok()),
            Some(expected_type),
            "content-type for {upload_name}"
        );
        assert!(
            resp.headers()
                .get("content-disposition")
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default()
                .contains("attachment"),
            "content-disposition for {upload_name}"
        );
    }
}

/// The share route serves a real chamber-local artifact (a report the agent
/// produced on disk), with a safe content type and attachment disposition.
#[tokio::test]
async fn chamber_file_serves_an_articles_pdf() {
    let (tmp, router, id) = setup();
    let chamber = tmp.path().join("alpha");
    std::fs::create_dir_all(chamber.join("articles")).unwrap();
    std::fs::write(chamber.join("articles/review.pdf"), b"%PDF-fake").unwrap();

    let resp = get(
        &router,
        &format!("/api/chambers/{id}/file?path=articles/review.pdf"),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("application/pdf")
    );
    assert!(
        resp.headers()
            .get("content-disposition")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .contains("attachment"),
        "chamber files are downloads"
    );
    assert_eq!(
        resp.headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok()),
        Some("9")
    );
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(body.to_vec(), b"%PDF-fake");
}

/// Traversal must not work: `..` components, absolute paths and escapes
/// outside the chamber all 404, as does anything outside the shareable
/// directories (config, notes, the mailbox).
#[tokio::test]
async fn chamber_file_rejects_escape_and_non_shareable_paths() {
    let (tmp, router, id) = setup();
    let chamber = tmp.path().join("alpha");
    std::fs::create_dir_all(chamber.join("articles")).unwrap();
    std::fs::write(chamber.join("articles/review.pdf"), b"%PDF").unwrap();
    std::fs::write(chamber.join("cryo.toml"), b"agent = \"pi\"").unwrap();
    std::fs::create_dir_all(chamber.join("messages/inbox")).unwrap();
    std::fs::write(chamber.join("messages/inbox/secret.md"), b"nope").unwrap();

    for bad in [
        "articles/../cryo.toml",
        "articles/..%2Fcryo.toml",
        "..%2Fcryo.toml",
        "/etc/passwd",
        "cryo.toml",
        "messages/inbox/secret.md",
        "articles", // a directory, not a file
        "articles/missing.pdf",
        "", // empty path
    ] {
        let resp = get(&router, &format!("/api/chambers/{id}/file?path={bad}")).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND, "path {bad:?}");
    }

    // A symlink inside articles pointing outside the chamber is not served.
    #[cfg(unix)]
    {
        let outside = tmp.path().join("outside.pdf");
        std::fs::write(&outside, b"secret").unwrap();
        std::os::unix::fs::symlink(&outside, chamber.join("articles/link.pdf")).unwrap();
        let resp = get(
            &router,
            &format!("/api/chambers/{id}/file?path=articles/link.pdf"),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND, "symlink escape");
    }
}

/// The disposition filename is sanitized: quotes and control characters from
/// the on-disk name must not leak into the header.
#[tokio::test]
async fn chamber_file_sanitizes_the_disposition_filename() {
    let (tmp, router, id) = setup();
    let chamber = tmp.path().join("alpha");
    std::fs::create_dir_all(chamber.join("articles")).unwrap();
    std::fs::write(chamber.join("articles/a\"b\nc.pdf"), b"%PDF").unwrap();

    let resp = get(
        &router,
        &format!("/api/chambers/{id}/file?path=articles/a%22b%0Ac.pdf"),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let cd = resp
        .headers()
        .get("content-disposition")
        .and_then(|v| v.to_str().ok())
        .unwrap();
    assert_eq!(cd, "attachment; filename=\"abc.pdf\"", "header: {cd:?}");
}

#[test]
fn disposition_filename_never_returns_an_empty_fallback() {
    assert_eq!(disposition_filename("\"\n"), "download");
}

#[tokio::test]
async fn chamber_file_encodes_a_non_ascii_disposition_filename() {
    let (tmp, router, id) = setup();
    let chamber = tmp.path().join("alpha");
    std::fs::create_dir_all(chamber.join("articles")).unwrap();
    std::fs::write(chamber.join("articles/综述.pdf"), b"%PDF").unwrap();
    let path = urlencoding::encode("articles/综述.pdf");

    let resp = get(&router, &format!("/api/chambers/{id}/file?path={path}")).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let cd = resp
        .headers()
        .get("content-disposition")
        .and_then(|v| v.to_str().ok())
        .unwrap();
    assert!(cd.contains("filename=\"download.pdf\""), "header: {cd:?}");
    assert!(
        cd.contains("filename*=UTF-8''%E7%BB%BC%E8%BF%B0.pdf"),
        "header: {cd:?}"
    );
}
