//! Bearer-token store for public-mode cryohub: one owner token, N named
//! invite tokens scoped to chamber ids. Backing file is JSON at
//! `~/.config/cryo/cryohub-tokens.json`, mode 0600. Revocation tombstones
//! (`revoked_at`) rather than deletes, so the audit trail survives.

use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenFile {
    pub owner: Option<String>,
    #[serde(default)]
    pub invites: Vec<Invite>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Invite {
    pub token: String,
    pub name: String,
    pub chambers: Vec<String>,
    pub created_at: String,
    #[serde(default)]
    pub revoked_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Role {
    Owner,
    Invite { name: String, chambers: Vec<String> },
}

/// 32 bytes from the OS CSPRNG, hex-encoded. No extra dependency: the hub
/// only runs on unix hosts (it already manages systemd/launchd services).
pub fn generate_token() -> Result<String> {
    let mut buf = [0u8; 32];
    std::fs::File::open("/dev/urandom")
        .context("open /dev/urandom")?
        .read_exact(&mut buf)
        .context("read /dev/urandom")?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

/// Constant-time byte comparison — cheap insurance against timing probes.
fn ct_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes()
        .zip(b.bytes())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

impl TokenFile {
    pub fn resolve(&self, bearer: &str) -> Option<Role> {
        if let Some(owner) = &self.owner {
            if ct_eq(owner, bearer) {
                return Some(Role::Owner);
            }
        }
        self.invites
            .iter()
            .filter(|i| i.revoked_at.is_none())
            .find(|i| ct_eq(&i.token, bearer))
            .map(|i| Role::Invite {
                name: i.name.clone(),
                chambers: i.chambers.clone(),
            })
    }

    /// Create the owner token if absent; return it either way.
    pub fn ensure_owner(&mut self) -> Result<String> {
        if let Some(owner) = &self.owner {
            return Ok(owner.clone());
        }
        let token = generate_token()?;
        self.owner = Some(token.clone());
        Ok(token)
    }

    pub fn create_invite(&mut self, name: &str, chambers: Vec<String>) -> Result<Invite> {
        if name.trim().is_empty() {
            bail!("invite name is empty");
        }
        if self
            .invites
            .iter()
            .any(|i| i.name == name && i.revoked_at.is_none())
        {
            bail!("an active invite named '{name}' already exists");
        }
        // Store scopes in the canonical (encoded) form the chamber index keys
        // on. An operator may type either form on the CLI; normalizing here is
        // what lets the guard, the chamber list and the SSE filter all compare
        // the same string.
        let chambers: Vec<String> = chambers
            .iter()
            .map(|c| crate::hub::discovery::canonical_scope(c))
            .collect();
        let invite = Invite {
            token: generate_token()?,
            name: name.to_string(),
            chambers,
            created_at: chrono::Utc::now().to_rfc3339(),
            revoked_at: None,
        };
        self.invites.push(invite.clone());
        Ok(invite)
    }

    /// Tombstone the active invite with this name. Returns false if none.
    pub fn revoke(&mut self, name: &str) -> bool {
        for i in &mut self.invites {
            if i.name == name && i.revoked_at.is_none() {
                i.revoked_at = Some(chrono::Utc::now().to_rfc3339());
                return true;
            }
        }
        false
    }
}

/// Tokens live under the config root (honors `XDG_CONFIG_HOME`) rather than
/// `dirs::config_dir()` — see `crate::hub::paths::hub_tokens_path`.
pub fn default_tokens_path() -> PathBuf {
    crate::hub::paths::hub_tokens_path()
}

pub fn load_tokens(path: &Path) -> Result<TokenFile> {
    if !path.exists() {
        return Ok(TokenFile::default());
    }
    let raw = std::fs::read_to_string(path).with_context(|| format!("read {path:?}"))?;
    serde_json::from_str(&raw).with_context(|| format!("parse {path:?}"))
}

pub fn save_tokens(path: &Path, tokens: &TokenFile) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let raw = serde_json::to_string_pretty(tokens)?;

    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;

        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        let base = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("tokens.json");

        // Write to a uniquely-named temp file in the same directory, created
        // 0600 from the start. This avoids (a) the permission window where a
        // new target exists as 0644 before set_permissions runs, (b) a chmod
        // failure leaving already-written credentials too permissive, and (c)
        // truncating/destroying the previous valid store on write failure
        // (the rename below replaces it atomically).
        let mut attempt = 0u32;
        let (tmp_path, mut file) = loop {
            let tmp_path = parent.join(format!(".{base}.tmp.{}.{attempt}", std::process::id()));
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&tmp_path)
            {
                Ok(f) => break (tmp_path, f),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    attempt += 1;
                    if attempt > 100 {
                        return Err(e)
                            .with_context(|| format!("create unique temp file {tmp_path:?}"));
                    }
                }
                Err(e) => {
                    return Err(e).with_context(|| format!("create temp file {tmp_path:?}"));
                }
            }
        };

        let write_result = (|| -> std::io::Result<()> {
            file.write_all(raw.as_bytes())?;
            file.flush()?;
            file.sync_all()?;
            Ok(())
        })();
        if let Err(e) = write_result {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(e).with_context(|| format!("write temp file {tmp_path:?}"));
        }
        drop(file);

        std::fs::rename(&tmp_path, path).with_context(|| {
            let _ = std::fs::remove_file(&tmp_path);
            format!("rename {tmp_path:?} -> {path:?}")
        })?;
    }

    #[cfg(not(unix))]
    {
        std::fs::write(path, raw)?;
    }

    Ok(())
}

#[cfg(test)]
#[path = "../unit_tests/hub/tokens.rs"]
mod tests;
