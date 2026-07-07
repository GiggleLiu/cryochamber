use super::*;

// These tests exercise the pure systemd unit-string builders. They run in CI's
// Linux job and are cfg'd out on the macOS dev machine. They never invoke
// `systemctl`.

#[test]
fn systemd_unit_string_quotes_path_and_escapes_percent() {
    // A working dir containing a space and a `%`, and a PATH containing a
    // space: none of these may corrupt the generated unit. `%` must become
    // `%%` (specifier) and PATH must be quoted so the space cannot split it
    // into a second, bogus assignment.
    let exec = systemd_exec_start(
        "/opt/my cryo/cryo-zulip",
        &["sync-daemon", "--interval", "60"],
    );
    let unit = systemd_unit_string(
        "zulip-sync",
        &exec,
        "/home/alice/chamber a%b",
        "/opt/my tools/bin:/usr/bin",
        "on-failure",
        "/home/alice/chamber a%b/cryo-zulip-sync.log",
    );

    // PATH is quoted so the embedded space cannot truncate it.
    assert!(
        unit.contains("Environment=\"PATH=/opt/my tools/bin:/usr/bin\""),
        "Environment must be a quoted PATH assignment: {unit}"
    );
    // The `%` in the working directory is doubled.
    assert!(
        unit.contains("WorkingDirectory=/home/alice/chamber a%%b"),
        "WorkingDirectory must escape %% : {unit}"
    );
    // No lone, unescaped `%` may survive anywhere in the unit: after removing
    // every `%%` pair, there must be no stray `%` left.
    assert!(
        !unit.replace("%%", "").contains('%'),
        "every % must be doubled to %% : {unit}"
    );
}

#[test]
fn systemd_exec_start_quotes_each_word_and_escapes_specials() {
    // Spaces are preserved via quoting; `"`, `\`, `$`, `%` are escaped so the
    // ExecStart line cannot be broken or reinterpreted by systemd.
    let exec = systemd_exec_start(
        "/usr/bin/cryo",
        &["a b", "quote\"x", "back\\slash", "dollar$x", "pct%y"],
    );
    assert_eq!(
        exec,
        "\"/usr/bin/cryo\" \"a b\" \"quote\\\"x\" \"back\\\\slash\" \"dollar$$x\" \"pct%%y\""
    );
}

#[test]
fn systemd_environment_path_is_quoted_and_escaped() {
    assert_eq!(
        systemd_environment_path("/usr/bin:/usr/local/bin"),
        "Environment=\"PATH=/usr/bin:/usr/local/bin\""
    );
    // A `%` in PATH is doubled inside the quotes.
    assert_eq!(
        systemd_environment_path("/opt/a%b/bin"),
        "Environment=\"PATH=/opt/a%%b/bin\""
    );
}
