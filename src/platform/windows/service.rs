use anyhow::{Context, Result};
use std::path::Path;
use std::ffi::OsString;
use std::time::Duration;
use windows_service::{
    define_windows_service,
    service::{
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
        ServiceType,
    },
    service_control_handler::{self, ServiceControlHandlerResult},
    service_dispatcher,
};

static mut SERVICE_DIR: Option<std::path::PathBuf> = None;

define_windows_service!(ffi_service_main, service_main);

/// Run daemon as a Windows service (called by SCM)
pub fn run_service(dir: std::path::PathBuf) -> Result<()> {
    unsafe {
        SERVICE_DIR = Some(dir);
    }
    service_dispatcher::start("cryochamber", ffi_service_main)
        .context("Failed to start service dispatcher")
}

fn service_main(_arguments: Vec<OsString>) {
    if let Err(e) = run_service_impl() {
        eprintln!("Service error: {}", e);
    }
}

fn run_service_impl() -> Result<()> {
    let dir = unsafe { SERVICE_DIR.as_ref() }
        .context("Service directory not set")?
        .clone();

    let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel();

    let event_handler = move |control_event| -> ServiceControlHandlerResult {
        match control_event {
            ServiceControl::Stop | ServiceControl::Shutdown => {
                let _ = shutdown_tx.send(());
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    };

    let status_handle = service_control_handler::register("cryochamber", event_handler)?;

    status_handle.set_service_status(ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::Running,
        controls_accepted: ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    })?;

    let daemon = crate::daemon::Daemon::new(dir);
    let daemon_handle = std::thread::spawn(move || daemon.run());

    // Wait for shutdown signal
    let _ = shutdown_rx.recv();

    status_handle.set_service_status(ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::Stopped,
        controls_accepted: ServiceControlAccept::empty(),
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    })?;

    let _ = daemon_handle.join();
    Ok(())
}


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
            &format!(
                "DisplayName=Cryochamber {} ({})",
                label_prefix,
                dir.display()
            ),
        ])
        .status()
        .context("Failed to run sc.exe create")?;
    if !status.success() {
        anyhow::bail!(
            "Failed to create Windows service (requires administrator privileges).\n\
             \n\
             To run without admin rights, use: CRYO_NO_SERVICE=1 cryo start\n\
             \n\
             Service name: {label}"
        );
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
