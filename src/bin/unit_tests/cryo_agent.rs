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
fn send_stdin_flag_parses_without_text_argument() {
    use clap::Parser;

    let cli = super::Cli::parse_from(["cryo-agent", "send", "--stdin"]);
    match cli.command {
        super::Commands::Send { text, stdin, .. } => {
            assert!(text.is_none());
            assert!(stdin);
        }
        _ => unreachable!(),
    }
}

#[test]
fn send_stdin_conflicts_with_text_argument() {
    use clap::Parser;

    let err = match super::Cli::try_parse_from(["cryo-agent", "send", "--stdin", "text"]) {
        Ok(_) => panic!("expected --stdin to conflict with a text argument"),
        Err(err) => err,
    };
    assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
}

#[test]
fn resolve_at_arg_normalizes_seconds_to_minute() {
    assert_eq!(
        resolve_at_arg("2026-08-01T09:15:59").unwrap(),
        "2026-08-01T09:15"
    );
}

#[test]
fn resolve_at_arg_rejects_garbage_before_any_ipc() {
    let err = resolve_at_arg("tomorrow 9am").unwrap_err().to_string();
    assert!(err.contains("invalid --at value"), "got: {err}");
    assert!(err.contains("Accepted forms"), "got: {err}");
}

#[test]
fn resolve_at_arg_rejects_tz_offset() {
    let err = resolve_at_arg("2026-05-12T07:30+08:00")
        .unwrap_err()
        .to_string();
    assert!(err.contains("Accepted forms"), "got: {err}");
}

#[test]
fn resolve_at_arg_accepts_relative_offset() {
    // Result depends on the current clock; just assert canonical shape.
    let out = resolve_at_arg("+30 minutes").unwrap();
    assert!(
        chrono::NaiveDateTime::parse_from_str(&out, "%Y-%m-%dT%H:%M").is_ok(),
        "not canonical: {out}"
    );
}

#[test]
fn dialog_default_parses_to_last_20() {
    use clap::Parser;
    use cryochamber::socket::DialogFilter;

    let cli = super::Cli::parse_from(["cryo-agent", "dialog"]);
    let filter = super::dialog_filter_from_args(match &cli.command {
        super::Commands::Dialog(args) => args.clone(),
        _ => unreachable!(),
    })
    .unwrap();
    assert!(matches!(filter, DialogFilter::LastN { count: 20 }));
}

#[test]
fn dialog_last_5_parses() {
    use clap::Parser;
    use cryochamber::socket::DialogFilter;

    let cli = super::Cli::parse_from(["cryo-agent", "dialog", "--last", "5"]);
    let filter = super::dialog_filter_from_args(match &cli.command {
        super::Commands::Dialog(args) => args.clone(),
        _ => unreachable!(),
    })
    .unwrap();
    assert!(matches!(filter, DialogFilter::LastN { count: 5 }));
}

#[test]
fn dialog_all_parses() {
    use clap::Parser;
    use cryochamber::socket::DialogFilter;

    let cli = super::Cli::parse_from(["cryo-agent", "dialog", "--all"]);
    let filter = super::dialog_filter_from_args(match &cli.command {
        super::Commands::Dialog(args) => args.clone(),
        _ => unreachable!(),
    })
    .unwrap();
    assert!(matches!(filter, DialogFilter::All));
}

#[test]
fn dialog_since_parses() {
    use clap::Parser;
    use cryochamber::socket::DialogFilter;

    let cli = super::Cli::parse_from(["cryo-agent", "dialog", "--since", "2026-04-25T09:00"]);
    let filter = super::dialog_filter_from_args(match &cli.command {
        super::Commands::Dialog(args) => args.clone(),
        _ => unreachable!(),
    })
    .unwrap();
    match filter {
        DialogFilter::Since { iso } => assert_eq!(iso, "2026-04-25T09:00"),
        _ => panic!("expected Since"),
    }
}

#[test]
fn dialog_last_and_all_rejected() {
    let result = super::dialog_filter_from_args(super::DialogArgs {
        last: Some(5),
        all: true,
        since: None,
    });
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("mutually exclusive"));
}

#[test]
fn dialog_last_and_since_rejected() {
    let result = super::dialog_filter_from_args(super::DialogArgs {
        last: Some(5),
        all: false,
        since: Some("2026-04-25".to_string()),
    });
    assert!(result.is_err());
}

#[test]
fn dialog_all_and_since_rejected() {
    let result = super::dialog_filter_from_args(super::DialogArgs {
        last: None,
        all: true,
        since: Some("2026-04-25".to_string()),
    });
    assert!(result.is_err());
}
