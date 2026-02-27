use cryochamber::channel::zulip::ZulipClient;

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
