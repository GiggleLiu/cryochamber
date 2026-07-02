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
fn launchd_stdio_log_path(home: &Path, label: &str) -> PathBuf {
    home.join("Library")
        .join("Logs")
        .join("cryo")
        .join(format!("{label}.log"))
}

/// Path where the OS service manager will actually write stdout/stderr for
/// the service identified by `(label_prefix, dir)`. On Linux, systemd honors
/// whatever path was passed to `install`, so this returns `fallback` verbatim.
/// On macOS, `install` ignores its `log_file` argument and routes launchd
/// stdio to `~/Library/Logs/cryo/<label>.log` (so xpcproxy doesn't need TCC
/// access to protected chamber dirs); this returns that path.
///
/// Use this for printing/displaying the log location so what you show the
/// operator matches what the service actually writes.
#[cfg(target_os = "macos")]
pub fn stdio_log_path(label_prefix: &str, dir: &Path, fallback: &Path) -> PathBuf {
    let Some(home) = dirs::home_dir() else {
        return fallback.to_path_buf();
    };
    let label = service_label(label_prefix, dir);
    launchd_stdio_log_path(&home, &label)
}

#[cfg(not(target_os = "macos"))]
pub fn stdio_log_path(_label_prefix: &str, _dir: &Path, fallback: &Path) -> PathBuf {
    fallback.to_path_buf()
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LaunchctlInstallAction {
    WritePlistAndLoad { unload_first: bool },
    LoadExistingPlist,
    Kickstart,
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LaunchctlRestartAction {
    NotInstalled,
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

#[cfg(target_os = "macos")]
fn launchctl_restart_action(plist_exists: bool, label_loaded: bool) -> LaunchctlRestartAction {
    match (plist_exists, label_loaded) {
        (false, _) => LaunchctlRestartAction::NotInstalled,
        (true, false) => LaunchctlRestartAction::LoadExistingPlist,
        (true, true) => LaunchctlRestartAction::Kickstart,
    }
}

/// Install and start a system service.
///
/// - `label_prefix`: e.g. "daemon" or "gh-sync"
/// - `dir`: working directory for the service
/// - `exe`: path to the executable
/// - `args`: arguments to pass
/// - `log_file`: path to log file for stdout/stderr where the OS can open it directly.
///   macOS launchd uses `~/Library/Logs/cryo/<label>.log` instead so xpcproxy
///   does not need access to protected chamber directories such as ~/Documents.
/// - `keep_alive`: if true, restart on any exit; if false, only restart on crash
#[cfg(target_os = "macos")]
pub fn install(
    label_prefix: &str,
    dir: &Path,
    exe: &Path,
    args: &[&str],
    _log_file: &Path,
    keep_alive: bool,
) -> Result<()> {
    let label = service_label(label_prefix, dir);
    let home = dirs::home_dir().context("Cannot determine home directory")?;
    let agents_dir = home.join("Library/LaunchAgents");
    std::fs::create_dir_all(&agents_dir)?;
    let plist_path = agents_dir.join(format!("{label}.plist"));
    let launchd_log_file = launchd_stdio_log_path(&home, &label);
    if let Some(parent) = launchd_log_file.parent() {
        std::fs::create_dir_all(parent)?;
    }

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

    // Keep launchd-owned stdio outside chamber directories. On macOS, xpcproxy
    // opens StandardOutPath/StandardErrorPath before spawning the service, and
    // TCC can deny protected locations such as ~/Documents with EX_CONFIG.
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
        log = xml_escape(&launchd_log_file.display().to_string()),
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
            launchctl_kickstart(&label)?;
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

#[cfg(target_os = "macos")]
fn launchctl_kickstart(label: &str) -> Result<()> {
    let uid = unsafe { libc::getuid() };
    let status = std::process::Command::new("launchctl")
        .args(["kickstart", "-k", &format!("gui/{uid}/{label}")])
        .status()
        .context("Failed to run launchctl")?;
    if !status.success() {
        anyhow::bail!("launchctl kickstart failed");
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

/// Restart an already installed system service without rewriting or removing
/// its service definition. Returns false if no service file is installed.
#[cfg(target_os = "macos")]
pub fn restart(label_prefix: &str, dir: &Path) -> Result<bool> {
    let label = service_label(label_prefix, dir);
    let plist_path = dirs::home_dir()
        .context("Cannot determine home directory")?
        .join("Library/LaunchAgents")
        .join(format!("{label}.plist"));

    match launchctl_restart_action(plist_path.exists(), launchctl_tracks(&label)) {
        LaunchctlRestartAction::NotInstalled => Ok(false),
        LaunchctlRestartAction::LoadExistingPlist => {
            launchctl_load_plist(&plist_path)?;
            Ok(true)
        }
        LaunchctlRestartAction::Kickstart => {
            launchctl_kickstart(&label)?;
            Ok(true)
        }
    }
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

/// Escape a value for interpolation into a systemd unit file. In unit files
/// `%` introduces a specifier (e.g. `%h`, `%i`), so a literal `%` must be
/// written as `%%`; otherwise a `%` in a path silently corrupts the unit.
#[cfg(target_os = "linux")]
fn systemd_escape_specifiers(s: &str) -> String {
    s.replace('%', "%%")
}

/// Quote a single ExecStart word. systemd splits ExecStart on whitespace, so
/// each word is double-quoted to preserve spaces, with the characters that are
/// active inside systemd's double quotes escaped: `\` and `"` are C-escaped,
/// `$` is doubled to `$$` (so it is not read as a variable reference), and `%`
/// is doubled to `%%` (specifier). Order matters — backslashes are escaped
/// first so the escapes we add below are not themselves doubled.
#[cfg(target_os = "linux")]
fn systemd_quote_exec_arg(arg: &str) -> String {
    let escaped = arg
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "$$")
        .replace('%', "%%");
    format!("\"{escaped}\"")
}

/// Build the ExecStart value: the executable followed by its arguments, each
/// quoted/escaped via `systemd_quote_exec_arg`.
#[cfg(target_os = "linux")]
fn systemd_exec_start(exe: &str, args: &[&str]) -> String {
    std::iter::once(systemd_quote_exec_arg(exe))
        .chain(args.iter().map(|a| systemd_quote_exec_arg(a)))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Build the `Environment=` line for PATH. The value is double-quoted so a
/// space in PATH cannot truncate it into a second (bogus) assignment, with `\`
/// and `"` C-escaped and `%` doubled to `%%`.
#[cfg(target_os = "linux")]
fn systemd_environment_path(path_env: &str) -> String {
    let escaped = path_env
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('%', "%%");
    format!("Environment=\"PATH={escaped}\"")
}

/// Render a systemd user unit file. Pure and escaping-aware so it can be
/// unit-tested without invoking `systemctl`. `exec_start` must already be the
/// quoted/escaped ExecStart value (see `systemd_exec_start`); `dir` and `log`
/// are `%`-escaped here, and `path_env` is quoted+escaped into an
/// `Environment=` line.
#[cfg(target_os = "linux")]
fn systemd_unit_string(
    label_prefix: &str,
    exec_start: &str,
    dir: &str,
    path_env: &str,
    restart: &str,
    log: &str,
) -> String {
    let dir = systemd_escape_specifiers(dir);
    let log = systemd_escape_specifiers(log);
    let environment = systemd_environment_path(path_env);
    format!(
        "[Unit]\n\
         Description=Cryochamber {prefix} ({dir})\n\
         \n\
         [Service]\n\
         ExecStart={exec_start}\n\
         WorkingDirectory={dir}\n\
         {environment}\n\
         Restart={restart}\n\
         StandardOutput=append:{log}\n\
         StandardError=append:{log}\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
        prefix = label_prefix,
        exec_start = exec_start,
        dir = dir,
        environment = environment,
        restart = restart,
        log = log,
    )
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

    let exec_start = systemd_exec_start(&exe.display().to_string(), args);
    let restart = if keep_alive { "always" } else { "on-failure" };

    // Capture PATH so the daemon can find agent binaries (e.g. opencode, claude).
    let path_env = std::env::var("PATH").unwrap_or_default();

    let unit = systemd_unit_string(
        label_prefix,
        &exec_start,
        &dir.display().to_string(),
        &path_env,
        restart,
        &log_file.display().to_string(),
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

    // `enable --now` starts a stopped service but does NOT restart one that was
    // already running, so a changed unit (new PATH, args, log path) would not
    // take effect until the next reboot. `try-restart` restarts it only if it
    // is currently active (a no-op otherwise), applying the new unit now.
    // Best-effort: the service is already enabled+started above, so a
    // try-restart hiccup must not fail the whole install.
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "try-restart", &label])
        .status();

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
pub fn restart(label_prefix: &str, dir: &Path) -> Result<bool> {
    let label = service_label(label_prefix, dir);
    let unit_path = dirs::home_dir()
        .context("Cannot determine home directory")?
        .join(".config/systemd/user")
        .join(format!("{label}.service"));

    if !unit_path.exists() {
        return Ok(false);
    }

    let status = std::process::Command::new("systemctl")
        .args(["--user", "restart", &label])
        .status()
        .context("Failed to run systemctl")?;
    if !status.success() {
        anyhow::bail!("systemctl restart failed");
    }
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
pub fn restart(_label_prefix: &str, _dir: &Path) -> Result<bool> {
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

#[cfg(all(test, target_os = "linux"))]
#[path = "unit_tests/service_linux.rs"]
mod linux_tests;
