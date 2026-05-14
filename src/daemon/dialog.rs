//! Pure rendering for `cryo-agent dialog`. No I/O, no claiming: the caller
//! provides message history plus the filenames that count as "new this
//! session," and this module returns the transcript string.

use chrono::NaiveDateTime;
use std::collections::HashSet;

use crate::message::Message;

pub(super) const NEW_MARKER: &str = "────────── new since last session ──────────";
pub(super) const EMPTY_PLACEHOLDER: &str = "(no dialog history yet)";

#[derive(Debug, Clone)]
pub(super) struct DialogInputs {
    pub archived_inbox: Vec<(String, Message)>,
    pub outbox: Vec<(String, Message)>,
    pub new_filenames: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum DialogFilterResolved {
    LastN(u32),
    All,
    Since(NaiveDateTime),
}

pub(super) fn render_dialog(inputs: &DialogInputs, filter: DialogFilterResolved) -> String {
    let interleaved = interleave(&inputs.archived_inbox, &inputs.outbox);
    if interleaved.is_empty() {
        return format!("{EMPTY_PLACEHOLDER}\n");
    }

    let new_set: HashSet<&str> = inputs.new_filenames.iter().map(String::as_str).collect();
    let total_new = interleaved
        .iter()
        .filter(|(name, _, _)| new_set.contains(name.as_str()))
        .count();
    let kept = apply_filter(&interleaved, filter);
    let kept_new = kept
        .iter()
        .filter(|(name, _, _)| new_set.contains(name.as_str()))
        .count();
    let omitted_new = total_new.saturating_sub(kept_new);

    let mut out = String::new();
    let mut marker_emitted = false;
    for (name, message, _) in kept {
        let is_new = new_set.contains(name.as_str());
        if is_new && !marker_emitted {
            out.push_str(NEW_MARKER);
            out.push_str("\n\n");
            marker_emitted = true;
        }
        out.push_str(&render_message(message));
    }

    if !marker_emitted && total_new > 0 {
        out.push_str(NEW_MARKER);
        out.push('\n');
        if omitted_new > 0 {
            out.push_str(&format!(
                "({omitted_new} new messages omitted by {})\n",
                filter_flag(filter)
            ));
        }
    }

    out
}

fn apply_filter<'a>(
    interleaved: &'a [(String, &'a Message, &'a str)],
    filter: DialogFilterResolved,
) -> Vec<&'a (String, &'a Message, &'a str)> {
    match filter {
        DialogFilterResolved::All => interleaved.iter().collect(),
        DialogFilterResolved::LastN(count) => {
            let start = interleaved.len().saturating_sub(count as usize);
            interleaved.iter().skip(start).collect()
        }
        DialogFilterResolved::Since(cutoff) => interleaved
            .iter()
            .filter(|(_, message, _)| message.timestamp >= cutoff)
            .collect(),
    }
}

fn filter_flag(filter: DialogFilterResolved) -> &'static str {
    match filter {
        DialogFilterResolved::LastN(_) => "--last",
        DialogFilterResolved::All => "--all",
        DialogFilterResolved::Since(_) => "--since",
    }
}

fn render_message(message: &Message) -> String {
    let metadata = crate::message::format_metadata_lines(&message.metadata);
    format!(
        "[{}] from: {}\n{}{}\n\n",
        message.timestamp.format("%Y-%m-%d %H:%M"),
        message.from,
        metadata,
        message.body.trim_end()
    )
}

fn interleave<'a>(
    inbox: &'a [(String, Message)],
    outbox: &'a [(String, Message)],
) -> Vec<(String, &'a Message, &'a str)> {
    let mut all: Vec<(String, &Message, &str)> = inbox
        .iter()
        .map(|(name, message)| (name.clone(), message, "inbox"))
        .chain(
            outbox
                .iter()
                .map(|(name, message)| (name.clone(), message, "outbox")),
        )
        .collect();
    all.sort_by(|left, right| {
        left.1
            .timestamp
            .cmp(&right.1.timestamp)
            .then_with(|| left.2.cmp(right.2))
            .then_with(|| left.0.cmp(&right.0))
    });
    all
}

#[cfg(test)]
mod render_helpers_tests {
    use super::*;
    use chrono::NaiveDate;
    use std::collections::BTreeMap;

    fn make_msg(from: &str, hour: u32) -> Message {
        Message {
            from: from.to_string(),
            subject: "S".to_string(),
            body: "B".to_string(),
            timestamp: NaiveDate::from_ymd_opt(2026, 4, 25)
                .unwrap()
                .and_hms_opt(hour, 0, 0)
                .unwrap(),
            metadata: BTreeMap::new(),
            is_question: false,
        }
    }

    #[test]
    fn interleave_breaks_ties_inbox_before_outbox() {
        let inbox = vec![("i.md".to_string(), make_msg("human", 10))];
        let outbox = vec![("o.md".to_string(), make_msg("agent", 10))];
        let merged = interleave(&inbox, &outbox);
        assert_eq!(merged[0].2, "inbox");
        assert_eq!(merged[1].2, "outbox");
    }
}
