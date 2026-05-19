use assert_cmd::Command;
use predicates::str::contains;

#[test]
fn cryohub_start_help_describes_global_hub() {
    let tmp = tempfile::tempdir().unwrap();

    #[allow(deprecated)]
    Command::cargo_bin("cryohub")
        .unwrap()
        .current_dir(tmp.path())
        .arg("start")
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("global"));
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
fn cryohub_status_reports_global_service_from_any_directory() {
    let cwd = tempfile::tempdir().unwrap();
    let other_cwd = tempfile::tempdir().unwrap();
    let fake_home = tempfile::tempdir().unwrap();
    let fake_state = tempfile::tempdir().unwrap();
    let hub_dir = cryochamber::hub::paths::hub_service_dir_for_state_home(fake_state.path());
    let label = cryochamber::service::service_label("hub", &hub_dir);

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
        return;
    };

    std::fs::write(&unit_path, "stub").unwrap();

    for dir in [cwd.path(), other_cwd.path()] {
        #[allow(deprecated)]
        Command::cargo_bin("cryohub")
            .unwrap()
            .current_dir(dir)
            .env("HOME", fake_home.path())
            .env("XDG_STATE_HOME", fake_state.path())
            .arg("status")
            .assert()
            .success()
            .stdout(contains("installed"))
            .stdout(contains(&label));
    }
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
        .stdout(contains("No global cryohub service installed"));
}

#[test]
fn cryohub_restart_reports_nothing_when_no_service() {
    let tmp = tempfile::tempdir().unwrap();
    let fake_home = tempfile::tempdir().unwrap();

    #[allow(deprecated)]
    Command::cargo_bin("cryohub")
        .unwrap()
        .current_dir(tmp.path())
        .env("HOME", fake_home.path())
        .arg("restart")
        .assert()
        .success()
        .stdout(contains("No global cryohub service installed"));
}

#[test]
fn cryohub_status_lists_other_installed_services_anchored_elsewhere() {
    // Install a fake hub service for /some/other/dir, then run `cryohub status`
    // from a different cwd and confirm the other service is surfaced. This is
    // the cwd-mismatch mitigation: users who started from a different dir can
    // still find their service.
    let cwd = tempfile::tempdir().unwrap();
    let fake_home = tempfile::tempdir().unwrap();
    let other_dir = std::path::PathBuf::from("/tmp/cryohub-test-other-dir-xyz");

    let other_label = cryochamber::service::service_label("hub", &other_dir);

    #[cfg(target_os = "macos")]
    let (unit_path, unit_body) = {
        let agents = fake_home.path().join("Library/LaunchAgents");
        std::fs::create_dir_all(&agents).unwrap();
        let body = format!(
            "<plist><dict><key>WorkingDirectory</key><string>{}</string></dict></plist>",
            other_dir.display()
        );
        (agents.join(format!("{other_label}.plist")), body)
    };
    #[cfg(target_os = "linux")]
    let (unit_path, unit_body) = {
        let units = fake_home.path().join(".config/systemd/user");
        std::fs::create_dir_all(&units).unwrap();
        let body = format!(
            "[Service]\nExecStart=/x\nWorkingDirectory={}\n",
            other_dir.display()
        );
        (units.join(format!("{other_label}.service")), body)
    };
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let (unit_path, unit_body): (std::path::PathBuf, String) = {
        return; // list_installed returns empty on unsupported platforms.
    };

    std::fs::write(&unit_path, unit_body).unwrap();

    #[allow(deprecated)]
    Command::cargo_bin("cryohub")
        .unwrap()
        .current_dir(cwd.path())
        .env("HOME", fake_home.path())
        .arg("status")
        .assert()
        .success()
        .stdout(contains("Legacy cwd-scoped cryohub services installed"))
        .stdout(contains(&other_label))
        .stdout(contains(other_dir.to_str().unwrap()));
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
