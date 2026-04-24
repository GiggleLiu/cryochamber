// src/service.rs
//! OS service management: install/uninstall launchd (macOS) or systemd (Linux)
//! user services that survive reboots.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// One installed service discovered by `list_installed`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledService {
    /// The full service label, e.g. `com.cryo.hub.abc1234567890def`.
    pub label: String,
    /// The working directory the service was installed for, parsed from the
    /// unit/plist file. May be `None` if the file is malformed.
    pub dir: Option<PathBuf>,
}

/// Derive a short hex hash from a path for unique service naming.
fn path_hash(dir: &Path) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    dir.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Build a unique service label for a given prefix and project directory.
/// e.g. "com.cryo.daemon.abc123..." or "com.cryo.gh-sync.abc123..."
pub fn service_label(prefix: &str, dir: &Path) -> String {
    format!("com.cryo.{}.{}", prefix, path_hash(dir))
}

/// Escape XML special characters for safe embedding in plist <string> elements.
#[cfg(target_os = "macos")]
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LaunchctlInstallAction {
    WritePlistAndLoad { unload_first: bool },
    LoadExistingPlist,
    Kickstart,
}

#[cfg(target_os = "macos")]
fn launchctl_install_action(plist_changed: bool, label_loaded: bool) -> LaunchctlInstallAction {
    match (plist_changed, label_loaded) {
        (true, label_loaded) => LaunchctlInstallAction::WritePlistAndLoad {
            unload_first: label_loaded,
        },
        (false, false) => LaunchctlInstallAction::LoadExistingPlist,
        (false, true) => LaunchctlInstallAction::Kickstart,
    }
}

/// Install and start a system service.
///
/// - `label_prefix`: e.g. "daemon" or "gh-sync"
/// - `dir`: working directory for the service
/// - `exe`: path to the executable
/// - `args`: arguments to pass
/// - `log_file`: path to log file for stdout/stderr
/// - `keep_alive`: if true, restart on any exit; if false, only restart on crash
#[cfg(target_os = "macos")]
pub fn install(
    label_prefix: &str,
    dir: &Path,
    exe: &Path,
    args: &[&str],
    log_file: &Path,
    keep_alive: bool,
) -> Result<()> {
    let label = service_label(label_prefix, dir);
    let agents_dir = dirs::home_dir()
        .context("Cannot determine home directory")?
        .join("Library/LaunchAgents");
    std::fs::create_dir_all(&agents_dir)?;
    let plist_path = agents_dir.join(format!("{label}.plist"));

    let args_xml: String = std::iter::once(format!(
        "    <string>{}</string>",
        xml_escape(&exe.display().to_string())
    ))
    .chain(
        args.iter()
            .map(|a| format!("    <string>{}</string>", xml_escape(a))),
    )
    .collect::<Vec<_>>()
    .join("\n");

    // KeepAlive: true = always restart
    // KeepAlive with SuccessfulExit: false = restart only on crash (non-zero exit)
    let keep_alive_xml = if keep_alive {
        "  <key>KeepAlive</key>\n  <true/>".to_string()
    } else {
        "  <key>KeepAlive</key>\n  <dict>\n    <key>SuccessfulExit</key>\n    <false/>\n  </dict>"
            .to_string()
    };

    // Capture PATH so the daemon can find agent binaries (e.g. opencode, claude).
    // launchd services get a minimal PATH by default.
    let path_env = std::env::var("PATH").unwrap_or_default();

    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{label}</string>
  <key>ProgramArguments</key>
  <array>
{args_xml}
  </array>
  <key>WorkingDirectory</key>
  <string>{dir}</string>
  <key>EnvironmentVariables</key>
  <dict>
    <key>PATH</key>
    <string>{path}</string>
  </dict>
  <key>RunAtLoad</key>
  <true/>
{keep_alive_xml}
  <key>StandardOutPath</key>
  <string>{log}</string>
  <key>StandardErrorPath</key>
  <string>{log}</string>
</dict>
</plist>"#,
        label = xml_escape(&label),
        args_xml = args_xml,
        dir = xml_escape(&dir.display().to_string()),
        path = xml_escape(&path_env),
        keep_alive_xml = keep_alive_xml,
        log = xml_escape(&log_file.display().to_string()),
    );

    // Every touch of `~/Library/LaunchAgents/` can fire a macOS 13+
    // "Background items added" popup, so we only rewrite the plist when the
    // content actually changed. A Start click on a chamber whose plist is
    // already correct stays silent.
    let plist_changed = match std::fs::read_to_string(&plist_path) {
        Ok(existing) => existing != plist,
        Err(_) => true,
    };
    let label_loaded = launchctl_tracks(&label);

    match launchctl_install_action(plist_changed, label_loaded) {
        LaunchctlInstallAction::WritePlistAndLoad { unload_first } => {
            if unload_first {
                // Unload the stale version before overwriting — launchd keeps a
                // handle on the old file otherwise.
                let _ = std::process::Command::new("launchctl")
                    .args(["unload", "-w"])
                    .arg(&plist_path)
                    .status();
            }
            std::fs::write(&plist_path, plist)?;
            launchctl_load_plist(&plist_path)?;
        }
        LaunchctlInstallAction::LoadExistingPlist => {
            // Plist is already up to date but launchd forgot about it (e.g. after
            // a logout). A plain `load -w` is enough.
            launchctl_load_plist(&plist_path)?;
        }
        LaunchctlInstallAction::Kickstart => {
            // Plist unchanged and launchd knows the label — but the daemon may
            // have exited on its own (hibernate --complete, plan finished). Use
            // `kickstart -k` to restart it without rewriting the plist, so no
            // "Background items added" popup fires.
            let uid = unsafe { libc::getuid() };
            let _ = std::process::Command::new("launchctl")
                .args(["kickstart", "-k", &format!("gui/{uid}/{label}")])
                .status();
        }
    }

    Ok(())
}

