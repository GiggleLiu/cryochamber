use std::path::Path;

use anyhow::Result;

use crate::socket::{Request, Response};

/// Send a daemon request using the current daemon instance ID from `timer.json`.
pub fn send_request(dir: &Path, request: &Request) -> Result<Response> {
    let instance_id = crate::state::load_state(&crate::state::state_path(dir))
        .ok()
        .flatten()
        .and_then(|state| state.instance_id);
    crate::socket::send_request_with_instance_id(dir, request, instance_id.as_deref())
}

pub fn daemon_responding(dir: &Path) -> bool {
    matches!(send_request(dir, &Request::Ping), Ok(resp) if resp.ok)
}

/// Send SIGUSR1 to the daemon to force an immediate wake.
///
/// Returns true only when state says the daemon PID is live, the socket answers
/// for the current daemon instance, and the signal is delivered.
pub fn signal_daemon_wake(dir: &Path) -> bool {
    if let Ok(Some(st)) = crate::state::load_state(&crate::state::state_path(dir)) {
        if let Some(pid) = st.pid {
            if crate::state::is_locked(&st) && daemon_responding(dir) {
                return crate::process::send_signal(pid, libc::SIGUSR1);
            }
        }
    }
    false
}

#[cfg(test)]
#[path = "unit_tests/daemon_client.rs"]
mod tests;
