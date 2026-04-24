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
