use super::*;

#[test]
fn cryo_agent_exposes_send_without_reply_subcommand() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = std::fs::read_to_string(root.join("src/bin/cryo_agent.rs")).unwrap();

    assert!(
        source.contains("Request::Send"),
        "cryo-agent send must map to a dedicated Request::Send variant"
    );
    assert!(
        !source.contains("Reply {"),
        "cryo-agent reply subcommand should be removed"
    );
}

#[test]
fn iso_pass_through_minute_precision() {
    assert_eq!(
        parse_iso_timestamp("2026-04-25T10:00").unwrap(),
        "2026-04-25T10:00"
    );
}

#[test]
fn iso_pass_through_second_precision_truncates() {
    assert_eq!(
        parse_iso_timestamp("2026-04-25T10:00:42").unwrap(),
        "2026-04-25T10:00"
    );
}

#[test]
fn iso_space_separator_accepted() {
    assert_eq!(
        parse_iso_timestamp("2026-04-25 10:00").unwrap(),
        "2026-04-25T10:00"
    );
}

#[test]
fn iso_date_only_gets_midnight() {
    assert_eq!(
        parse_iso_timestamp("2026-04-25").unwrap(),
        "2026-04-25T00:00"
    );
}

#[test]
fn iso_invalid_date_rejected() {
    let err = parse_iso_timestamp("2026-13-40").unwrap_err().to_string();
    assert!(err.contains("unrecognized time expression"));
}

#[test]
fn relative_offset_plus_prefix() {
    let d = parse_relative_offset("+30 minutes").unwrap();
    assert_eq!(d.num_minutes(), 30);
}

#[test]
fn relative_offset_no_plus_prefix() {
    let d = parse_relative_offset("2 hours").unwrap();
    assert_eq!(d.num_hours(), 2);
}

#[test]
fn relative_offset_singular_unit() {
    let d = parse_relative_offset("+1 day").unwrap();
    assert_eq!(d.num_days(), 1);
}

#[test]
fn relative_offset_weeks() {
    let d = parse_relative_offset("+2 weeks").unwrap();
    assert_eq!(d.num_days(), 14);
}

#[test]
fn relative_offset_unknown_unit_lists_accepted_forms() {
    let err = parse_relative_offset("+1 fortnight")
        .unwrap_err()
        .to_string();
    assert!(err.contains("unrecognized time expression"));
    assert!(err.contains("Accepted forms"));
    assert!(err.contains("ISO8601"));
}

#[test]
fn looks_like_iso_detects_date_prefix() {
    assert!(looks_like_iso_date("2026-04-25"));
    assert!(looks_like_iso_date("2026-04-25T10:00"));
    assert!(!looks_like_iso_date("tomorrow 9am"));
    assert!(!looks_like_iso_date("+30 minutes"));
}
