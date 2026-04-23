use std::path::Path;

use anyhow::Result;

use crate::socket::{Request, Response, IPC_PROTOCOL_VERSION};

fn current_instance_id(dir: &Path) -> Option<String> {
    crate::state::load_state(&crate::state::state_path(dir))
        .ok()
        .flatten()
        .and_then(|state| state.instance_id)
}

/// Send a daemon request using the current daemon instance ID from `timer.json`.
pub fn send_request(dir: &Path, request: &Request) -> Result<Response> {
    let instance_id = current_instance_id(dir);
    crate::socket::send_request_with_instance_id(dir, request, instance_id.as_deref())
}

/// Send a daemon request after verifying the daemon speaks this client's IPC protocol.
pub fn send_checked_request(dir: &Path, request: &Request) -> Result<Response> {
    let instance_id = current_instance_id(dir);
    if !matches!(request, Request::Hello { .. }) {
        ensure_protocol_compatible(dir, instance_id.as_deref())?;
    }
    crate::socket::send_request_with_instance_id(dir, request, instance_id.as_deref())
}

fn ensure_protocol_compatible(dir: &Path, instance_id: Option<&str>) -> Result<()> {
    let hello = Request::Hello {
        protocol_version: IPC_PROTOCOL_VERSION,
    };
    let response = match crate::socket::send_request_with_instance_id(dir, &hello, instance_id) {
        Ok(response) => response,
        Err(err) if err.to_string().contains("Cannot connect to daemon socket") => return Err(err),
        Err(err) => {
            anyhow::bail!(
                "IPC protocol mismatch with running daemon. Run `cryo restart` after installing \
                 matching `cryo` and `cryo-agent` binaries. Compatibility probe failed: {err}"
            );
        }
    };

    if response.ok {
        Ok(())
    } else {
        anyhow::bail!("{}", response.message)
    }
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
