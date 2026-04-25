use super::*;
use crate::socket::{Request, Response, SocketServer};
use crate::state::{save_state, CryoState};
use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixListener;
use std::sync::mpsc;

fn state_with_instance(instance_id: Option<&str>) -> CryoState {
    CryoState {
        session_number: 1,
        pid: None,
        agent_override: None,
        max_session_duration_override: None,
        last_report_time: None,
        instance_id: instance_id.map(str::to_string),
        session_active: false,
        previous_session_crashed: false,
    }
}

#[test]
fn test_send_request_loads_instance_id_from_state() {
    let dir = tempfile::tempdir().unwrap();
    let sock = crate::socket::socket_path(dir.path());
    std::fs::create_dir_all(sock.parent().unwrap()).unwrap();
    save_state(
        &crate::state::state_path(dir.path()),
        &state_with_instance(Some("state-instance")),
    )
    .unwrap();

    let (tx, rx) = mpsc::channel();
    let server = SocketServer::bind(&sock).unwrap();
    let handle = std::thread::spawn(move || {
        let (request, responder) = server
            .accept_one(Some("state-instance"))
            .unwrap()
            .expect("request should pass instance check");
        tx.send(request).unwrap();
        responder
            .respond(&Response {
                ok: true,
                message: "pong".into(),
            })
            .unwrap();
    });

    let resp = send_request(dir.path(), &Request::Ping).unwrap();
    assert!(resp.ok);
    assert_eq!(resp.message, "pong");
    assert!(matches!(rx.recv().unwrap(), Request::Ping));

    handle.join().unwrap();
}

#[test]
fn test_daemon_responding_uses_state_backed_client() {
    let dir = tempfile::tempdir().unwrap();
    let sock = crate::socket::socket_path(dir.path());
    std::fs::create_dir_all(sock.parent().unwrap()).unwrap();
    save_state(
        &crate::state::state_path(dir.path()),
        &state_with_instance(Some("responding-instance")),
    )
    .unwrap();

    let server = SocketServer::bind(&sock).unwrap();
    let handle = std::thread::spawn(move || {
        let (request, responder) = server
            .accept_one(Some("responding-instance"))
            .unwrap()
            .expect("request should pass instance check");
        assert!(matches!(request, Request::Ping));
        responder
            .respond(&Response {
                ok: true,
                message: "pong".into(),
            })
            .unwrap();
    });

    assert!(daemon_responding(dir.path()));
    handle.join().unwrap();
}

#[test]
fn test_send_checked_request_reports_protocol_mismatch_when_probe_gets_eof() {
    let dir = tempfile::tempdir().unwrap();
    let sock = crate::socket::socket_path(dir.path());
    std::fs::create_dir_all(sock.parent().unwrap()).unwrap();

    save_state(
        &crate::state::state_path(dir.path()),
        &state_with_instance(Some("old-daemon")),
    )
    .unwrap();

    let listener = UnixListener::bind(&sock).unwrap();
    let handle = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert!(line.contains("\"cmd\":\"hello\""), "got {line:?}");
        assert!(
            line.contains("\"instance_id\":\"old-daemon\""),
            "got {line:?}"
        );
        // Simulate an older daemon that cannot deserialize the hello request:
        // it logs an accept error and closes without writing a response.
    });

    let err = send_checked_request(dir.path(), &Request::Ping)
        .unwrap_err()
        .to_string();
    assert!(err.contains("IPC protocol mismatch"), "{err}");
    assert!(err.contains("cryo restart"), "{err}");
    handle.join().unwrap();
}

#[test]
fn test_send_checked_request_reports_protocol_mismatch_response() {
    let dir = tempfile::tempdir().unwrap();
    let sock = crate::socket::socket_path(dir.path());
    std::fs::create_dir_all(sock.parent().unwrap()).unwrap();

    save_state(
        &crate::state::state_path(dir.path()),
        &state_with_instance(Some("old-daemon")),
    )
    .unwrap();

    let server = SocketServer::bind(&sock).unwrap();
    let handle = std::thread::spawn(move || {
        let (request, responder) = server
            .accept_one(Some("old-daemon"))
            .unwrap()
            .expect("hello request should pass instance check");
        assert!(matches!(request, Request::Hello { .. }));
        responder
            .respond(&Response {
                ok: false,
                message: "IPC protocol mismatch: daemon uses 4, client uses 5. Run `cryo restart`."
                    .into(),
            })
            .unwrap();
    });

    let err = send_checked_request(dir.path(), &Request::Ping)
        .unwrap_err()
        .to_string();
    assert!(err.contains("IPC protocol mismatch"), "{err}");
    assert!(err.contains("cryo restart"), "{err}");
    handle.join().unwrap();
}
