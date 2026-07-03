use super::*;
use chrono::NaiveDateTime;
use clap::Parser;
use cryochamber::message::Message;
use cryochamber::sync_common::format_outbox_post;
use std::collections::BTreeMap;

fn mk(from: &str, subject: &str, body: &str) -> Message {
    Message {
        from: from.into(),
        subject: subject.into(),
        body: body.into(),
        timestamp: NaiveDateTime::default(),
        metadata: BTreeMap::new(),
        is_question: false,
    }
}

#[test]
fn agent_reply_posts_body_only() {
    // Zulip already shows the bot name above the message; re-stating
    // "**agent**" in the body just adds noise. The subject is always
    // "Reply" anyway, which is information-free.
    let out = format_outbox_post(&mk("agent", "Reply", "hello human"));
    assert_eq!(out, "hello human");
}

#[test]
fn cryochamber_system_message_renders_as_blockquote() {
    // Daemon-authored messages are machine-generated; render them as a Zulip
    // blockquote so they read as system info rather than a human-style reply.
    let out = format_outbox_post(&mk(
        "cryochamber",
        "Fallback Alert: demo",
        "Agent hibernated without replying.",
    ));
    assert_eq!(
        out,
        "> **Fallback Alert: demo**\n>\n> Agent hibernated without replying."
    );
}

#[test]
fn cryochamber_multiline_body_quotes_each_line() {
    let out = format_outbox_post(&mk(
        "cryochamber",
        "Fallback Alert: deadline_missed",
        "Agent exceeded max retries.\nNext attempt in 60s.",
    ));
    assert_eq!(
        out,
        "> **Fallback Alert: deadline_missed**\n>\n> Agent exceeded max retries.\n> Next attempt in 60s."
    );
}

#[test]
fn unknown_sender_keeps_attribution() {
    // Anything that isn't agent/cryochamber should still identify itself.
    let out = format_outbox_post(&mk("teammate", "Question", "Are you free?"));
    assert_eq!(out, "**teammate** (Question)\n\nAre you free?");
}

#[test]
fn init_defaults_to_new_messages_only() {
    let cli = Cli::try_parse_from([
        "cryo-zulip",
        "init",
        "--config",
        "zuliprc",
        "--stream",
        "ops",
    ])
    .unwrap();

    match cli.command {
        Commands::Init { history, .. } => assert!(!history),
        _ => panic!("expected init command"),
    }
}

#[test]
fn init_history_flag_imports_existing_messages() {
    let cli = Cli::try_parse_from([
        "cryo-zulip",
        "init",
        "--config",
        "zuliprc",
        "--stream",
        "ops",
        "--history",
    ])
    .unwrap();

    match cli.command {
        Commands::Init { history, .. } => assert!(history),
        _ => panic!("expected init command"),
    }
}

#[test]
fn init_import_message_reports_history_mode() {
    assert_eq!(
        init_import_message(true, Some(42)),
        "Existing messages will be imported on first pull."
    );
}

#[test]
fn init_import_message_reports_newer_than_last_seen_message() {
    assert_eq!(
        init_import_message(false, Some(42)),
        "Only messages newer than Zulip message 42 will be imported."
    );
}

#[test]
fn init_import_message_reports_future_only_when_no_existing_messages() {
    assert_eq!(
        init_import_message(false, None),
        "No existing messages found; future messages will be imported."
    );
}

#[cfg(unix)]
#[test]
fn copy_zuliprc_to_project_makes_credentials_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    // Source lives outside .cryo so the real copy (and chmod) path runs.
    let src = dir.path().join("zuliprc");
    std::fs::write(
        &src,
        "[api]\nemail=bot@example.com\nkey=secret\nsite=https://zulip.example.com\n",
    )
    .unwrap();
    // Start world-readable to prove we tighten it, not merely inherit it.
    std::fs::set_permissions(&src, std::fs::Permissions::from_mode(0o644)).unwrap();

    copy_zuliprc_to_project(&src, dir.path()).unwrap();

    let dest = dir.path().join(".cryo").join("zuliprc");
    let mode = std::fs::metadata(&dest).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "zuliprc must be 0600, got {mode:o}");
}

#[test]
fn ensure_cryo_gitignored_creates_file_when_absent() {
    let dir = tempfile::tempdir().unwrap();
    ensure_cryo_gitignored(dir.path()).unwrap();

    let gitignore = std::fs::read_to_string(dir.path().join(".gitignore")).unwrap();
    assert!(
        gitignore.lines().any(|l| l.trim() == ".cryo/"),
        ".gitignore must ignore .cryo/: {gitignore}"
    );
}

#[test]
fn ensure_cryo_gitignored_appends_to_existing_file_missing_entry() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".gitignore"), "target/\n*.tmp\n").unwrap();

    ensure_cryo_gitignored(dir.path()).unwrap();

    let gitignore = std::fs::read_to_string(dir.path().join(".gitignore")).unwrap();
    assert!(
        gitignore.contains("target/"),
        "existing entries kept: {gitignore}"
    );
    assert!(
        gitignore.lines().any(|l| l.trim() == ".cryo/"),
        ".cryo/ appended: {gitignore}"
    );
}

#[test]
fn ensure_cryo_gitignored_is_idempotent_when_already_present() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".gitignore"), "target/\n.cryo/\n").unwrap();

    ensure_cryo_gitignored(dir.path()).unwrap();

    let gitignore = std::fs::read_to_string(dir.path().join(".gitignore")).unwrap();
    assert_eq!(
        gitignore.matches(".cryo/").count(),
        1,
        ".cryo/ must not be duplicated: {gitignore}"
    );
}

#[test]
fn copy_zuliprc_to_project_keeps_existing_file_when_source_is_destination() {
    let dir = tempfile::tempdir().unwrap();
    let cryo_dir = dir.path().join(".cryo");
    std::fs::create_dir_all(&cryo_dir).unwrap();
    let config_path = cryo_dir.join("zuliprc");
    std::fs::write(
        &config_path,
        "[api]\nemail=bot@example.com\nkey=secret\nsite=https://zulip.example.com\n",
    )
    .unwrap();

    copy_zuliprc_to_project(&config_path, dir.path()).unwrap();

    let copied = std::fs::read_to_string(&config_path).unwrap();
    assert!(copied.contains("key=secret"));
}

#[test]
fn sync_service_uses_crash_only_restart_policy() {
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/bin/cryo_zulip.rs"),
    )
    .unwrap();
    let start = source
        .find("cryochamber::service::install(\n        \"zulip-sync\",")
        .expect("zulip sync service install call should exist");
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
