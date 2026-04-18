//! Chamber discovery: scan `./chambers/*/cryo.toml` and merge with the daemon registry.

use std::path::{Path, PathBuf};

/// Where a chamber was sourced from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// Under `./chambers/` in the workspace.
    Workspace,
    /// Running daemon registered elsewhere on the machine.
    External,
}

/// Encode a canonicalized absolute path as a URL-safe chamber id.
pub fn encode_id(path: &Path) -> String {
    urlencoding::encode(&path.to_string_lossy()).into_owned()
}

/// Decode a chamber id back to an absolute path.
pub fn decode_id(id: &str) -> Option<PathBuf> {
    // Validate that all % sequences are valid hex pairs
    let mut chars = id.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            let hex1 = chars.next()?;
            let hex2 = chars.next()?;
            if !hex1.is_ascii_hexdigit() || !hex2.is_ascii_hexdigit() {
                return None;
            }
        }
    }
    urlencoding::decode(id).ok().map(|s| PathBuf::from(s.into_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_round_trip() {
        let path = PathBuf::from("/Users/alice/work space/chambers/my chamber");
        let id = encode_id(&path);
        assert!(!id.contains(' '), "id must be URL-safe");
        assert!(!id.contains('/'), "id must not contain raw slashes");
        let back = decode_id(&id).unwrap();
        assert_eq!(back, path);
    }

    #[test]
    fn decode_rejects_invalid() {
        // %ZZ is not valid percent-encoding
        assert!(decode_id("%ZZ").is_none());
    }

    #[test]
    fn source_serialises_lowercase() {
        let json = serde_json::to_string(&Source::Workspace).unwrap();
        assert_eq!(json, "\"workspace\"");
        let json = serde_json::to_string(&Source::External).unwrap();
        assert_eq!(json, "\"external\"");
    }
}
