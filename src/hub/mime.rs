//! One extension → content-type table, shared by every route that serves a
//! file off disk (chamber attachments and the built Agent Console).
//!
//! Keeping it in one place matters more for the console than for attachments:
//! a browser refuses an ES module served as `application/octet-stream`, so a
//! missing entry here is a blank page rather than a cosmetic wart.

/// The content type for `name`, matched on its extension. Unknown extensions
/// fall back to `application/octet-stream` — the safe answer for a byte stream
/// no browser should try to interpret.
pub fn mime_for(name: &str) -> &'static str {
    match name
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        // Documents and code
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "webmanifest" => "application/manifest+json",
        "wasm" => "application/wasm",
        "txt" | "md" | "log" => "text/plain; charset=utf-8",
        "pdf" => "application/pdf",
        // Images
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        // Fonts
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
#[path = "../unit_tests/hub/mime.rs"]
mod tests;
