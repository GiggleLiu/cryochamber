#[test]
fn sync_service_uses_crash_only_restart_policy() {
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/bin/cryo_gh.rs"),
    )
    .unwrap();
    let start = source
        .find("cryochamber::service::install(\n        \"gh-sync\",")
        .expect("gh sync service install call should exist");
    let snippet = &source[start..source[start..].find(")?;").unwrap() + start];

    assert!(
        snippet.contains("false,\n    "),
        "sync Halt exits cleanly, so the service must not use always-restart: {snippet}"
    );
    assert!(
        !snippet.contains("true,\n    "),
        "always-restart would respawn after a clean Halt: {snippet}"
    );
}
