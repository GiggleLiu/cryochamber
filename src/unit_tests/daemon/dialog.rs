use crate::daemon::dialog::{render_dialog, DialogFilterResolved, DialogInputs};
use crate::message::Message;
use chrono::NaiveDate;
use std::collections::BTreeMap;

fn msg(from: &str, body: &str, year: i32, month: u32, day: u32, hour: u32, minute: u32) -> Message {
    Message {
        from: from.to_string(),
        subject: "Reply".to_string(),
        body: body.to_string(),
        timestamp: NaiveDate::from_ymd_opt(year, month, day)
            .unwrap()
            .and_hms_opt(hour, minute, 0)
            .unwrap(),
        metadata: BTreeMap::new(),
        is_question: false,
    }
}

fn named(name: &str, m: Message) -> (String, Message) {
    (name.to_string(), m)
}

#[test]
fn render_empty_returns_placeholder() {
    let inputs = DialogInputs {
        archived_inbox: vec![],
        outbox: vec![],
        new_filenames: vec![],
    };
    let out = render_dialog(&inputs, DialogFilterResolved::All);
    assert_eq!(out.trim(), "(no dialog history yet)");
}

#[test]
fn render_archived_only_oldest_first_no_marker() {
    let inputs = DialogInputs {
        archived_inbox: vec![named(
            "2026-04-24T18-00-human.md",
            msg("human", "Hi", 2026, 4, 24, 18, 0),
        )],
        outbox: vec![named(
            "2026-04-24T18-05-agent.md",
            msg("agent", "Hello!", 2026, 4, 24, 18, 5),
        )],
        new_filenames: vec![],
    };
    let out = render_dialog(&inputs, DialogFilterResolved::All);
    assert!(!out.contains("new since last session"));
    let i_human = out.find("from: human").unwrap();
    let i_agent = out.find("from: agent").unwrap();
    assert!(i_human < i_agent, "human should appear before agent");
}

#[test]
fn render_marker_before_first_new_message() {
    let inputs = DialogInputs {
        archived_inbox: vec![
            named(
                "2026-04-24T18-00-human.md",
                msg("human", "Hi", 2026, 4, 24, 18, 0),
            ),
            named(
                "2026-04-25T09-30-human.md",
                msg("human", "Update plan", 2026, 4, 25, 9, 30),
            ),
        ],
        outbox: vec![named(
            "2026-04-24T18-05-agent.md",
            msg("agent", "Hello!", 2026, 4, 24, 18, 5),
        )],
        new_filenames: vec!["2026-04-25T09-30-human.md".to_string()],
    };
    let out = render_dialog(&inputs, DialogFilterResolved::All);
    let marker = "────────── new since last session ──────────";
    let i_marker = out.find(marker).expect("marker present");
    let i_old_agent = out.find("Hello!").unwrap();
    let i_new_human = out.find("Update plan").unwrap();
    assert!(i_old_agent < i_marker);
    assert!(i_marker < i_new_human);
}

#[test]
fn render_last_n_trims_oldest_first() {
    let inputs = DialogInputs {
        archived_inbox: vec![
            named(
                "2026-04-24T18-00-human.md",
                msg("human", "msg1", 2026, 4, 24, 18, 0),
            ),
            named(
                "2026-04-24T18-10-human.md",
                msg("human", "msg2", 2026, 4, 24, 18, 10),
            ),
            named(
                "2026-04-24T18-20-human.md",
                msg("human", "msg3", 2026, 4, 24, 18, 20),
            ),
        ],
        outbox: vec![],
        new_filenames: vec![],
    };
    let out = render_dialog(&inputs, DialogFilterResolved::LastN(2));
    assert!(!out.contains("msg1"));
    assert!(out.contains("msg2"));
    assert!(out.contains("msg3"));
}

#[test]
fn render_since_filters_by_timestamp() {
    let inputs = DialogInputs {
        archived_inbox: vec![
            named(
                "2026-04-24T18-00-human.md",
                msg("human", "old", 2026, 4, 24, 18, 0),
            ),
            named(
                "2026-04-25T09-00-human.md",
                msg("human", "fresh", 2026, 4, 25, 9, 0),
            ),
        ],
        outbox: vec![],
        new_filenames: vec![],
    };
    let cutoff = NaiveDate::from_ymd_opt(2026, 4, 25)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap();
    let out = render_dialog(&inputs, DialogFilterResolved::Since(cutoff));
    assert!(!out.contains("old"));
    assert!(out.contains("fresh"));
}

#[test]
fn render_filter_drops_all_new_emits_omitted_count() {
    let inputs = DialogInputs {
        archived_inbox: vec![
            named(
                "2026-04-24T18-00-human.md",
                msg("human", "old1", 2026, 4, 24, 18, 0),
            ),
            named(
                "2026-04-24T18-05-human.md",
                msg("human", "new1", 2026, 4, 24, 18, 5),
            ),
        ],
        outbox: vec![
            named(
                "2026-04-24T18-10-agent.md",
                msg("agent", "old2", 2026, 4, 24, 18, 10),
            ),
            named(
                "2026-04-24T18-20-agent.md",
                msg("agent", "old3", 2026, 4, 24, 18, 20),
            ),
        ],
        new_filenames: vec!["2026-04-24T18-05-human.md".to_string()],
    };
    let out = render_dialog(&inputs, DialogFilterResolved::LastN(2));
    assert!(out.contains("old2"));
    assert!(out.contains("old3"));
    assert!(!out.contains("new1"));
    assert!(out.contains("new since last session"));
    assert!(out.contains("(1 new messages omitted by --last)"));
}

#[test]
fn render_all_messages_new_marker_at_top() {
    let inputs = DialogInputs {
        archived_inbox: vec![named(
            "2026-04-25T09-30-human.md",
            msg("human", "first contact", 2026, 4, 25, 9, 30),
        )],
        outbox: vec![],
        new_filenames: vec!["2026-04-25T09-30-human.md".to_string()],
    };
    let out = render_dialog(&inputs, DialogFilterResolved::All);
    let marker_idx = out.find("new since last session").unwrap();
    let body_idx = out.find("first contact").unwrap();
    assert!(marker_idx < body_idx);
}
