use super::*;

#[test]
fn test_daemon_entry_has_socket_path() {
    let entry = DaemonEntry {
        pid: 1234,
        dir: "/tmp/test".to_string(),
        socket_path: Some("/tmp/test/.cryo/cryo.sock".to_string()),
    };
    let json = serde_json::to_string(&entry).unwrap();
    assert!(json.contains("cryo.sock"));
}
