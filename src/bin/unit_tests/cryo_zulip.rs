use super::*;
use chrono::NaiveDateTime;
use cryochamber::message::Message;
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