#[cfg(target_os = "macos")]
fn launchctl_load_plist(plist_path: &Path) -> Result<()> {
    let status = std::process::Command::new("launchctl")
        .args(["load", "-w"])
        .arg(plist_path)
        .status()
        .context("Failed to run launchctl")?;
    if !status.success() {
        anyhow::bail!("launchctl load failed");
    }
    Ok(())
}

/// Returns true if launchd knows about the given label (loaded, regardless
/// of whether the process is currently running).
#[cfg(target_os = "macos")]
fn launchctl_tracks(label: &str) -> bool {
    std::process::Command::new("launchctl")
        .args(["list", label])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Uninstall a system service. Returns true if a service was found and removed.
#[cfg(target_os = "macos")]
pub fn uninstall(label_prefix: &str, dir: &Path) -> Result<bool> {
    let label = service_label(label_prefix, dir);
    let plist_path = dirs::home_dir()
        .context("Cannot determine home directory")?
        .join("Library/LaunchAgents")
        .join(format!("{label}.plist"));

    if !plist_path.exists() {
        return Ok(false);
    }

    let _ = std::process::Command::new("launchctl")
        .args(["unload", "-w"])
        .arg(&plist_path)
        .status();
    std::fs::remove_file(&plist_path)?;
    Ok(true)
}

/// Check if a service is installed.
#[cfg(target_os = "macos")]
pub fn is_installed(label_prefix: &str, dir: &Path) -> bool {
    let label = service_label(label_prefix, dir);
    dirs::home_dir()
        .map(|h| {
            h.join("Library/LaunchAgents")
                .join(format!("{label}.plist"))
                .exists()
        })
        .unwrap_or(false)
}

/// List every installed user service whose label starts with
/// `com.cryo.{label_prefix}.`. Each entry includes the directory the service
/// was installed for, parsed out of the unit/plist file.
#[cfg(target_os = "macos")]
pub fn list_installed(label_prefix: &str) -> Vec<InstalledService> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    let agents = home.join("Library/LaunchAgents");
    let prefix = format!("com.cryo.{label_prefix}.");
    let suffix = ".plist";
    list_installed_in(&agents, &prefix, suffix, parse_plist_working_directory)
}

/// Extract `<key>WorkingDirectory</key><string>...</string>` from a plist.
#[cfg(target_os = "macos")]
fn parse_plist_working_directory(text: &str) -> Option<String> {
    let key_end = text.find("<key>WorkingDirectory</key>")?;
    let after = &text[key_end..];
    let str_open = after.find("<string>")?;
    let str_start = str_open + "<string>".len();
    let str_end_rel = after[str_start..].find("</string>")?;
    Some(after[str_start..str_start + str_end_rel].to_string())
}

