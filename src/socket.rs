// src/socket.rs
//! IPC between cryo-agent and the daemon.
//!
//! - Unix: Unix domain socket at `.cryo/cryo.sock`
//! - Windows: TCP loopback; port is stored in `.cryo/cryo.port`

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Request from CLI to daemon.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    Hibernate {
        wake: Option<String>,
        complete: bool,
        exit_code: u8,
        summary: Option<String>,
    },
    Note {
        text: String,
    },
    Alert {
        action: String,
        target: String,
        message: String,
    },
    Reply {
        text: String,
    },
}

/// Response from daemon to CLI.
#[derive(Debug, Serialize, Deserialize)]
pub struct Response {
    pub ok: bool,
    pub message: String,
}

// ── Path helpers ─────────────────────────────────────────────────────────────

/// Returns the Unix socket path (Unix) or the port-file path (Windows).
pub fn socket_path(dir: &Path) -> PathBuf {
    #[cfg(unix)]
    {
        dir.join(".cryo").join("cryo.sock")
    }
    #[cfg(windows)]
    {
        dir.join(".cryo").join("cryo.port")
    }
}

// ── Platform-specific implementation ─────────────────────────────────────────

#[cfg(unix)]
mod imp {
    use super::*;
    use std::os::unix::net::{UnixListener, UnixStream};

    pub struct SocketServer {
        pub(super) listener: UnixListener,
    }

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
        pub fn bind(path: &Path) -> anyhow::Result<Self> {
            if path.exists() {
                std::fs::remove_file(path)?;
            }
            let listener = UnixListener::bind(path)?;
            Ok(Self { listener })
        }

        pub fn accept_one(&self) -> anyhow::Result<Option<(Request, Responder)>> {
            let (stream, _) = self.listener.accept()?;
            let mut reader = BufReader::new(stream.try_clone()?);
            let mut line = String::new();
            reader.read_line(&mut line)?;
            if line.trim().is_empty() {
                return Ok(None);
            }
            let request: Request = serde_json::from_str(line.trim())?;
            Ok(Some((request, Responder { stream })))
        }

        pub fn set_nonblocking(&self, nonblocking: bool) -> anyhow::Result<()> {
            self.listener.set_nonblocking(nonblocking)?;
            Ok(())
        }

        pub fn cleanup(path: &Path) {
            let _ = std::fs::remove_file(path);
        }
    }

    pub fn send_request(dir: &Path, request: &Request) -> anyhow::Result<Response> {
        let path = super::socket_path(dir);
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
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::net::{TcpListener, TcpStream};

    pub struct SocketServer {
        pub(super) listener: TcpListener,
        _port_file: std::path::PathBuf,
    }

    pub struct Responder {
        stream: TcpStream,
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
        pub fn bind(port_file: &Path) -> anyhow::Result<Self> {
            // Bind on a random free port
            let listener = TcpListener::bind("127.0.0.1:0")?;
            let port = listener.local_addr()?.port();
            // Write port number to the port file so clients can connect
            if let Some(parent) = port_file.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(port_file, port.to_string())?;
            Ok(Self {
                listener,
                _port_file: port_file.to_path_buf(),
            })
        }

        pub fn accept_one(&self) -> anyhow::Result<Option<(Request, Responder)>> {
            let (stream, _) = self.listener.accept()?;
            let mut reader = BufReader::new(stream.try_clone()?);
            let mut line = String::new();
            reader.read_line(&mut line)?;
            if line.trim().is_empty() {
                return Ok(None);
            }
            let request: Request = serde_json::from_str(line.trim())?;
            Ok(Some((request, Responder { stream })))
        }

        pub fn set_nonblocking(&self, nonblocking: bool) -> anyhow::Result<()> {
            self.listener.set_nonblocking(nonblocking)?;
            Ok(())
        }

        pub fn cleanup(port_file: &Path) {
            let _ = std::fs::remove_file(port_file);
        }
    }

    pub fn send_request(dir: &Path, request: &Request) -> anyhow::Result<Response> {
        let port_file = super::socket_path(dir);
        let port_str = std::fs::read_to_string(&port_file).map_err(|e| {
            anyhow::anyhow!(
                "Cannot read daemon port file at {}: {e}. Is the daemon running?",
                port_file.display()
            )
        })?;
        let port: u16 = port_str.trim().parse().map_err(|_| {
            anyhow::anyhow!("Invalid port in {}: '{}'", port_file.display(), port_str.trim())
        })?;

        let mut stream = TcpStream::connect(("127.0.0.1", port)).map_err(|e| {
            anyhow::anyhow!("Cannot connect to daemon on port {port}: {e}")
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
}

// ── Public re-exports ─────────────────────────────────────────────────────────

pub use imp::{Responder, SocketServer};

/// Send a request to the daemon and return the response.
pub fn send_request(dir: &Path, request: &Request) -> anyhow::Result<Response> {
    imp::send_request(dir, request)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_hibernate_request() {
        let req = Request::Hibernate {
            wake: Some("2026-03-08T09:00".to_string()),
            complete: false,
            exit_code: 0,
            summary: Some("Done".to_string()),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: Request = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, Request::Hibernate { .. }));
    }

    #[test]
    fn test_serialize_note_request() {
        let req = Request::Note {
            text: "progress update".to_string(),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("progress update"));
    }

    #[test]
    fn test_serialize_response_ok() {
        let resp = Response {
            ok: true,
            message: "Hibernating".to_string(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("true"));
    }

    #[test]
    fn test_serialize_alert_request() {
        let req = Request::Alert {
            action: "email".to_string(),
            target: "user@example.com".to_string(),
            message: "stuck".to_string(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: Request = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, Request::Alert { .. }));
    }

    #[test]
    fn test_serialize_reply_request() {
        let req = Request::Reply {
            text: "done with phase 1".to_string(),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("done with phase 1"));
    }

    #[test]
    fn test_socket_path() {
        let dir = std::path::Path::new("/tmp/test-cryo");
        let path = socket_path(dir);
        assert!(path.to_str().unwrap().contains(".cryo"));
    }

    #[test]
    fn test_send_request_no_server() {
        let dir = tempfile::tempdir().unwrap();
        let result = send_request(dir.path(), &Request::Note { text: "hi".into() });
        assert!(result.is_err()); // no server listening
    }

    use std::sync::mpsc;

    #[test]
    fn test_socket_server_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let sock = socket_path(dir.path());
        std::fs::create_dir_all(sock.parent().unwrap()).unwrap();

        let (tx, rx) = mpsc::channel();
        let server = SocketServer::bind(&sock).unwrap();

        let handle = std::thread::spawn(move || {
            if let Some((req, responder)) = server.accept_one().unwrap() {
                tx.send(req).unwrap();
                responder
                    .respond(&Response {
                        ok: true,
                        message: "got it".into(),
                    })
                    .unwrap();
            }
        });

        let resp = send_request(
            dir.path(),
            &Request::Note {
                text: "hello".into(),
            },
        )
        .unwrap();
        assert!(resp.ok);
        assert_eq!(resp.message, "got it");

        let received = rx.recv().unwrap();
        assert!(matches!(received, Request::Note { .. }));

        handle.join().unwrap();
    }
}
