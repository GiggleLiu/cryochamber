//! KaTeX font files, vendored and embedded at compile time.
//!
//! Served by `cryohub` at `/assets/vendor/fonts/{name}`. Filenames must
//! match the `url(fonts/...)` references in `templates/vendor/katex.min.css`.
//! Keep in sync when upgrading KaTeX (see templates/vendor/README.md).

/// Look up a KaTeX font by filename, returning its embedded bytes.
pub fn get(name: &str) -> Option<&'static [u8]> {
    match name {
        "KaTeX_AMS-Regular.woff2" => Some(include_bytes!(
            "../../../templates/vendor/fonts/KaTeX_AMS-Regular.woff2"
        )),
        "KaTeX_Caligraphic-Bold.woff2" => Some(include_bytes!(
            "../../../templates/vendor/fonts/KaTeX_Caligraphic-Bold.woff2"
        )),
        "KaTeX_Caligraphic-Regular.woff2" => Some(include_bytes!(
            "../../../templates/vendor/fonts/KaTeX_Caligraphic-Regular.woff2"
        )),
        "KaTeX_Fraktur-Bold.woff2" => Some(include_bytes!(
            "../../../templates/vendor/fonts/KaTeX_Fraktur-Bold.woff2"
        )),
        "KaTeX_Fraktur-Regular.woff2" => Some(include_bytes!(
            "../../../templates/vendor/fonts/KaTeX_Fraktur-Regular.woff2"
        )),
        "KaTeX_Main-Bold.woff2" => Some(include_bytes!(
            "../../../templates/vendor/fonts/KaTeX_Main-Bold.woff2"
        )),
        "KaTeX_Main-BoldItalic.woff2" => Some(include_bytes!(
            "../../../templates/vendor/fonts/KaTeX_Main-BoldItalic.woff2"
        )),
        "KaTeX_Main-Italic.woff2" => Some(include_bytes!(
            "../../../templates/vendor/fonts/KaTeX_Main-Italic.woff2"
        )),
        "KaTeX_Main-Regular.woff2" => Some(include_bytes!(
            "../../../templates/vendor/fonts/KaTeX_Main-Regular.woff2"
        )),
        "KaTeX_Math-BoldItalic.woff2" => Some(include_bytes!(
            "../../../templates/vendor/fonts/KaTeX_Math-BoldItalic.woff2"
        )),
        "KaTeX_Math-Italic.woff2" => Some(include_bytes!(
            "../../../templates/vendor/fonts/KaTeX_Math-Italic.woff2"
        )),
        "KaTeX_SansSerif-Bold.woff2" => Some(include_bytes!(
            "../../../templates/vendor/fonts/KaTeX_SansSerif-Bold.woff2"
        )),
        "KaTeX_SansSerif-Italic.woff2" => Some(include_bytes!(
            "../../../templates/vendor/fonts/KaTeX_SansSerif-Italic.woff2"
        )),
        "KaTeX_SansSerif-Regular.woff2" => Some(include_bytes!(
            "../../../templates/vendor/fonts/KaTeX_SansSerif-Regular.woff2"
        )),
        "KaTeX_Script-Regular.woff2" => Some(include_bytes!(
            "../../../templates/vendor/fonts/KaTeX_Script-Regular.woff2"
        )),
        "KaTeX_Size1-Regular.woff2" => Some(include_bytes!(
            "../../../templates/vendor/fonts/KaTeX_Size1-Regular.woff2"
        )),
        "KaTeX_Size2-Regular.woff2" => Some(include_bytes!(
            "../../../templates/vendor/fonts/KaTeX_Size2-Regular.woff2"
        )),
        "KaTeX_Size3-Regular.woff2" => Some(include_bytes!(
            "../../../templates/vendor/fonts/KaTeX_Size3-Regular.woff2"
        )),
        "KaTeX_Size4-Regular.woff2" => Some(include_bytes!(
            "../../../templates/vendor/fonts/KaTeX_Size4-Regular.woff2"
        )),
        "KaTeX_Typewriter-Regular.woff2" => Some(include_bytes!(
            "../../../templates/vendor/fonts/KaTeX_Typewriter-Regular.woff2"
        )),
        _ => None,
    }
}
