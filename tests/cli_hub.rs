use assert_cmd::Command;
use predicates::prelude::*;
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

/// Run `cryohub` with config + home redirected into `home`, so token
/// subcommands never touch the developer's real `~/.config/cryo`.
/// `default_tokens_path()` routes through `config_root()`, which honours
/// `XDG_CONFIG_HOME` on every platform — see `src/hub/paths.rs`. Per-command
/// `.env()` (not `std::env::set_var`) keeps this safe under the parallel test
/// harness without any global lock.
#[allow(deprecated)]
fn cryohub_in(home: &std::path::Path, args: &[&str]) -> assert_cmd::assert::Assert {
    let mut cmd = Command::cargo_bin("cryohub").unwrap();
    cmd.current_dir(home)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_STATE_HOME", home.join("state"))
        .args(args)
        .assert()
}

#[test]
fn token_owner_create_list_revoke_roundtrip() {
    let home = tempfile::tempdir().unwrap();
    let hex64 = predicates::str::is_match("^[0-9a-f]{64}$").unwrap();

    // `token owner` is create-if-absent, so it is idempotent: two runs must
    // print the *same* secret, otherwise every invocation would silently lock
    // out whoever is holding the previous one.
    let first = cryohub_in(home.path(), &["token", "owner"]).success();
    let owner = String::from_utf8(first.get_output().stdout.clone())
        .unwrap()
        .trim()
        .to_string();
    assert!(
        hex64.eval(&owner),
        "expected a 64-hex owner token, got {owner:?}"
    );

    let second = cryohub_in(home.path(), &["token", "owner"]).success();
    let owner_again = String::from_utf8(second.get_output().stdout.clone())
        .unwrap()
        .trim()
        .to_string();
    assert_eq!(owner, owner_again, "`token owner` must be idempotent");

    // The store landed under the redirected config root, not the real one.
    let tokens_path = home.path().join("config/cryo/cryohub-tokens.json");
    assert!(
        tokens_path.exists(),
        "tokens file should be at {}",
        tokens_path.display()
    );

    // create
    let created = cryohub_in(
        home.path(),
        &["token", "create", "--name", "Alice", "--chambers", "c1,c2"],
    )
    .success()
    .stdout(contains("#invite="));
    let created_out = String::from_utf8(created.get_output().stdout.clone()).unwrap();
    let alice = created_out
        .lines()
        .find_map(|l| l.strip_prefix("token: "))
        .expect("create should print the token once")
        .trim()
        .to_string();
    assert!(
        hex64.eval(&alice),
        "expected a 64-hex invite token, got {alice:?}"
    );

    // Duplicate active name is refused.
    cryohub_in(
        home.path(),
        &["token", "create", "--name", "Alice", "--chambers", "c1"],
    )
    .failure();

    // list: names and scopes, never secrets.
    cryohub_in(home.path(), &["token", "list"])
        .success()
        .stdout(contains("Alice"))
        .stdout(contains("c1,c2"))
        .stdout(contains("active"))
        .stdout(contains(alice.as_str()).not());

    // revoke, then confirm the tombstone is visible and a second revoke fails.
    cryohub_in(home.path(), &["token", "revoke", "Alice"]).success();
    cryohub_in(home.path(), &["token", "list"])
        .success()
        .stdout(contains("revoked"))
        .stdout(contains(alice.as_str()).not());
    cryohub_in(home.path(), &["token", "revoke", "Alice"]).failure();
}

/// A host that cannot resolve makes `serve` fail at bind time — after
/// `cmd_start` has written the config and settled the owner token, so these
/// tests observe the startup path without ever leaving a server listening.
const UNBINDABLE: &[&str] = &[
    "--foreground",
    "--host",
    "cryohub-test.invalid",
    "--port",
    "0",
];

fn start_unbindable(home: &std::path::Path, extra: &[&str]) -> assert_cmd::assert::Assert {
    let mut args = vec!["start"];
    args.extend_from_slice(extra);
    args.extend_from_slice(UNBINDABLE);
    cryohub_in(home, &args)
}

