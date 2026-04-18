//! Chamber discovery: scan `./chambers/*/cryo.toml` and merge with the daemon registry.

use std::path::{Path, PathBuf};
use std::collections::BTreeMap;

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
    urlencoding::decode(id)
        .ok()
        .map(|s| PathBuf::from(s.into_owned()))
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ChamberEntry {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    pub source: Source,
    pub config_error: Option<String>,
    pub running: bool,
    pub session: Option<u32>,
    pub next_wake: Option<String>,
    pub unread: usize,
}

/// A map from chamber id → entry.
pub type ChamberIndex = BTreeMap<String, ChamberEntry>;

/// Scan `<workspace>/chambers/*` for chambers. Returns entries for every
/// subdirectory (even ones with broken or missing `cryo.toml` — those get a
/// `config_error`). Runtime fields (`running`, `session`, `next_wake`,
/// `unread`) are filled in by `populate_runtime`, not here.
pub fn scan_workspace(workspace: &Path) -> ChamberIndex {
    let chambers_dir = workspace.join("chambers");
    let mut out = ChamberIndex::new();
    let Ok(rd) = std::fs::read_dir(&chambers_dir) else {
        return out;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let canonical = path.canonicalize().unwrap_or(path.clone());
        let name = canonical
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "(unknown)".into());
        let cryo_toml = canonical.join("cryo.toml");
        let config_error = if !cryo_toml.exists() {
            Some("missing cryo.toml".into())
        } else {
            crate::config::load_config(&cryo_toml).err().map(|e| e.to_string())
        };
        let id = encode_id(&canonical);
        out.insert(
            id.clone(),
            ChamberEntry {
                id,
                name,
                path: canonical,
                source: Source::Workspace,
                config_error,
                running: false,
                session: None,
                next_wake: None,
                unread: 0,
            },
        );
    }
    out
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

    #[test]
    fn scan_empty_workspace_returns_empty_index() {
        let dir = tempfile::tempdir().unwrap();
        let idx = scan_workspace(dir.path());
        assert!(idx.is_empty());
    }

    #[test]
    fn scan_finds_chambers_with_valid_config() {
        let dir = tempfile::tempdir().unwrap();
        let chambers = dir.path().join("chambers");
        std::fs::create_dir_all(chambers.join("alpha")).unwrap();
        std::fs::create_dir_all(chambers.join("beta")).unwrap();
        let cfg = crate::config::CryoConfig::default();
        crate::config::save_config(&chambers.join("alpha").join("cryo.toml"), &cfg).unwrap();
        crate::config::save_config(&chambers.join("beta").join("cryo.toml"), &cfg).unwrap();

        let idx = scan_workspace(dir.path());
        assert_eq!(idx.len(), 2);
        let names: Vec<_> = idx.values().map(|e| e.name.clone()).collect();
        assert!(names.contains(&"alpha".to_string()));
        assert!(names.contains(&"beta".to_string()));
        for entry in idx.values() {
            assert_eq!(entry.source, Source::Workspace);
            assert!(entry.config_error.is_none());
        }
    }

    #[test]
    fn scan_flags_missing_cryo_toml_as_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("chambers").join("broken")).unwrap();
        let idx = scan_workspace(dir.path());
        assert_eq!(idx.len(), 1);
        let entry = idx.values().next().unwrap();
        assert!(entry.config_error.is_some());
    }
}
