// src/service.rs
//! OS service management: install/uninstall platform-specific services
//! that survive reboots. Delegates to platform layer.

use anyhow::Result;
use std::path::Path;

/// Derive a short hex hash from a path for unique service naming.
pub fn dir_hash(dir: &Path) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    dir.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Build a unique service label for a given prefix and project directory.
/// e.g. "com.cryo.daemon.abc123..." or "com.cryo.gh-sync.abc123..."
pub fn service_label(prefix: &str, dir: &Path) -> String {
    format!("com.cryo.{}.{}", prefix, dir_hash(dir))
}

/// Install and start a system service.
pub fn install(
    label_prefix: &str,
    dir: &Path,
    exe: &Path,
    args: &[&str],
    log_file: &Path,
    keep_alive: bool,
) -> Result<()> {
    crate::platform::service::install(label_prefix, dir, exe, args, log_file, keep_alive)
}

/// Uninstall a system service. Returns true if a service was found and removed.
pub fn uninstall(label_prefix: &str, dir: &Path) -> Result<bool> {
    crate::platform::service::uninstall(label_prefix, dir)
}

/// Check if a service is installed.
pub fn is_installed(label_prefix: &str, dir: &Path) -> bool {
    crate::platform::service::is_installed(label_prefix, dir)
}
