use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Request from CLI to daemon via Unix socket.
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

/// Returns the socket path for a project directory.
pub fn socket_path(dir: &Path) -> PathBuf {
    dir.join(".cryo").join("cryo.sock")
}

/// Send a request to the daemon and return the response.
pub fn send_request(dir: &Path, request: &Request) -> anyhow::Result<Response> {
    let path = socket_path(dir);
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
        assert!(path.ends_with("cryo.sock"));
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

        // Spawn server handler in a thread
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

        // Client sends a request
        let resp = send_request(
            dir.path(),
            &Request::Note {
                text: "hello".into(),
            },
        )
        .unwrap();
        assert!(resp.ok);
        assert_eq!(resp.message, "got it");

        // Server received the request
        let received = rx.recv().unwrap();
        assert!(matches!(received, Request::Note { .. }));

        handle.join().unwrap();
    }

    #[test]
    fn test_accept_empty_line() {
        let dir = tempfile::tempdir().unwrap();
        let sock_path = dir.path().join("test.sock");
        let server = SocketServer::bind(&sock_path).unwrap();
        server.set_nonblocking(false).unwrap();

        let handle = std::thread::spawn({
            let sock_path = sock_path.clone();
            move || {
                let mut stream = std::os::unix::net::UnixStream::connect(&sock_path).unwrap();
                use std::io::Write;
                stream.write_all(b"\n").unwrap();
                stream.flush().unwrap();
            }
        });

        let result = server.accept_one().unwrap();
        assert!(result.is_none(), "Empty line should return None");
        handle.join().unwrap();
    }

    #[test]
    fn test_accept_malformed_json() {
        let dir = tempfile::tempdir().unwrap();
        let sock_path = dir.path().join("test.sock");
        let server = SocketServer::bind(&sock_path).unwrap();
        server.set_nonblocking(false).unwrap();

        let handle = std::thread::spawn({
            let sock_path = sock_path.clone();
            move || {
                let mut stream = std::os::unix::net::UnixStream::connect(&sock_path).unwrap();
                use std::io::Write;
                stream.write_all(b"{not json\n").unwrap();
                stream.flush().unwrap();
            }
        });

        let result = server.accept_one();
        assert!(result.is_err(), "Malformed JSON should return error");
        handle.join().unwrap();
    }

    #[test]
    fn test_accept_unknown_fields_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let sock_path = dir.path().join("test.sock");
        let server = SocketServer::bind(&sock_path).unwrap();
        server.set_nonblocking(false).unwrap();

        let handle = std::thread::spawn({
            let sock_path = sock_path.clone();
            move || {
                let mut stream = std::os::unix::net::UnixStream::connect(&sock_path).unwrap();
                use std::io::{BufRead, BufReader, Write};
                // Note request with an extra unknown field
                let json = r#"{"cmd":"note","text":"hello","unknown_field":42}"#;
                stream.write_all(json.as_bytes()).unwrap();
                stream.write_all(b"\n").unwrap();
                stream.flush().unwrap();
                // Read response
                let mut reader = BufReader::new(stream);
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
            }
        });

        let result = server.accept_one();
        // serde ignores unknown fields by default (no deny_unknown_fields set)
        match result {
            Ok(Some((req, responder))) => {
                assert!(matches!(req, Request::Note { text } if text == "hello"));
                responder
                    .respond(&Response {
                        ok: true,
                        message: "ok".to_string(),
                    })
                    .unwrap();
            }
            Ok(None) => panic!("Should not return None for valid JSON with extra fields"),
            Err(e) => panic!("Should not error for unknown fields: {e}"),
        }
        handle.join().unwrap();
    }
}
