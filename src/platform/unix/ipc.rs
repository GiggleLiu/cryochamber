use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

use crate::socket::{Request, Response};

fn ipc_path(dir: &Path) -> PathBuf {
    dir.join(".cryo").join("cryo.sock")
}

/// Server side of IPC. Daemon creates this on startup.
pub struct IpcServer {
    listener: UnixListener,
}

/// Handle to send a response back to the client.
pub struct IpcResponder {
    stream: UnixStream,
}

impl IpcResponder {
    pub fn respond(mut self, response: &Response) -> anyhow::Result<()> {
        let mut payload = serde_json::to_string(response)?;
        payload.push('\n');
        self.stream.write_all(payload.as_bytes())?;
        self.stream.flush()?;
        Ok(())
    }
}

impl IpcServer {
    /// Bind to the IPC endpoint for the given project directory.
    /// Removes stale socket if present.
    pub fn bind(dir: &Path) -> anyhow::Result<Self> {
        let path = ipc_path(dir);
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        let listener = UnixListener::bind(&path)?;
        Ok(Self { listener })
    }

    /// Accept one connection, parse the request, return it with a responder.
    pub fn accept_one(&self) -> anyhow::Result<Option<(Request, IpcResponder)>> {
        let (stream, _) = self.listener.accept()?;
        let mut reader = BufReader::new(stream.try_clone()?);
        let mut line = String::new();
        reader.read_line(&mut line)?;
        if line.trim().is_empty() {
            return Ok(None);
        }
        let request: Request = serde_json::from_str(line.trim())?;
        Ok(Some((request, IpcResponder { stream })))
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

    /// Remove the IPC endpoint file.
    pub fn cleanup(dir: &Path) {
        let path = ipc_path(dir);
        let _ = std::fs::remove_file(path);
    }
}

/// Send a request to the daemon and return the response.
pub fn send_request(dir: &Path, request: &Request) -> anyhow::Result<Response> {
    let path = ipc_path(dir);
    let mut stream = UnixStream::connect(&path).map_err(|e| {
        anyhow::anyhow!("Cannot connect to daemon socket at {}: {e}", path.display())
    })?;

    let mut payload = serde_json::to_string(request)?;
    payload.push('\n');
    stream.write_all(payload.as_bytes())?;
    stream.flush()?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let response: Response = serde_json::from_str(line.trim())?;
    Ok(response)
}

/// Return the IPC endpoint path for a project directory.
pub fn ipc_endpoint_path(dir: &Path) -> PathBuf {
    ipc_path(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ipc_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".cryo")).unwrap();

        let server = IpcServer::bind(dir.path()).unwrap();
        server.set_nonblocking(false).unwrap();

        let request = Request::Note {
            text: "test".into(),
        };
        let dir_path = dir.path().to_owned();
        let handle = std::thread::spawn(move || {
            send_request(&dir_path, &request).unwrap()
        });

        let (req, responder) = server.accept_one().unwrap().unwrap();
        assert!(matches!(req, Request::Note { .. }));
        responder
            .respond(&Response {
                ok: true,
                message: "ok".into(),
            })
            .unwrap();

        let response = handle.join().unwrap();
        assert!(response.ok);
        assert_eq!(response.message, "ok");
    }

    #[test]
    fn test_ipc_endpoint_path() {
        let dir = std::path::Path::new("/tmp/test-cryo");
        let path = ipc_endpoint_path(dir);
        assert!(path.ends_with("cryo.sock"));
        assert!(path.to_str().unwrap().contains(".cryo"));
    }
}
