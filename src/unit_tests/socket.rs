use super::*;
use crate::state::{save_state, CryoState};

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
    let result = send_request(dir.path(), &Request::Ping);
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
        if let Some((req, responder)) = server.accept_one(None).unwrap() {
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
    let state = CryoState {
        session_number: 1,
        pid: None,
        retry_count: 0,
        agent_override: None,
        max_retries_override: None,
        max_session_duration_override: None,
        last_report_time: None,
        provider_index: None,
        instance_id: Some("instance-123".to_string()),
        pending_fallback: None,
        in_flight_fallback: None,
        previous_session_crashed: false,
    };
    save_state(&dir.path().join("timer.json"), &state).unwrap();
    let resp = send_request(
        dir.path(),
        &Request::Reply {
            text: "hello".into(),
        },
    )
    .unwrap();
    assert!(resp.ok);
    assert_eq!(resp.message, "got it");

    // Server received the request
    let received = rx.recv().unwrap();
    assert!(matches!(received, Request::Reply { text } if text == "hello"));

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

    let result = server.accept_one(None).unwrap();
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

    let result = server.accept_one(None);
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
            // Reply request with an extra unknown field
            let json = r#"{"cmd":"reply","text":"hello","unknown_field":42}"#;
            stream.write_all(json.as_bytes()).unwrap();
            stream.write_all(b"\n").unwrap();
            stream.flush().unwrap();
            // Read response
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
        }
    });

    let result = server.accept_one(None);
    // serde ignores unknown fields by default (no deny_unknown_fields set)
    match result {
        Ok(Some((req, responder))) => {
            assert!(matches!(req, Request::Reply { text } if text == "hello"));
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
fn test_accept_one_rejects_mismatched_instance_id() {
    let dir = tempfile::tempdir().unwrap();
    let sock_path = dir.path().join("test.sock");
    let server = SocketServer::bind(&sock_path).unwrap();
    server.set_nonblocking(false).unwrap();

    let handle = std::thread::spawn({
        let sock_path = sock_path.clone();
        move || {
            let mut stream = std::os::unix::net::UnixStream::connect(&sock_path).unwrap();
            use std::io::{BufRead, BufReader, Write};
            let json = r#"{"instance_id":"wrong-instance","cmd":"ping"}"#;
            stream.write_all(json.as_bytes()).unwrap();
            stream.write_all(b"\n").unwrap();
            stream.flush().unwrap();

            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            line
        }
    });

    let result = server.accept_one(Some("expected-instance")).unwrap();
    assert!(
        result.is_none(),
        "Mismatched instance should be rejected before reaching the daemon"
    );

    let response_line = handle.join().unwrap();
    let response: Response = serde_json::from_str(response_line.trim()).unwrap();
    assert!(!response.ok);
    assert!(response.message.contains("instance"));
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
