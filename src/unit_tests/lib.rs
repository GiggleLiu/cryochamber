use crate::test_support::EnvVarGuard;

#[test]
fn test_work_dir_prefers_chamber_dir_env_over_cwd() {
    let dir = tempfile::tempdir().unwrap();
    let _guard = EnvVarGuard::set_path(crate::CHAMBER_DIR_ENV, dir.path());
    let resolved = crate::work_dir().unwrap();
    assert_eq!(
        resolved,
        dir.path().canonicalize().unwrap(),
        "CRYO_CHAMBER_DIR must win over the current directory"
    );
}

#[test]
fn test_ensure_chamber_dir_rejects_non_chamber_with_actionable_message() {
    let dir = tempfile::tempdir().unwrap();
    let err = crate::ensure_chamber_dir(dir.path())
        .unwrap_err()
        .to_string();
    // The old failure mode surfaced as "Daemon instance mismatch" or a
    // connect error against the wrong chamber — a red herring that cost real
    // agent sessions. The error must name the actual problem and both fixes.
    assert!(err.contains("cryo.toml"), "{err}");
    assert!(err.contains("CRYO_CHAMBER_DIR"), "{err}");

    std::fs::write(dir.path().join("cryo.toml"), "agent = \"opencode\"\n").unwrap();
    assert!(crate::ensure_chamber_dir(dir.path()).is_ok());
}
