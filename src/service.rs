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

    let args_xml: String = std::iter::once(format!("    <string>{}</string>", exe.display()))
        .chain(args.iter().map(|a| format!("    <string>{a}</string>")))
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
  <key>RunAtLoad</key>
  <true/>
{keep_alive_xml}
  <key>StandardOutPath</key>
  <string>{log}</string>
  <key>StandardErrorPath</key>
  <string>{log}</string>
</dict>
</plist>"#,
        label = label,
        args_xml = args_xml,
        dir = dir.display(),
        keep_alive_xml = keep_alive_xml,
        log = log_file.display(),
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

    let exec_start = format!(
        "{} {}",
        exe.display(),
        args.iter()
            .map(|a| a.to_string())
            .collect::<Vec<_>>()
            .join(" ")
    );

    let restart = if keep_alive { "always" } else { "on-failure" };

    let unit = format!(
        "[Unit]\n\
         Description=Cryochamber {prefix} ({dir})\n\
         \n\
         [Service]\n\
         ExecStart={exec_start}\n\
         WorkingDirectory={dir}\n\
         Restart={restart}\n\
         StandardOutput=append:{log}\n\
         StandardError=append:{log}\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
        prefix = label_prefix,
        exec_start = exec_start,
        dir = dir.display(),
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
