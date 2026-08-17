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
fn create_invite_stores_scopes_in_canonical_encoded_form() {
    // An operator may paste either the encoded index id or the plain chamber
    // path into `token create --chambers`. Both must land in the store as the
    // encoded id, which is what the chamber list and the SSE filter compare.
    let mut tf = TokenFile::default();
    let encoded = crate::hub::discovery::encode_id(std::path::Path::new("/srv/chambers/alpha"));
    let inv = tf
        .create_invite("Alice", vec!["/srv/chambers/alpha".into()])
        .unwrap();
    assert_eq!(inv.chambers, vec![encoded.clone()]);

    let inv = tf.create_invite("Bob", vec![encoded.clone()]).unwrap();
    assert_eq!(
        inv.chambers,
        vec![encoded],
        "already-canonical input is kept"
    );

    // A bare name (no path separators) is its own canonical form.
    let inv = tf.create_invite("Carol", vec!["c1".into()]).unwrap();
    assert_eq!(inv.chambers, vec!["c1".to_string()]);
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
#[cfg(unix)]
fn save_replaces_loose_file_and_stays_0600() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("tokens.json");
    // Pre-existing file with loose permissions and junk content: replacing it
    // must yield a 0600 file with valid JSON, not the old permissive mode.
    std::fs::write(&path, "this is not json").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

    let mut tf = TokenFile::default();
    tf.ensure_owner().unwrap();
    tf.create_invite("Bob", vec!["x".into()]).unwrap();
    save_tokens(&path, &tf).unwrap();

    let loaded = load_tokens(&path).unwrap();
    assert_eq!(loaded.owner, tf.owner);
    assert_eq!(loaded.invites.len(), 1);
    let mode = std::fs::metadata(&path).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
    // The temp file was renamed into place, leaving no strays behind.
    let leftovers: Vec<_> = std::fs::read_dir(tmp.path()).unwrap().collect();
    assert_eq!(leftovers.len(), 1);
}

#[test]
fn load_missing_file_yields_default() {
    let tmp = tempfile::tempdir().unwrap();
    let tf = load_tokens(&tmp.path().join("nope.json")).unwrap();
    assert!(tf.owner.is_none());
    assert!(tf.invites.is_empty());
}

/// Public mode is the default, so first start has to *mint* the owner token
/// rather than refuse. The token comes back only on the call that created it:
/// callers that are not a terminal (the service path) must have nothing to
/// print, and only `cryohub token owner` reads it out again.
#[test]
fn ensure_owner_token_creates_on_first_call_and_stays_quiet_afterwards() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tokens.json");

    let created = crate::hub::ensure_owner_token_at(&path)
        .unwrap()
        .expect("a store without an owner must yield the token it created");
    assert_eq!(created.len(), 64);
    assert!(
        path.exists(),
        "the token must be persisted before it is shown"
    );
    assert_eq!(
        load_tokens(&path).unwrap().owner.as_deref(),
        Some(created.as_str())
    );

    assert_eq!(
        crate::hub::ensure_owner_token_at(&path).unwrap(),
        None,
        "an existing owner token must never be handed back for printing"
    );
    assert_eq!(
        load_tokens(&path).unwrap().owner.as_deref(),
        Some(created.as_str()),
        "and must not be rotated"
    );
}
