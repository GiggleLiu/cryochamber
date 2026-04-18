//! HTML shell + static assets.

use axum::response::Html;

const SHELL_HTML: &str = include_str!("../../../templates/web_shell.html");
const WEB_CSS: &str = include_str!("../../../templates/web.css");

pub async fn get_index() -> Html<&'static str> {
    Html(SHELL_HTML)
}

pub async fn get_css() -> ([(&'static str, &'static str); 1], &'static str) {
    ([("content-type", "text/css")], WEB_CSS)
}
