//! HTML shell + static assets.

use axum::response::Html;

const SHELL_HTML: &str = include_str!("../../../templates/web_shell.html");
const WEB_CSS: &str = include_str!("../../../templates/web.css");
const LOGO_SVG: &str = include_str!("../../../docs/logo/logo.svg");
const MARK_SVG: &str = include_str!("../../../docs/logo/mark.svg");

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

#[cfg(test)]
#[path = "../../unit_tests/hub/routes/pages.rs"]
mod tests;
