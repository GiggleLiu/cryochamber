use assert_cmd::Command;
use predicates::str::contains;

#[test]
fn cryohub_start_rejects_chamber_cwd_with_workspace_message() {
    let tmp = tempfile::tempdir().unwrap();
    // Simulate a chamber: a cryo.toml but no chambers/ subdir.
    let cfg = cryochamber::config::CryoConfig::default();
    cryochamber::config::save_config(&tmp.path().join("cryo.toml"), &cfg).unwrap();

    #[allow(deprecated)]
    Command::cargo_bin("cryohub")
        .unwrap()
        .current_dir(tmp.path())
        .arg("start")
        .arg("--foreground")
        .timeout(std::time::Duration::from_secs(3))
        .assert()
        .failure()
        .stderr(contains("workspace mode"));
}

#[test]
fn cryohub_status_reports_not_installed_by_default() {
    let tmp = tempfile::tempdir().unwrap();

    #[allow(deprecated)]
    Command::cargo_bin("cryohub")
        .unwrap()
        .current_dir(tmp.path())
        .arg("status")
        .assert()
        .success()
        .stdout(contains("not installed"));
}

#[test]
fn cryohub_stop_reports_nothing_when_no_service() {
    let tmp = tempfile::tempdir().unwrap();

    #[allow(deprecated)]
    Command::cargo_bin("cryohub")
        .unwrap()
        .current_dir(tmp.path())
        .arg("stop")
        .assert()
        .success()
        .stdout(contains("No cryohub service installed"));
}

#[test]
fn cryo_web_subcommand_is_gone() {
    let tmp = tempfile::tempdir().unwrap();

    #[allow(deprecated)]
    Command::cargo_bin("cryo")
        .unwrap()
        .current_dir(tmp.path())
        .arg("web")
        .assert()
        .failure();
}
