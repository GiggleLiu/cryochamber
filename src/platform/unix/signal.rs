use anyhow::{Context, Result};
use signal_hook::consts::{SIGINT, SIGTERM, SIGUSR1};
use signal_hook::flag;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

/// Register SIGTERM and SIGINT handlers on a shared shutdown flag.
pub fn register_shutdown_handler(shutdown: Arc<AtomicBool>) -> Result<()> {
    flag::register(SIGTERM, Arc::clone(&shutdown))
        .context("Failed to register SIGTERM handler")?;
    flag::register(SIGINT, Arc::clone(&shutdown))
        .context("Failed to register SIGINT handler")?;
    Ok(())
}

/// Register SIGUSR1 handler on a shared wake flag.
pub fn register_wake_handler(wake: Arc<AtomicBool>) -> Result<()> {
    flag::register(SIGUSR1, Arc::clone(&wake)).context("Failed to register SIGUSR1 handler")?;
    Ok(())
}

/// Send SIGUSR1 to the daemon for the given project directory.
pub fn signal_wake(dir: &Path) -> bool {
    if let Ok(Some(st)) = crate::state::load_state(&crate::state::state_path(dir)) {
        if let Some(pid) = st.pid {
            if crate::state::is_locked(&st) {
                return crate::platform::process::send_signal(pid, SIGUSR1);
            }
        }
    }
    false
}

/// Spawn a thread that polls signal flags and forwards events to the channel.
pub fn spawn_signal_forwarder<E: Send + 'static>(
    shutdown: Arc<AtomicBool>,
    wake: Arc<AtomicBool>,
    tx: mpsc::Sender<E>,
    shutdown_event: E,
    wake_event: impl Fn() -> E + Send + 'static,
    _dir: &Path,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(250));
        if shutdown.load(Ordering::Relaxed) {
            let _ = tx.send(shutdown_event);
            break;
        }
        if wake.swap(false, Ordering::Relaxed) {
            let _ = tx.send(wake_event());
        }
    })
}
