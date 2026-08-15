//! HTML shell + static assets.

use axum::extract::Path;
use axum::http::StatusCode;
use axum::response::Html;

const SHELL_HTML: &str = include_str!("../../../templates/web_shell.html");
const WEB_CSS: &str = include_str!("../../../templates/web.css");
const LOGO_SVG: &str = include_str!("../../../docs/logo/logo.svg");
const MARK_SVG: &str = include_str!("../../../docs/logo/mark.svg");

// Vendored client-side rendering libs (markdown + LaTeX) so the hub needs no
// CDN at runtime — see templates/vendor/README.md for versions & licenses.
const KATEX_CSS: &str = include_str!("../../../templates/vendor/katex.min.css");
const KATEX_JS: &str = include_str!("../../../templates/vendor/katex.min.js");
const MARKED_JS: &str = include_str!("../../../templates/vendor/marked.min.js");
const PURIFY_JS: &str = include_str!("../../../templates/vendor/purify.min.js");

pub async fn get_index() -> Html<&'static str> {
    Html(SHELL_HTML)
}

pub async fn get_css() -> ([(&'static str, &'static str); 1], &'static str) {
    ([("content-type", "text/css")], WEB_CSS)
}

pub async fn get_logo() -> ([(&'static str, &'static str); 1], &'static str) {
    ([("content-type", "image/svg+xml; charset=utf-8")], LOGO_SVG)
}

pub async fn get_mark() -> ([(&'static str, &'static str); 1], &'static str) {
    ([("content-type", "image/svg+xml; charset=utf-8")], MARK_SVG)
}

pub async fn get_katex_css() -> ([(&'static str, &'static str); 1], &'static str) {
    ([("content-type", "text/css")], KATEX_CSS)
}

pub async fn get_katex_js() -> ([(&'static str, &'static str); 1], &'static str) {
    (
        [("content-type", "application/javascript; charset=utf-8")],
        KATEX_JS,
    )
}

pub async fn get_marked_js() -> ([(&'static str, &'static str); 1], &'static str) {
    (
        [("content-type", "application/javascript; charset=utf-8")],
        MARKED_JS,
    )
}

pub async fn get_purify_js() -> ([(&'static str, &'static str); 1], &'static str) {
    (
        [("content-type", "application/javascript; charset=utf-8")],
        PURIFY_JS,
    )
}

/// Serve a vendored KaTeX font (woff2). The filename is matched against the
/// embedded font table, so arbitrary paths can never be read from disk.
pub async fn get_font(
    Path(name): Path<String>,
) -> Result<([(&'static str, &'static str); 1], axum::body::Bytes), StatusCode> {
    match crate::hub::routes::fonts::get(&name) {
        Some(bytes) => Ok((
            [("content-type", "font/woff2")],
            axum::body::Bytes::from_static(bytes),
        )),
        None => Err(StatusCode::NOT_FOUND),
    }
}

#[cfg(test)]
#[path = "../../unit_tests/hub/routes/pages.rs"]
mod tests;
