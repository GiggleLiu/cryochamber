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
fn protocol_file_matching_is_case_insensitive_for_direct_callers() {
    assert!(ProtocolFile::Claude.matches_executable("Claude-Code"));
    assert!(ProtocolFile::Claude.matches_executable("/usr/bin/Claude"));
    assert!(ProtocolFile::Agents.matches_executable("AnyAgent"));
}

#[test]
fn scaffold_chamber_creates_all_artifacts_in_empty_dir() {
    let dir = tempfile::tempdir().unwrap();
    let report = crate::protocol::scaffold_chamber(dir.path(), "opencode").unwrap();
    assert!(report.cryo_toml_created);
    assert!(report.protocol_created);
    assert!(report.plan_created);
    assert!(report.readme_created);
    assert!(report.notes_created);
    assert_eq!(report.protocol_filename, "AGENTS.md");

    for f in ["cryo.toml", "AGENTS.md", "plan.md", "README.md", "NOTES.md"] {
        assert!(dir.path().join(f).exists(), "missing {f}");
    }
    assert!(dir.path().join("messages/inbox").exists());
    assert!(dir.path().join("messages/outbox").exists());
    assert!(dir.path().join("messages/inbox/archive").exists());
}

#[test]
fn scaffold_chamber_with_claude_writes_claude_md() {
    let dir = tempfile::tempdir().unwrap();
    let report = crate::protocol::scaffold_chamber(dir.path(), "claude").unwrap();
    assert_eq!(report.protocol_filename, "CLAUDE.md");
    assert!(dir.path().join("CLAUDE.md").exists());
    assert!(!dir.path().join("AGENTS.md").exists());
}

#[test]
fn scaffold_chamber_idempotent_on_existing_files() {
    let dir = tempfile::tempdir().unwrap();
    crate::protocol::scaffold_chamber(dir.path(), "opencode").unwrap();
    let report = crate::protocol::scaffold_chamber(dir.path(), "opencode").unwrap();
    assert!(!report.cryo_toml_created);
    assert!(!report.protocol_created);
    assert!(!report.plan_created);
    assert!(!report.readme_created);
    assert!(!report.notes_created);
}