#[test]
fn start_mints_and_prints_the_owner_token_on_first_run() {
    // Bearer auth is the default, so a brand-new hub has to hand the operator
    // the token while they are still looking at the terminal — it is not
    // recoverable from the console, and nothing else prints it.
    let home = tempfile::tempdir().unwrap();

    let first = start_unbindable(home.path(), &[]).failure();
    let stdout = String::from_utf8(first.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.contains("Owner token (save it"),
        "first start must print the owner token: {stdout}"
    );
    let printed = stdout
        .lines()
        .find(|l| predicates::str::is_match("^[0-9a-f]{64}$").unwrap().eval(l))
        .unwrap_or_else(|| panic!("no 64-hex token in stdout: {stdout}"))
        .to_string();

    // It is the real, persisted owner token, not a one-off string.
    cryohub_in(home.path(), &["token", "owner"])
        .success()
        .stdout(contains(printed.as_str()));

    // And it is shown exactly once: a later start neither reprints nor rotates.
    let second = start_unbindable(home.path(), &[]).failure();
    let stdout2 = String::from_utf8(second.get_output().stdout.clone()).unwrap();
    assert!(
        !stdout2.contains("Owner token"),
        "an existing owner token must not be reprinted: {stdout2}"
    );
    cryohub_in(home.path(), &["token", "owner"])
        .success()
        .stdout(contains(printed.as_str()));
}

/// `~/config/cryo/cryohub.toml` under a `cryohub_in` home.
fn hub_config_text(home: &std::path::Path) -> String {
    let path = home.join("config/cryo/cryohub.toml");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

#[test]
fn a_rejected_config_fails_before_installing_a_service() {
    // The service path used to skip its preconditions entirely: it reported
    // success, installed a KeepAlive unit, and left the daemon crash-looping.
    // Note the absence of --foreground: this is the install path.
    let home = tempfile::tempdir().unwrap();
    let config = home.path().join("config/cryo/cryohub.toml");
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    std::fs::write(&config, "console_dir = \"console/dist\"\n").unwrap();

    cryohub_in(home.path(), &["start"])
        .failure()
        .stderr(contains("console_dir"));

    // Nothing was installed, in this HOME or anywhere it could have written.
    #[cfg(target_os = "macos")]
    let unit_dir = home.path().join("Library/LaunchAgents");
    #[cfg(target_os = "linux")]
    let unit_dir = home.path().join(".config/systemd/user");
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        let installed = std::fs::read_dir(&unit_dir)
            .map(|entries| entries.count())
            .unwrap_or(0);
        assert_eq!(
            installed,
            0,
            "a failed start must install no service unit ({})",
            unit_dir.display()
        );
    }
    cryohub_in(home.path(), &["status"])
        .success()
        .stdout(contains("not installed"));
}

#[test]
fn public_mode_is_the_default_and_only_an_explicit_flag_clears_it() {
    let home = tempfile::tempdir().unwrap();

    // A plain first start is public, and says so on disk and in `status`.
    start_unbindable(home.path(), &[]).failure();
    assert!(
        hub_config_text(home.path()).contains("public = true"),
        "a plain start must save public mode: {}",
        hub_config_text(home.path())
    );
    cryohub_in(home.path(), &["status"])
        .success()
        .stdout(contains("Mode: public (bearer auth)"));

    // Disabling is explicit...
    start_unbindable(home.path(), &["--no-public"]).failure();
    assert!(
        hub_config_text(home.path()).contains("public = false"),
        "--no-public must be saved: {}",
        hub_config_text(home.path())
    );
    cryohub_in(home.path(), &["status"])
        .success()
        .stdout(contains("Mode: open (loopback)"));

    // ...and so is undoing it: a plain start must not silently re-enable auth
    // and lock out a hub the operator deliberately left open.
    start_unbindable(home.path(), &[]).failure();
    assert!(hub_config_text(home.path()).contains("public = false"));

    start_unbindable(home.path(), &["--public"]).failure();
    assert!(hub_config_text(home.path()).contains("public = true"));
    start_unbindable(home.path(), &[]).failure();
    assert!(
        hub_config_text(home.path()).contains("public = true"),
        "a plain start must not drop public mode"
    );
}

#[test]
fn start_rejects_both_mode_flags_at_once() {
    let home = tempfile::tempdir().unwrap();
    cryohub_in(home.path(), &["start", "--public", "--no-public"]).failure();
}

#[test]
fn start_help_documents_public_mode() {
    let home = tempfile::tempdir().unwrap();
    cryohub_in(home.path(), &["start", "--help"])
        .success()
        .stdout(contains("--public"))
        .stdout(contains("--no-public"))
        .stdout(contains("bearer auth"))
        .stdout(contains("open mode"));
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
