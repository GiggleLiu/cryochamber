use crate::daemon::session::{format_wake_sources, is_transient_write_artifact};
use std::path::{Path, PathBuf};

#[test]
fn transient_write_artifacts_are_recognized_by_leading_dot() {
    // `message::write_message` stages inbox files as `.<name>.tmp`.
    assert!(is_transient_write_artifact(Path::new(
        "messages/inbox/.2026-08-06T15-46-57_zulip_0006.md.tmp"
    )));
    assert!(is_transient_write_artifact(Path::new(
        "messages/inbox/.swp"
    )));
    assert!(!is_transient_write_artifact(Path::new(
        "messages/inbox/2026-08-06T15-46-57_zulip_0006.md"
    )));
    assert!(!is_transient_write_artifact(Path::new("messages/inbox")));
}

#[test]
fn format_wake_sources_drops_staging_paths_but_keeps_real_ones() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("messages/inbox");
    std::fs::create_dir_all(&inbox).unwrap();
    let real = inbox.join("2026-08-06T15-46-57_zulip_0006.md");
    std::fs::write(&real, "body").unwrap();

    let sources = vec![
        inbox.join(".2026-08-06T15-46-57_zulip_0006.md.tmp"),
        real.clone(),
    ];
    let formatted = format_wake_sources(dir.path(), &sources);

    assert_eq!(
        formatted,
        vec!["messages/inbox/2026-08-06T15-46-57_zulip_0006.md".to_string()],
        "the staging path must never be named as a wake source"
    );
}

#[test]
fn format_wake_sources_returns_empty_when_only_staging_paths_seen() {
    // The caller falls back to naming the inbox directory, so an empty result
    // is the correct signal — never a dangling `.tmp` path.
    let dir = tempfile::tempdir().unwrap();
    let sources: Vec<PathBuf> = vec![dir.path().join("messages/inbox/.pending.md.tmp")];

    assert!(format_wake_sources(dir.path(), &sources).is_empty());
}

#[test]
fn format_wake_sources_dedups_repeated_paths() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("messages/inbox");
    std::fs::create_dir_all(&inbox).unwrap();
    let real = inbox.join("msg.md");
    std::fs::write(&real, "body").unwrap();

    let formatted = format_wake_sources(dir.path(), &[real.clone(), real]);
    assert_eq!(formatted.len(), 1);
}
