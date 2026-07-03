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
fn test_serialize_response_ok() {
    let resp = Response {
        ok: true,
        message: "Hibernating".to_string(),
    };
    let json = serde_json::to_string(&resp).unwrap();
    assert!(json.contains("true"));
}

#[test]
fn test_serialize_hello_request() {
    let req = Request::Hello {
        protocol_version: IPC_PROTOCOL_VERSION,
    };
    let json = serde_json::to_string(&req).unwrap();
    assert!(json.contains("\"cmd\":\"hello\""));
    assert!(json.contains("\"protocol_version\":"));
}

#[test]
fn test_deserialize_send_request() {
    let json = r#"{"cmd":"send","text":"status update"}"#;
    let parsed = serde_json::from_str::<Request>(json);
    assert!(
        parsed.is_ok(),
        "socket request enum must accept a dedicated send command"
    );
}

#[test]
fn test_serialize_receive_request() {
    let req = Request::Receive;
    let json = serde_json::to_string(&req).unwrap();
    assert!(json.contains("\"cmd\":\"receive\""));
    let parsed: Request = serde_json::from_str(&json).unwrap();
    assert!(matches!(parsed, Request::Receive));
}

#[test]
fn dialog_request_round_trip_last_n() {
    use crate::socket::{DialogFilter, Request};

    let req = Request::Dialog {
        filter: DialogFilter::LastN { count: 20 },
    };
    let s = serde_json::to_string(&req).unwrap();
    let back: Request = serde_json::from_str(&s).unwrap();
    assert_eq!(format!("{back:?}"), format!("{req:?}"));
    assert!(s.contains("\"cmd\":\"dialog\""));
    assert!(s.contains("\"filter\""));
}

#[test]
fn dialog_filter_round_trip_variants() {
    use crate::socket::DialogFilter;

    for f in [
        DialogFilter::LastN { count: 5 },
        DialogFilter::All,
        DialogFilter::Since {
            iso: "2026-04-25T09:00".to_string(),
        },
    ] {
        let s = serde_json::to_string(&f).unwrap();
        let back: DialogFilter = serde_json::from_str(&s).unwrap();
        assert_eq!(format!("{back:?}"), format!("{f:?}"));
    }
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
fn test_send_request_with_instance_id_uses_explicit_instance_not_state() {
    let dir = tempfile::tempdir().unwrap();
    let sock = socket_path(dir.path());
    std::fs::create_dir_all(sock.parent().unwrap()).unwrap();
    std::fs::write(
        dir.path().join("timer.json"),
        r#"{"session_number":1,"pid":null,"instance_id":"state-instance"}"#,
    )
    .unwrap();

    let server = SocketServer::bind(&sock).unwrap();
    let handle = std::thread::spawn(move || {
        let accepted = server.accept_one(Some("explicit-instance")).unwrap();
        match accepted {
            Some((Request::Ping, responder)) => {
                responder
                    .respond(&Response {
                        ok: true,
                        message: "pong".into(),
                    })
                    .unwrap();
            }
            Some((_, _)) => panic!("expected ping request"),
            None => panic!("expected request"),
        }
    });

    let resp = send_request_with_instance_id(dir.path(), &Request::Ping, Some("explicit-instance"))
        .unwrap();
    assert!(resp.ok);
    assert_eq!(resp.message, "pong");
    handle.join().unwrap();
}

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

    let resp = send_request(
        dir.path(),
        &Request::Send {
            text: "hello".into(),
            question: false,
        },
    )
    .unwrap();
    assert!(resp.ok);
    assert_eq!(resp.message, "got it");

    // Server received the request
    let received = rx.recv().unwrap();
    assert!(matches!(received, Request::Send { text, .. } if text == "hello"));

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
            // Send request with an extra unknown field
            let json = r#"{"cmd":"send","text":"hello","unknown_field":42}"#;
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
            assert!(matches!(req, Request::Send { text, .. } if text == "hello"));
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

#[test]
fn test_accept_one_times_out_on_silent_client() {
    use std::time::{Duration, Instant};

    let dir = tempfile::tempdir().unwrap();
    let sock_path = dir.path().join("test.sock");
    let server = SocketServer::bind(&sock_path).unwrap();
    server.set_nonblocking(false).unwrap();

    // Client connects but never sends a newline. It holds the connection open
    // (parking on the channel) until the server has returned, exercising the
    // read-timeout path rather than an EOF.
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let handle = std::thread::spawn({
        let sock_path = sock_path.clone();
        move || {
            let _stream = std::os::unix::net::UnixStream::connect(&sock_path).unwrap();
            release_rx.recv().ok();
        }
    });

    let start = Instant::now();
    let result = server
        .accept_one_with_timeout(None, Duration::from_millis(200))
        .unwrap();
    let elapsed = start.elapsed();

    assert!(
        result.is_none(),
        "silent client should yield None once the read times out"
    );
    assert!(
        elapsed < Duration::from_secs(3),
        "accept_one should return shortly after the read timeout, took {elapsed:?}"
    );

    release_tx.send(()).ok();
    handle.join().unwrap();
}

#[test]
fn test_accept_one_large_request_round_trips() {
    // A valid request several buffers wide must round-trip. This proves the
    // blocking-with-timeout accepted stream (the macOS non-blocking-inherit
    // path) reads a multi-chunk line correctly.
    let dir = tempfile::tempdir().unwrap();
    let sock = socket_path(dir.path());
    std::fs::create_dir_all(sock.parent().unwrap()).unwrap();

    let server = SocketServer::bind(&sock).unwrap();
    let big_text = "x".repeat(64 * 1024);
    let expected_len = big_text.len();

    let handle = std::thread::spawn(move || match server.accept_one(None).unwrap() {
        Some((Request::Send { text, .. }, responder)) => {
            responder
                .respond(&Response {
                    ok: true,
                    message: format!("len={}", text.len()),
                })
                .unwrap();
        }
        Some((other, _)) => panic!("expected send request, got {other:?}"),
        None => panic!("expected a request"),
    });

    let resp = send_request(
        dir.path(),
        &Request::Send {
            text: big_text,
            question: false,
        },
    )
    .unwrap();
    assert!(resp.ok);
    assert_eq!(resp.message, format!("len={expected_len}"));

    handle.join().unwrap();
}
