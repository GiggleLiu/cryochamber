use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

pub const IPC_PROTOCOL_VERSION: u32 = 9;

/// Read/write timeout applied to each accepted client connection. Bounds how
/// long a single request poll may block on a slow or silent client so the
/// single-threaded daemon never wedges on one connection.
const ACCEPT_IO_TIMEOUT: Duration = Duration::from_secs(5);

/// Upper bound on the bytes read for one request line. A client that streams
/// without ever sending a newline cannot grow daemon memory without bound;
/// once the cap is hit the connection is dropped if the bytes don't parse.
const MAX_REQUEST_BYTES: u64 = 1024 * 1024;

/// Filter for `cryo-agent dialog`. Matches the CLI flags
/// `--last N` (default 20), `--all`, `--since <iso>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DialogFilter {
    LastN { count: u32 },
    All,
    Since { iso: String },
}

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
    Dialog {
        filter: DialogFilter,
    },
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
    /// Best-effort liveness probe: true when the peer has closed its end of
    /// the connection. A parked `cryo-agent hibernate` client blocks on
    /// its response and never writes again, so a readable stream that yields
    /// EOF (or a hard error) means the client process is gone. `WouldBlock`
    /// means the peer is alive with nothing to say. Errors arming the probe
    /// report "alive" — the safe default, since a false "gone" would drop a
    /// live client's response.
    pub fn peer_disconnected(&self) -> bool {
        use std::os::fd::AsRawFd;
        let mut buf = [0u8; 1];
        // MSG_PEEK: don't consume; MSG_DONTWAIT: never block, regardless of
        // the stream's blocking mode. (`UnixStream::peek` is still unstable.)
        let n = unsafe {
            libc::recv(
                self.stream.as_raw_fd(),
                buf.as_mut_ptr() as *mut libc::c_void,
                1,
                libc::MSG_PEEK | libc::MSG_DONTWAIT,
            )
        };
        match n {
            0 => true,    // orderly EOF: peer closed
            1.. => false, // stray bytes: peer alive
            _ => {
                let err = std::io::Error::last_os_error();
                !matches!(
                    err.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                )
            }
        }
    }

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
        self.accept_one_with_timeout(expected_instance_id, ACCEPT_IO_TIMEOUT)
    }

    /// Like [`accept_one`], but with an explicit per-connection read/write
    /// timeout. Exposed for tests that cannot afford the production default.
    pub fn accept_one_with_timeout(
        &self,
        expected_instance_id: Option<&str>,
        io_timeout: Duration,
    ) -> anyhow::Result<Option<(Request, Responder)>> {
        // Let the listener's own WouldBlock propagate: callers rely on it to
        // mean "no pending connection".
        let (stream, _) = self.listener.accept()?;

        // On macOS the accepted stream inherits the listener's O_NONBLOCK, so a
        // client whose bytes lag the accept would surface WouldBlock and the
        // request would be silently dropped. On Linux the accepted stream is
        // blocking with no timeout, so a client that connects and never sends a
        // newline wedges the whole single-threaded daemon. Force blocking with
        // a bounded timeout on both. Socket options are shared across the
        // `try_clone` below, and the write timeout also governs `respond`.
        stream.set_nonblocking(false)?;
        // Setting SO_RCVTIMEO/SO_SNDTIMEO can fail with EINVAL on macOS when the
        // peer has already closed the connection. That only happens for clients
        // that hang up without waiting for a reply; the read below then returns
        // promptly (buffered bytes or EOF) rather than wedging, so a failure to
        // arm the timeout here is best-effort and safe to ignore. On a live peer
        // both calls succeed and the timeout is active.
        let _ = stream.set_read_timeout(Some(io_timeout));
        let _ = stream.set_write_timeout(Some(io_timeout));

        // Cap the request line so a client streaming without a newline can't
        // grow memory unbounded.
        let mut reader = BufReader::new(stream.try_clone()?).take(MAX_REQUEST_BYTES);
        let mut line = String::new();
        if let Err(e) = reader.read_line(&mut line) {
            if e.kind() == std::io::ErrorKind::WouldBlock
                || e.kind() == std::io::ErrorKind::TimedOut
            {
                // Read timed out: no complete request this poll — drop the
                // connection instead of propagating an error.
                return Ok(None);
            }
            return Err(e.into());
        }
        if line.trim().is_empty() {
            return Ok(None);
        }
        let envelope: SocketEnvelope = match serde_json::from_str(line.trim()) {
            Ok(envelope) => envelope,
            Err(e) => {
                // If the size cap was hit (no newline within
                // MAX_REQUEST_BYTES), a parse failure means a truncated line —
                // drop the connection rather than surfacing a hard error.
                if reader.limit() == 0 {
                    return Ok(None);
                }
                return Err(e.into());
            }
        };
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
