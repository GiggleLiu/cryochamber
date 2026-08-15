use crate::hub::tokens::*;

#[test]
fn generate_token_is_64_hex_and_unique() {
    let a = generate_token().unwrap();
    let b = generate_token().unwrap();
    assert_eq!(a.len(), 64);
    assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    assert_ne!(a, b);
}

#[test]
fn resolve_owner_invite_revoked_and_unknown() {
    let mut tf = TokenFile::default();
    let owner = tf.ensure_owner().unwrap();
    let inv = tf
        .create_invite("Alice", vec!["autoresearch".into()])
        .unwrap();
    assert_eq!(tf.resolve(&owner), Some(Role::Owner));
    assert_eq!(
        tf.resolve(&inv.token),
        Some(Role::Invite {
            name: "Alice".into(),
            chambers: vec!["autoresearch".into()]
        })
    );
    assert_eq!(tf.resolve("deadbeef"), None);
    assert!(tf.revoke("Alice"));
    assert_eq!(
        tf.resolve(&inv.token),
        None,
        "revoked token must not resolve"
    );
    // tombstone, not deletion
    assert!(tf.invites[0].revoked_at.is_some());
    assert!(!tf.revoke("Alice"), "second revoke is a no-op");
}

#[test]
fn ensure_owner_is_idempotent() {
    let mut tf = TokenFile::default();
    let a = tf.ensure_owner().unwrap();
    let b = tf.ensure_owner().unwrap();
    assert_eq!(a, b);
}

#[test]
fn duplicate_invite_name_is_rejected() {
    let mut tf = TokenFile::default();
    tf.create_invite("Alice", vec![]).unwrap();
    assert!(tf.create_invite("Alice", vec![]).is_err());
}

#[test]
fn save_load_roundtrip_with_0600() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("tokens.json");
    let mut tf = TokenFile::default();
    tf.ensure_owner().unwrap();
    tf.create_invite("Bob", vec!["x".into()]).unwrap();
    save_tokens(&path, &tf).unwrap();
    let loaded = load_tokens(&path).unwrap();
    assert_eq!(loaded.owner, tf.owner);
    assert_eq!(loaded.invites.len(), 1);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}

#[test]
fn load_missing_file_yields_default() {
    let tmp = tempfile::tempdir().unwrap();
    let tf = load_tokens(&tmp.path().join("nope.json")).unwrap();
    assert!(tf.owner.is_none());
    assert!(tf.invites.is_empty());
}
