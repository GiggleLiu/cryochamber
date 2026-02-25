// src/service.rs
//! OS service management: install/uninstall launchd (macOS) or systemd (Linux)
//! user services that survive reboots.

use anyhow::{Context, Result};
use std::path::Path;

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
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
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

    let args_xml: String =
        std::iter::once(format!("    <string>{}</string>", xml_escape(&exe.display().to_string())))
            .chain(args.iter().map(|a| format!("    <string>{}</string>", xml_escape(a))))
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

    std::fs::write(&plist_path, plist)?;

    let status = std::process::Command::new("launchctl")
        .args(["load", "-w"])
        .arg(&plist_path)
        .status()
        .context("Failed to run launchctl")?;
    if !status.success() {
        anyhow::bail!("launchctl load failed");
    }

    Ok(())
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
