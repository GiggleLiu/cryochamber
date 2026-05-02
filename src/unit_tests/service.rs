use super::*;

#[test]
fn launchd_stdio_log_path_lives_under_user_logs() {
    let home = Path::new("/Users/alice");
    let path = launchd_stdio_log_path(home, "com.cryo.daemon.abc123");

    assert_eq!(
        path,
        home.join("Library")
            .join("Logs")
            .join("cryo")
            .join("com.cryo.daemon.abc123.log")
    );
}

#[test]
fn launchctl_install_action_rewrites_changed_plist_without_unload_when_label_absent() {
    assert_eq!(
        launchctl_install_action(true, false),
        LaunchctlInstallAction::WritePlistAndLoad {
            unload_first: false
        }
    );
}

#[test]
fn launchctl_install_action_unloads_before_rewriting_changed_loaded_label() {
    assert_eq!(
        launchctl_install_action(true, true),
        LaunchctlInstallAction::WritePlistAndLoad { unload_first: true }
    );
}

#[test]
fn launchctl_install_action_loads_existing_plist_when_label_missing() {
    assert_eq!(
        launchctl_install_action(false, false),
        LaunchctlInstallAction::LoadExistingPlist
    );
}

#[test]
fn launchctl_install_action_kickstarts_when_plist_and_label_are_current() {
    assert_eq!(
        launchctl_install_action(false, true),
        LaunchctlInstallAction::Kickstart
    );
}
