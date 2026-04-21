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
fn agent_reply_drops_subject_parenthetical() {
    // The agent's reply subject is always "Reply" (see daemon::write_reply).
    // Showing it in the Zulip thread just produces "**agent** (Reply)" noise.
    let out = format_outbox_post(&mk("agent", "Reply", "hello human"));
    assert_eq!(out, "**agent**\n\nhello human");
    assert!(!out.contains("(Reply)"));
}

#[test]
fn cryochamber_report_keeps_subject() {
    // Reports and fallback alerts carry information in the subject — we
    // must not strip it just because agent replies do.
    let out = format_outbox_post(&mk(
        "cryochamber",
        "Cryochamber Report: demo",
        "Last 24h: 3 sessions, 0 failed",
    ));
    assert_eq!(
        out,
        "**cryochamber** (Cryochamber Report: demo)\n\nLast 24h: 3 sessions, 0 failed"
    );
}

#[test]
fn fallback_alert_keeps_subject() {
    let out = format_outbox_post(&mk(
        "cryochamber",
        "Fallback Alert: deadline_missed",
        "Agent exceeded max retries",
    ));
    assert!(out.starts_with("**cryochamber** (Fallback Alert: deadline_missed)\n\n"));
}
