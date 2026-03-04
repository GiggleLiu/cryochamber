use anyhow::{Context, Result};
use std::path::Path;

/// Install and start a Windows service via the Service Control Manager.
///
/// Note: For full Windows Service support, the executable must implement
/// the Windows Service dispatch table. As a fallback, this uses `sc.exe`
/// to create a service entry. If the executable is not service-aware,
/// consider using CRYO_NO_SERVICE=1 to fall back to direct process spawn.
pub fn install(
    label_prefix: &str,
    dir: &Path,
    exe: &Path,
    args: &[&str],
    _log_file: &Path,
    _keep_alive: bool,
) -> Result<()> {
    let label = crate::service::service_label(label_prefix, dir);

    // Build the binPath argument for sc.exe (quoted exe + args)
    let bin_path = std::iter::once(format!("\"{}\"", exe.display()))
        .chain(args.iter().map(|a| format!("\"{}\"", a)))
        .collect::<Vec<_>>()
        .join(" ");

    let status = std::process::Command::new("sc.exe")
        .args([
            "create",
            &label,
            &format!("binPath={}", bin_path),
            "start=auto",
            &format!("DisplayName=Cryochamber {} ({})", label_prefix, dir.display()),
        ])
        .status()
        .context("Failed to run sc.exe create")?;
    if !status.success() {
        anyhow::bail!("sc.exe create failed for service {label}");
    }

    let status = std::process::Command::new("sc.exe")
        .args(["start", &label])
        .status()
        .context("Failed to run sc.exe start")?;
    if !status.success() {
        eprintln!("Warning: sc.exe start failed for {label} (may already be running)");
    }

    Ok(())
}

/// Uninstall a Windows service. Returns true if a service was found and removed.
pub fn uninstall(label_prefix: &str, dir: &Path) -> Result<bool> {
    let label = crate::service::service_label(label_prefix, dir);

    // Stop the service first (ignore errors if not running)
    let _ = std::process::Command::new("sc.exe")
        .args(["stop", &label])
        .status();

    let status = std::process::Command::new("sc.exe")
        .args(["delete", &label])
        .status()
        .context("Failed to run sc.exe delete")?;

    Ok(status.success())
}

/// Check if a Windows service is installed.
pub fn is_installed(label_prefix: &str, dir: &Path) -> bool {
    let label = crate::service::service_label(label_prefix, dir);

    std::process::Command::new("sc.exe")
        .args(["query", &label])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
