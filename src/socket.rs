use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const IPC_PROTOCOL_VERSION: u32 = 6;

/// Request from CLI to daemon via Unix socket.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    Ping,
    Hello {
        protocol_version: u32,
    },
    Hibernate {
        complete: bool,
        exit_code: u8,
        summary: Option<String>,
    },
    Send {
        text: String,
        #[serde(default)]
        question: bool,
    },
    Receive,
    TodoAdd {
        text: String,
        at: String,
    },
    TodoDone {
        id: u32,
    },
    TodoRemove {
        id: u32,
    },
    TodoList,
}

/// Response from daemon to CLI.
#[derive(Debug, Serialize, Deserialize)]
pub struct Response {
    pub ok: bool,
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct SocketEnvelope {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    instance_id: Option<String>,
    #[serde(flatten)]
    request: Request,
}

/// Returns the socket path for a project directory.
pub fn socket_path(dir: &Path) -> PathBuf {
    dir.join(".cryo").join("cryo.sock")
}

/// Send a request to the daemon and return the response.
pub fn send_request(dir: &Path, request: &Request) -> anyhow::Result<Response> {
    send_request_with_instance_id(dir, request, None)
}

/// Send a request to the daemon with an explicit daemon instance ID.
pub fn send_request_with_instance_id(
    dir: &Path,
    request: &Request,
    instance_id: Option<&str>,
) -> anyhow::Result<Response> {
    let path = socket_path(dir);
    let mut stream = UnixStream::connect(&path).map_err(|e| {
        anyhow::anyhow!("Cannot connect to daemon socket at {}: {e}", path.display())
    })?;

    let envelope = SocketEnvelope {
        instance_id: instance_id.map(str::to_string),
        request: request.clone(),
    };

    let mut payload = serde_json::to_string(&envelope)?;
    payload.push('\n');
    stream.write_all(payload.as_bytes())?;
    stream.flush()?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let response: Response = serde_json::from_str(line.trim())?;
    Ok(response)
}

/// Server side of the Unix socket. Daemon creates this on startup.
pub struct SocketServer {
    listener: UnixListener,
}

/// Handle to send a response back to the client.
pub struct Responder {
    stream: UnixStream,
}

impl Responder {
    pub fn respond(mut self, response: &Response) -> anyhow::Result<()> {
        let mut payload = serde_json::to_string(response)?;
        payload.push('\n');
        self.stream.write_all(payload.as_bytes())?;
        self.stream.flush()?;
        Ok(())
    }
}

impl SocketServer {
    /// Bind to the given socket path. Removes stale socket if present.
    pub fn bind(path: &Path) -> anyhow::Result<Self> {
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        let listener = UnixListener::bind(path)?;
        Ok(Self { listener })
    }

    /// Accept one connection, parse the request, return it with a responder.
    pub fn accept_one(
        &self,
        expected_instance_id: Option<&str>,
    ) -> anyhow::Result<Option<(Request, Responder)>> {
        let (stream, _) = self.listener.accept()?;
        let mut reader = BufReader::new(stream.try_clone()?);
        let mut line = String::new();
        reader.read_line(&mut line)?;
        if line.trim().is_empty() {
            return Ok(None);
        }
        let envelope: SocketEnvelope = serde_json::from_str(line.trim())?;
        let responder = Responder { stream };
        if let Some(expected) = expected_instance_id {
            if envelope.instance_id.as_deref() != Some(expected) {
                responder.respond(&Response {
                    ok: false,
                    message: "Daemon instance mismatch. The state file is stale; reload and retry."
                        .to_string(),
                })?;
                return Ok(None);
            }
        }
        Ok(Some((envelope.request, responder)))
    }

    /// Set the listener to non-blocking mode.
    pub fn set_nonblocking(&self, nonblocking: bool) -> anyhow::Result<()> {
        self.listener.set_nonblocking(nonblocking)?;
        Ok(())
    }

    /// Get a reference to the raw listener (for polling in daemon event loop).
    pub fn listener(&self) -> &UnixListener {
        &self.listener
    }

    /// Remove the socket file.
    pub fn cleanup(path: &Path) {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
#[path = "unit_tests/socket.rs"]
mod tests;
