use cryochamber::channel::zulip::{
    parse_get_messages_response, parse_get_profile_response, parse_get_stream_id_response,
    ZulipClient,
};

#[test]
fn test_parse_zuliprc() {
    let dir = tempfile::tempdir().unwrap();
    let rc_path = dir.path().join("zuliprc");
    std::fs::write(
        &rc_path,
        "[api]\nemail=bot@example.com\nkey=abc123secret\nsite=https://zulip.example.com\n",
    )
    .unwrap();

    let client = ZulipClient::from_zuliprc(&rc_path).unwrap();
    let creds = client.credentials();
    assert_eq!(creds.email, "bot@example.com");
    assert_eq!(creds.api_key, "abc123secret");
    assert_eq!(creds.site, "https://zulip.example.com");
}

#[test]
fn test_parse_zuliprc_with_spaces() {
    let dir = tempfile::tempdir().unwrap();
    let rc_path = dir.path().join("zuliprc");
    std::fs::write(
        &rc_path,
        "[api]\nemail = bot@example.com\nkey = abc123\nsite = https://zulip.example.com\n",
    )
    .unwrap();

    let client = ZulipClient::from_zuliprc(&rc_path).unwrap();
    let creds = client.credentials();
    assert_eq!(creds.email, "bot@example.com");
    assert_eq!(creds.api_key, "abc123");
}

#[test]
fn test_parse_zuliprc_missing_field() {
    let dir = tempfile::tempdir().unwrap();
    let rc_path = dir.path().join("zuliprc");
    std::fs::write(&rc_path, "[api]\nemail=bot@example.com\n").unwrap();

    let result = ZulipClient::from_zuliprc(&rc_path);
    assert!(result.is_err());
}

#[test]
fn test_parse_get_profile_response() {
    let json = serde_json::json!({
        "result": "success",
        "msg": "",
        "user_id": 42,
        "email": "bot@example.com",
        "full_name": "Test Bot"
    });
    let (user_id, email) = parse_get_profile_response(&json).unwrap();
    assert_eq!(user_id, 42);
    assert_eq!(email, "bot@example.com");
}

#[test]
fn test_parse_get_stream_id_response() {
    let json = serde_json::json!({
        "result": "success",
        "msg": "",
        "stream_id": 15
    });
    let stream_id = parse_get_stream_id_response(&json).unwrap();
    assert_eq!(stream_id, 15);
}

#[test]
fn test_parse_get_messages_response() {
    let json = serde_json::json!({
        "result": "success",
        "msg": "",
        "messages": [
            {
                "id": 100,
                "sender_id": 42,
                "sender_email": "alice@example.com",
                "sender_full_name": "Alice",
                "content": "Hello from Zulip",
                "subject": "general-topic",
                "timestamp": 1740700000,
                "type": "stream"
            },
            {
                "id": 101,
                "sender_id": 43,
                "sender_email": "bot@example.com",
                "sender_full_name": "Bot",
                "content": "I am the bot",
                "subject": "general-topic",
                "timestamp": 1740700060,
                "type": "stream"
            }
        ],
        "found_newest": true,
        "found_oldest": false
    });
    let (messages, found_newest) =
        parse_get_messages_response(&json, Some("bot@example.com")).unwrap();
    // Should filter out bot's own message
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].from, "Alice");
    assert_eq!(messages[0].body, "Hello from Zulip");
    assert_eq!(
        messages[0].metadata.get("source"),
        Some(&"zulip".to_string())
    );
    assert_eq!(
        messages[0].metadata.get("zulip_message_id"),
        Some(&"100".to_string())
    );
    assert!(found_newest);
}

#[test]
fn test_parse_get_messages_response_empty() {
    let json = serde_json::json!({
        "result": "success",
        "msg": "",
        "messages": [],
        "found_newest": true,
        "found_oldest": true
    });
    let (messages, found_newest) = parse_get_messages_response(&json, None).unwrap();
    assert!(messages.is_empty());
    assert!(found_newest);
}

#[test]
fn test_parse_get_messages_no_self_filter() {
    let json = serde_json::json!({
        "result": "success",
        "msg": "",
        "messages": [
            {
                "id": 100,
                "sender_id": 42,
                "sender_email": "alice@example.com",
                "sender_full_name": "Alice",
                "content": "Hello",
                "subject": "topic",
                "timestamp": 1740700000,
                "type": "stream"
            }
        ],
        "found_newest": false,
        "found_oldest": false
    });
    let (messages, found_newest) = parse_get_messages_response(&json, None).unwrap();
    assert_eq!(messages.len(), 1);
    assert!(!found_newest);
}
