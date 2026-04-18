use assert_cmd::Command;
use predicates::str::contains;

#[test]
fn cryo_web_rejects_chamber_cwd_with_migration_message() {
    let tmp = tempfile::tempdir().unwrap();
    // Simulate a chamber: a cryo.toml but no chambers/ subdir.
    let cfg = cryochamber::config::CryoConfig::default();
    cryochamber::config::save_config(&tmp.path().join("cryo.toml"), &cfg).unwrap();

    #[allow(deprecated)]
    Command::cargo_bin("cryo")
        .unwrap()
        .current_dir(tmp.path())
        .env("CRYO_NO_SERVICE", "1")
        .arg("web")
        .arg("--foreground")
        .timeout(std::time::Duration::from_secs(3))
        .assert()
        .failure()
        .stderr(contains("workspace mode"));
}
