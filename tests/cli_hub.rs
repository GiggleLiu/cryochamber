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
fn cryohub_start_rejects_non_workspace_dir() {
    // An empty dir with neither cryo.toml nor chambers/ is not a workspace.
    let tmp = tempfile::tempdir().unwrap();

    #[allow(deprecated)]
    Command::cargo_bin("cryohub")
        .unwrap()
        .current_dir(tmp.path())
        .arg("start")
        .arg("--foreground")
        .timeout(std::time::Duration::from_secs(3))
        .assert()
        .failure()
        .stderr(contains("needs a workspace"));
}

#[test]
fn cryohub_status_reports_not_installed_by_default() {
    let tmp = tempfile::tempdir().unwrap();
    // Isolate HOME so any unit/plist sitting in the real home isn't picked up.
    let fake_home = tempfile::tempdir().unwrap();

    #[allow(deprecated)]
    Command::cargo_bin("cryohub")
        .unwrap()
        .current_dir(tmp.path())
        .env("HOME", fake_home.path())
        .arg("status")
        .assert()
        .success()
        .stdout(contains("not installed"));
}

#[test]
fn cryohub_status_reports_installed_when_unit_exists() {
    // Exercises the installed branch of cmd_status by planting a unit file at
    // the path service::is_installed looks for, using an isolated HOME.
    let tmp = tempfile::tempdir().unwrap();
    let fake_home = tempfile::tempdir().unwrap();
    let workspace = tmp.path().canonicalize().unwrap();

    let label = cryochamber::service::service_label("hub", &workspace);

    #[cfg(target_os = "macos")]
    let unit_path = {
        let agents = fake_home.path().join("Library/LaunchAgents");
        std::fs::create_dir_all(&agents).unwrap();
        agents.join(format!("{label}.plist"))
    };
    #[cfg(target_os = "linux")]
    let unit_path = {
        let units = fake_home.path().join(".config/systemd/user");
        std::fs::create_dir_all(&units).unwrap();
        units.join(format!("{label}.service"))
    };
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let unit_path: std::path::PathBuf = {
        // is_installed is hard-coded to false on other platforms; skip.
        return;
    };

    std::fs::write(&unit_path, "stub").unwrap();

    #[allow(deprecated)]
    Command::cargo_bin("cryohub")
        .unwrap()
        .current_dir(&workspace)
        .env("HOME", fake_home.path())
        .arg("status")
        .assert()
        .success()
        .stdout(contains("installed"))
        .stdout(contains(&label));
}

#[test]
fn cryohub_stop_reports_nothing_when_no_service() {
    let tmp = tempfile::tempdir().unwrap();
    let fake_home = tempfile::tempdir().unwrap();

    #[allow(deprecated)]
    Command::cargo_bin("cryohub")
        .unwrap()
        .current_dir(tmp.path())
        .env("HOME", fake_home.path())
        .arg("stop")
        .assert()
        .success()
        .stdout(contains("No cryohub service installed"));
}

#[test]
fn cryo_web_subcommand_is_gone() {
    let tmp = tempfile::tempdir().unwrap();

    // Clap reports unknown subcommands as "unrecognized subcommand".
    #[allow(deprecated)]
    Command::cargo_bin("cryo")
        .unwrap()
        .current_dir(tmp.path())
        .arg("web")
        .assert()
        .failure()
        .stderr(contains("unrecognized subcommand"));
}
