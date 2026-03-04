use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Request from CLI to daemon via IPC.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    Hibernate {
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

// --- Delegate to platform layer ---
pub use crate::platform::ipc::{send_request, IpcResponder, IpcServer};

/// Backwards-compatible type aliases.
pub type SocketServer = IpcServer;
pub type Responder = IpcResponder;

/// Returns the IPC endpoint path for a project directory.
pub fn socket_path(dir: &Path) -> PathBuf {
    crate::platform::ipc::ipc_endpoint_path(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_hibernate_request() {
        let req = Request::Hibernate {
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

    #[cfg(unix)]
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
        std::fs::create_dir_all(dir.path().join(".cryo")).unwrap();

        let (tx, rx) = mpsc::channel();
        let server = SocketServer::bind(dir.path()).unwrap();

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

    #[cfg(unix)]
    #[test]
    fn test_accept_empty_line() {
        use std::os::unix::net::UnixStream;

        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".cryo")).unwrap();
        let server = SocketServer::bind(dir.path()).unwrap();
        server.set_nonblocking(false).unwrap();

        let sock_path = socket_path(dir.path());
        let handle = std::thread::spawn(move || {
            let mut stream = UnixStream::connect(&sock_path).unwrap();
            use std::io::Write;
            stream.write_all(b"\n").unwrap();
            stream.flush().unwrap();
        });

        let result = server.accept_one().unwrap();
        assert!(result.is_none(), "Empty line should return None");
        handle.join().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_accept_malformed_json() {
        use std::os::unix::net::UnixStream;

        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".cryo")).unwrap();
        let server = SocketServer::bind(dir.path()).unwrap();
        server.set_nonblocking(false).unwrap();

        let sock_path = socket_path(dir.path());
        let handle = std::thread::spawn(move || {
            let mut stream = UnixStream::connect(&sock_path).unwrap();
            use std::io::Write;
            stream.write_all(b"{not json\n").unwrap();
            stream.flush().unwrap();
        });

        let result = server.accept_one();
        assert!(result.is_err(), "Malformed JSON should return error");
        handle.join().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_accept_unknown_fields_ignored() {
        use std::os::unix::net::UnixStream;

        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".cryo")).unwrap();
        let server = SocketServer::bind(dir.path()).unwrap();
        server.set_nonblocking(false).unwrap();

        let sock_path = socket_path(dir.path());
        let handle = std::thread::spawn(move || {
            let mut stream = UnixStream::connect(&sock_path).unwrap();
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

    #[test]
    fn test_todo_add_request_serialization() {
        let req = Request::TodoAdd {
            text: "Check CI".into(),
            at: "2026-03-02T14:00".into(),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"cmd\":\"todo_add\""));
        assert!(json.contains("\"text\":\"Check CI\""));
        assert!(json.contains("\"at\":\"2026-03-02T14:00\""));
    }

    #[test]
    fn test_todo_done_request_serialization() {
        let req = Request::TodoDone { id: 3 };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"cmd\":\"todo_done\""));
        assert!(json.contains("\"id\":3"));
    }

    #[test]
    fn test_todo_remove_request_serialization() {
        let req = Request::TodoRemove { id: 5 };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"cmd\":\"todo_remove\""));
    }

    #[test]
    fn test_todo_list_request_serialization() {
        let req = Request::TodoList;
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"cmd\":\"todo_list\""));
    }
}
