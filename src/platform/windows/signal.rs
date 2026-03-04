use anyhow::Result;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, OnceLock};
use std::time::Duration;
use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::System::Threading::{
    CreateEventW, OpenEventW, SetEvent, WaitForSingleObject, EVENT_MODIFY_STATE,
    WAIT_OBJECT_0,
};

/// Global shutdown flag, set by the console ctrl handler.
static SHUTDOWN_FLAG: OnceLock<Arc<AtomicBool>> = OnceLock::new();

/// Console ctrl handler callback (called by Windows on Ctrl+C, service stop, etc.).
unsafe extern "system" fn ctrl_handler(ctrl_type: u32) -> i32 {
    // CTRL_C_EVENT = 0, CTRL_BREAK_EVENT = 1, CTRL_CLOSE_EVENT = 2
    if ctrl_type <= 2 {
        if let Some(flag) = SHUTDOWN_FLAG.get() {
            flag.store(true, Ordering::Relaxed);
        }
        1 // handled
    } else {
        0 // not handled
    }
}

/// Register Ctrl+C / service stop handler on a shared shutdown flag.
pub fn register_shutdown_handler(shutdown: Arc<AtomicBool>) -> Result<()> {
    SHUTDOWN_FLAG
        .set(shutdown)
        .map_err(|_| anyhow::anyhow!("Shutdown handler already registered"))?;
    unsafe {
        use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;
        if SetConsoleCtrlHandler(Some(ctrl_handler), 1) == 0 {
            anyhow::bail!(
                "SetConsoleCtrlHandler failed: {}",
                std::io::Error::last_os_error()
            );
        }
    }
    Ok(())
}

/// No-op on Windows — wake is handled via named events, not a flag.
pub fn register_wake_handler(_wake: Arc<AtomicBool>) -> Result<()> {
    Ok(())
}

fn event_name(dir: &Path) -> Vec<u16> {
    let hash = crate::service::dir_hash(dir);
    let name = format!("Local\\cryo-wake-{hash}\0");
    name.encode_utf16().collect()
}

/// Send a wake signal to the daemon for the given project directory.
pub fn signal_wake(dir: &Path) -> bool {
    let name = event_name(dir);
    unsafe {
        let handle = OpenEventW(EVENT_MODIFY_STATE, 0, name.as_ptr());
        if handle == 0 {
            return false;
        }
        let ok = SetEvent(handle);
        CloseHandle(handle);
        ok != 0
    }
}

/// Spawn a thread that polls the shutdown flag and waits on a named event for wake.
pub fn spawn_signal_forwarder<E: Send + 'static>(
    shutdown: Arc<AtomicBool>,
    _wake: Arc<AtomicBool>,
    tx: mpsc::Sender<E>,
    shutdown_event: E,
    wake_event: impl Fn() -> E + Send + 'static,
    dir: &Path,
) -> std::thread::JoinHandle<()> {
    let name = event_name(dir);
    std::thread::spawn(move || {
        // Create a named event for wake signaling
        let event_handle = unsafe {
            CreateEventW(std::ptr::null(), 0, 0, name.as_ptr())
        };
        if event_handle == 0 {
            eprintln!(
                "Warning: failed to create wake event: {}",
                std::io::Error::last_os_error()
            );
            // Fall back to polling-only mode
            loop {
                std::thread::sleep(Duration::from_millis(250));
                if shutdown.load(Ordering::Relaxed) {
                    let _ = tx.send(shutdown_event);
                    break;
                }
            }
            return;
        }

        loop {
            // Wait on the event with a 250ms timeout
            let result = unsafe { WaitForSingleObject(event_handle, 250) };
            if shutdown.load(Ordering::Relaxed) {
                let _ = tx.send(shutdown_event);
                break;
            }
            if result == WAIT_OBJECT_0 {
                let _ = tx.send(wake_event());
            }
        }

        unsafe {
            CloseHandle(event_handle);
        }
    })
}
