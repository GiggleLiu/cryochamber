use super::*;
use crate::socket::{Request, Response, SocketServer};
use crate::state::{save_state, CryoState};
use std::sync::mpsc;

fn state_with_instance(instance_id: Option<&str>) -> CryoState {
    CryoState {
        session_number: 1,
        pid: None,
        retry_count: 0,
        agent_override: None,
        max_retries_override: None,
        max_session_duration_override: None,
        last_report_time: None,
        provider_index: None,
        instance_id: instance_id.map(str::to_string),
        pending_fallback: None,
        in_flight_fallback: None,
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
