use super::*;

#[test]
fn console_build_output_gets_types_a_browser_will_execute() {
    // These four are the ones a vite build cannot survive without: a module
    // served as octet-stream is refused outright, and the rest degrade the app.
    assert!(mime_for("index-abc123.js").starts_with("text/javascript"));
    assert!(mime_for("worker-abc123.mjs").starts_with("text/javascript"));
    assert!(mime_for("index-abc123.css").starts_with("text/css"));
    assert_eq!(
        mime_for("manifest.webmanifest"),
        "application/manifest+json"
    );
    assert!(mime_for("index.html").starts_with("text/html"));
    assert_eq!(mime_for("KaTeX_Main-Regular.woff2"), "font/woff2");
    assert_eq!(mime_for("KaTeX_Main-Regular.ttf"), "font/ttf");
    assert_eq!(mime_for("favicon.ico"), "image/x-icon");
}

#[test]
fn attachment_types_are_unchanged() {
    assert_eq!(mime_for("shot.png"), "image/png");
    assert_eq!(mime_for("shot.JPG"), "image/jpeg");
    assert_eq!(mime_for("doc.pdf"), "application/pdf");
    assert_eq!(mime_for("notes.md"), "text/plain; charset=utf-8");
    assert_eq!(mime_for("data.json"), "application/json");
    assert_eq!(mime_for("logo.svg"), "image/svg+xml");
}

#[test]
fn an_unknown_or_absent_extension_is_an_opaque_byte_stream() {
    assert_eq!(mime_for("archive.tar.zst"), "application/octet-stream");
    assert_eq!(mime_for("LICENSE"), "application/octet-stream");
    assert_eq!(mime_for(""), "application/octet-stream");
}
