//! Integration tests for the Zulip HTTP client methods using a minimal
//! in-process HTTP mock server (built on `std::net::TcpListener`).
//!
//! No external mock libraries are needed — the server speaks bare HTTP/1.1.

use cryochamber::channel::zulip::ZulipClient;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;

// ── minimal mock HTTP server ─────────────────────────────────────────────────

/// A one-shot mock server that returns the same JSON body for every request it
/// handles. Runs in a background thread; cleans up when dropped.
struct MockServer {
    port: u16,
    _handle: std::thread::JoinHandle<()>,
}

impl MockServer {
    /// Start the server.  `max_requests` controls how many connections the
    /// background thread will accept before stopping (use 1 for simple tests,
    /// more for tests that trigger multiple requests like pagination).
    fn start(response_json: Arc<String>, max_requests: usize) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let handle = std::thread::spawn(move || {
            let mut count = 0;
            for stream in listener.incoming() {
                if count >= max_requests {
                    break;
                }
                match stream {
                    Ok(s) => {
                        serve_response(s, &response_json);
                        count += 1;
                    }
                    Err(_) => break,
                }
            }
        });

        Self {
            port,
            _handle: handle,
        }
    }

    fn port(&self) -> u16 {
        self.port
    }
}

/// Read one HTTP request from `stream` and write back a 200 JSON response.
fn serve_response(mut stream: TcpStream, body: &str) {
    // Read headers (stop at the first blank line \r\n\r\n)
    let mut buf = vec![0u8; 16_384];
    let mut read = 0;
    loop {
        match stream.read(&mut buf[read..]) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                read += n;
                if buf[..read].windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
                if read == buf.len() {
                    break;
                }
            }
        }
    }
    // We deliberately ignore the request body (fine for our tests).
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

// ── test helpers ─────────────────────────────────────────────────────────────

/// Create a ZulipClient whose `site` points at the local mock server.
/// Returns both the client and the tempdir (which must stay alive).
fn make_client(port: u16) -> (ZulipClient, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let rc_path = dir.path().join("zuliprc");
    std::fs::write(
        &rc_path,
        format!("[api]\nemail=bot@example.com\nkey=fakekey\nsite=http://127.0.0.1:{port}\n"),
    )
    .unwrap();
    let client = ZulipClient::from_zuliprc(&rc_path).unwrap();
    (client, dir)
}

// ── send_message ──────────────────────────────────────────────────────────────

#[test]
fn test_send_message_success() {
    let resp = Arc::new(r#"{"result":"success","id":999,"msg":""}"#.to_string());
    let server = MockServer::start(resp, 1);

    let (client, _dir) = make_client(server.port());
    let msg_id = client
        .send_message(1, "cryochamber", "hello from test")
        .expect("send_message should succeed");
    assert_eq!(msg_id, 999);
}

#[test]
fn test_send_message_api_error_returns_err() {
    let resp = Arc::new(r#"{"result":"error","msg":"Stream not found","id":0}"#.to_string());
    let server = MockServer::start(resp, 1);

    let (client, _dir) = make_client(server.port());
    let result = client.send_message(1, "cryochamber", "oops");
    assert!(result.is_err());
    assert!(
        result.unwrap_err().to_string().contains("Stream not found"),
        "Error should mention the API message"
    );
}

// ── pull_messages ─────────────────────────────────────────────────────────────

#[test]
fn test_pull_messages_empty_stream() {
    let resp =
        Arc::new(r#"{"result":"success","messages":[],"found_newest":true,"msg":""}"#.to_string());
    let server = MockServer::start(resp, 1);

    let dir = tempfile::tempdir().unwrap();
    let (client, _cr_dir) = make_client(server.port());

    let newest_id = client
        .pull_messages(1, None, None, dir.path())
        .expect("pull_messages should succeed");

    assert!(
        newest_id.is_none(),
        "Empty stream should yield None for newest_id"
    );

    // Inbox directory should be empty (or non-existent)
    let inbox = dir.path().join("messages/inbox");
    if inbox.exists() {
        let md: Vec<_> = std::fs::read_dir(&inbox)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|x| x == "md"))
            .collect();
        assert!(
            md.is_empty(),
            "No messages should be written to inbox for empty stream"
        );
    }
}

#[test]
fn test_pull_messages_self_filtering() {
    let resp = Arc::new(
        serde_json::json!({
            "result": "success",
            "found_newest": true,
            "msg": "",
            "messages": [
                {
                    "id": 10,
                    "sender_email": "bot@example.com",
                    "sender_full_name": "Bot",
                    "subject": "dev",
                    "content": "My own message — should be filtered"
                },
                {
                    "id": 11,
                    "sender_email": "human@example.com",
                    "sender_full_name": "Human",
                    "subject": "dev",
                    "content": "Hello agent"
                }
            ]
        })
        .to_string(),
    );
    let server = MockServer::start(resp, 1);

    let dir = tempfile::tempdir().unwrap();
    let (client, _cr_dir) = make_client(server.port());

    let newest_id = client
        .pull_messages(1, None, Some("bot@example.com"), dir.path())
        .expect("pull_messages should succeed");

    // raw_max_id must advance to 11 (the highest message ID, even if filtered)
    assert_eq!(newest_id, Some(11));

    // Only the human's message should be in inbox
    let inbox = dir.path().join("messages/inbox");
    let md: Vec<_> = std::fs::read_dir(&inbox)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "md"))
        .collect();
    assert_eq!(md.len(), 1, "Only the non-self message should reach inbox");

    let content = std::fs::read_to_string(md[0].path()).unwrap();
    assert!(
        content.contains("Hello agent"),
        "Inbox should contain human message body"
    );
}

#[test]
fn test_pull_messages_raw_max_id_advances_even_when_all_filtered() {
    // Only self messages — none should reach inbox, but newest_id must advance.
    let resp = Arc::new(
        serde_json::json!({
            "result": "success",
            "found_newest": true,
            "msg": "",
            "messages": [{
                "id": 20,
                "sender_email": "bot@example.com",
                "sender_full_name": "Bot",
                "subject": "dev",
                "content": "Only me here"
            }]
        })
        .to_string(),
    );
    let server = MockServer::start(resp, 1);

    let dir = tempfile::tempdir().unwrap();
    let (client, _cr_dir) = make_client(server.port());

    let newest_id = client
        .pull_messages(1, None, Some("bot@example.com"), dir.path())
        .expect("pull_messages should succeed");

    assert_eq!(
        newest_id,
        Some(20),
        "raw_max_id should be 20 even when all messages are filtered"
    );

    // Inbox should have no messages
    let inbox = dir.path().join("messages/inbox");
    if inbox.exists() {
        let md: Vec<_> = std::fs::read_dir(&inbox)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|x| x == "md"))
            .collect();
        assert!(md.is_empty(), "Filtered messages must not reach inbox");
    }
}