#[cfg(target_os = "linux")]
pub fn install(
    label_prefix: &str,
    dir: &Path,
    exe: &Path,
    args: &[&str],
    log_file: &Path,
    keep_alive: bool,
) -> Result<()> {
    let label = service_label(label_prefix, dir);
    let unit_dir = dirs::home_dir()
        .context("Cannot determine home directory")?
        .join(".config/systemd/user");
    std::fs::create_dir_all(&unit_dir)?;
    let unit_path = unit_dir.join(format!("{label}.service"));

    // Quote executable and arguments for systemd ExecStart (handles spaces/special chars)
    let exec_start = std::iter::once(format!("\"{}\"", exe.display()))
        .chain(args.iter().map(|a| format!("\"{}\"", a)))
        .collect::<Vec<_>>()
        .join(" ");

    let restart = if keep_alive { "always" } else { "on-failure" };

    // Capture PATH so the daemon can find agent binaries (e.g. opencode, claude).
    let path_env = std::env::var("PATH").unwrap_or_default();

    let unit = format!(
        "[Unit]\n\
         Description=Cryochamber {prefix} ({dir})\n\
         \n\
         [Service]\n\
         ExecStart={exec_start}\n\
         WorkingDirectory={dir}\n\
         Environment=PATH={path}\n\
         Restart={restart}\n\
         StandardOutput=append:{log}\n\
         StandardError=append:{log}\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
        prefix = label_prefix,
        exec_start = exec_start,
        dir = dir.display(),
        path = path_env,
        restart = restart,
        log = log_file.display(),
    );

    std::fs::write(&unit_path, unit)?;

    let status = std::process::Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status()
        .context("Failed to run systemctl")?;
    if !status.success() {
        anyhow::bail!("systemctl daemon-reload failed");
    }

    let status = std::process::Command::new("systemctl")
        .args(["--user", "enable", "--now", &label])
        .status()?;
    if !status.success() {
        anyhow::bail!("systemctl enable --now failed");
    }

    Ok(())
}

#[cfg(target_os = "linux")]
pub fn uninstall(label_prefix: &str, dir: &Path) -> Result<bool> {
    let label = service_label(label_prefix, dir);
    let unit_path = dirs::home_dir()
        .context("Cannot determine home directory")?
        .join(".config/systemd/user")
        .join(format!("{label}.service"));

    if !unit_path.exists() {
        return Ok(false);
    }

    let _ = std::process::Command::new("systemctl")
        .args(["--user", "disable", "--now", &label])
        .status();
    std::fs::remove_file(&unit_path)?;
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status();
    Ok(true)
}

#[cfg(target_os = "linux")]
pub fn is_installed(label_prefix: &str, dir: &Path) -> bool {
    let label = service_label(label_prefix, dir);
    dirs::home_dir()
        .map(|h| {
            h.join(".config/systemd/user")
                .join(format!("{label}.service"))
                .exists()
        })
        .unwrap_or(false)
}

/// List every installed user service whose label starts with
/// `com.cryo.{label_prefix}.`. Each entry includes the directory the service
/// was installed for, parsed out of the unit file.
#[cfg(target_os = "linux")]
pub fn list_installed(label_prefix: &str) -> Vec<InstalledService> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    let units = home.join(".config/systemd/user");
    let prefix = format!("com.cryo.{label_prefix}.");
    let suffix = ".service";
    list_installed_in(&units, &prefix, suffix, parse_unit_working_directory)
}

/// Extract `WorkingDirectory=...` from a systemd unit file.
#[cfg(target_os = "linux")]
fn parse_unit_working_directory(text: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("WorkingDirectory=") {
            return Some(rest.trim().to_string());
        }
    }
    None
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn install(
    _label_prefix: &str,
    _dir: &Path,
    _exe: &Path,
    _args: &[&str],
    _log_file: &Path,
    _keep_alive: bool,
) -> Result<()> {
    anyhow::bail!("OS service management is not supported on this platform")
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn uninstall(_label_prefix: &str, _dir: &Path) -> Result<bool> {
    anyhow::bail!("OS service management is not supported on this platform")
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn is_installed(_label_prefix: &str, _dir: &Path) -> bool {
    false
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn list_installed(_label_prefix: &str) -> Vec<InstalledService> {
    Vec::new()
}

/// Scan `dir` for files whose name starts with `prefix` and ends with `suffix`,
/// reading each file with `parse_dir` to extract its working directory. Used
/// by both the macOS and Linux implementations of `list_installed`.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn list_installed_in(
    dir: &Path,
    prefix: &str,
    suffix: &str,
    parse_dir: fn(&str) -> Option<String>,
) -> Vec<InstalledService> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in rd.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if !name.starts_with(prefix) || !name.ends_with(suffix) {
            continue;
        }
        let label = name[..name.len() - suffix.len()].to_string();
        let working_dir = std::fs::read_to_string(entry.path())
            .ok()
            .and_then(|s| parse_dir(&s))
            .map(PathBuf::from);
        out.push(InstalledService {
            label,
            dir: working_dir,
        });
    }
    out.sort_by(|a, b| a.label.cmp(&b.label));
    out
}

#[cfg(all(test, target_os = "macos"))]
#[path = "unit_tests/service.rs"]
mod tests;
