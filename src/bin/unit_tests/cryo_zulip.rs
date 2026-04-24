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
fn cryochamber_report_renders_as_blockquote() {
    // Reports are machine-generated; render them as a Zulip blockquote
    // so they read as system info rather than a human-style reply.
    let out = format_outbox_post(&mk(
        "cryochamber",
        "Cryochamber Report: demo",
        "Last 24h: 3 sessions, 0 failed",
    ));
    assert_eq!(
        out,
        "> **Cryochamber Report: demo**\n>\n> Last 24h: 3 sessions, 0 failed"
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
