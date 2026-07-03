use super::*;

#[test]
fn write_file_if_missing_writes_new_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("example.txt");

    let wrote = write_file_if_missing(&path, "hello").unwrap();

    assert!(wrote);
    assert_eq!(std::fs::read_to_string(path).unwrap(), "hello");
}

#[test]
fn write_file_if_missing_skips_existing_file_without_clobbering() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("example.txt");
    std::fs::write(&path, "existing").unwrap();

    let wrote = write_file_if_missing(&path, "replacement").unwrap();

    assert!(!wrote);
    assert_eq!(std::fs::read_to_string(path).unwrap(), "existing");
}

#[test]
fn scaffold_chamber_creates_all_artifacts_in_empty_dir() {
    let dir = tempfile::tempdir().unwrap();
    let report = crate::protocol::scaffold_chamber(dir.path(), "opencode").unwrap();
    assert!(report.cryo_toml_created);
    assert!(report.plan_created);
    assert!(report.readme_created);
    assert!(report.notes_created);
    assert!(report.gitignore_created);

    for f in [
        "cryo.toml",
        "plan.md",
        "README.md",
        "NOTES.md",
        ".gitignore",
    ] {
        assert!(dir.path().join(f).exists(), "missing {f}");
    }
    // The credential/socket dir must be gitignored so `.cryo/zuliprc` and the
    // IPC socket are never committed.
    let gitignore = std::fs::read_to_string(dir.path().join(".gitignore")).unwrap();
    assert!(
        gitignore.lines().any(|l| l.trim() == ".cryo/"),
        ".gitignore must ignore .cryo/: {gitignore}"
    );
    assert!(!dir.path().join("AGENTS.md").exists());
    assert!(!dir.path().join("CLAUDE.md").exists());
    assert!(dir.path().join("messages/inbox").exists());
    assert!(dir.path().join("messages/outbox").exists());
    assert!(dir.path().join("messages/inbox/archive").exists());
}

#[test]
fn scaffold_chamber_with_claude_does_not_write_agent_protocol_file() {
    let dir = tempfile::tempdir().unwrap();
    crate::protocol::scaffold_chamber(dir.path(), "claude").unwrap();
    assert!(!dir.path().join("CLAUDE.md").exists());
    assert!(!dir.path().join("AGENTS.md").exists());
}

#[test]
fn scaffold_chamber_idempotent_on_existing_files() {
    let dir = tempfile::tempdir().unwrap();
    crate::protocol::scaffold_chamber(dir.path(), "opencode").unwrap();
    let report = crate::protocol::scaffold_chamber(dir.path(), "opencode").unwrap();
    assert!(!report.cryo_toml_created);
    assert!(!report.plan_created);
    assert!(!report.readme_created);
    assert!(!report.notes_created);
    assert!(!report.gitignore_created);
}
