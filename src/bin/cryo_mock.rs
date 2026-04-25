//! Scenario-driven mock agent for integration tests.
//!
//! Reads `./scenario.toml` from the current working directory and executes a
//! list of declarative actions. Replaces the earlier `scenario.sh` shell scripts
//! so the test matrix is searchable and the action vocabulary lives in one file.
//!
//! See `tests/scenarios/README.md` for the schema.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result};
use chrono::{Duration, Local};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Scenario {
    #[allow(dead_code)]
    #[serde(default)]
    description: String,
    /// Path (relative to cwd) to a counter file. When set, each invocation
    /// increments the counter and the session number is used to pick a matching
    /// `[[when]]` block. When unset, `actions` at the top level runs every time.
    #[serde(default)]
    counter: Option<String>,
    /// Flat action list (used when no `[[when]]` blocks are present).
    #[serde(default)]
    actions: Vec<Action>,
    /// Session-keyed action blocks (first match wins).
    #[serde(default)]
    when: Vec<WhenBlock>,
}

#[derive(Debug, Deserialize)]
struct WhenBlock {
    /// Session predicate. Forms: `"1"`, `"<3"`, `"<=2"`, `">=2"`, `">3"`, `"*"`.
    session: String,
    actions: Vec<Action>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Action {
    /// Print text to stdout (written to cryo-agent.log by the daemon).
    Echo { text: String },
    /// Sleep for `seconds` seconds.
    Sleep { seconds: u64 },
    /// Exit the process with `code`.
    Exit { code: i32 },
    /// Call `cryo-agent hibernate [--complete] [--summary ...] [--exit N]`.
    Hibernate {
        #[serde(default)]
        complete: bool,
        #[serde(default)]
        summary: Option<String>,
        #[serde(default)]
        exit: Option<i32>,
    },
    /// Call `cryo-agent todo add <text> --at <time>`.
    TodoAdd {
        text: String,
        /// Time spec: `"now"`, `"+10m"`/`"-10m"`/`"+1h"`/`"+2d"`, env var
        /// (`"$VAR"`), or passthrough string (e.g. `"banana"` or absolute ISO).
        at: String,
    },
    /// Call `cryo-agent send <message>`.
    Send { message: String },
    /// Call `cryo-agent dialog` and assert its stdout contains the requested text.
    DialogAssert {
        #[serde(default)]
        last: Option<u32>,
        #[serde(default)]
        all: bool,
        #[serde(default)]
        since: Option<String>,
        #[serde(default)]
        contains: Vec<String>,
    },
    /// Write `content` to `path` (relative to cwd); used by env-injection tests.
    WriteFile { path: String, content: String },
    /// Spawn a detached orphan subprocess that outlives the mock agent.
    SpawnOrphan { command: String, args: Vec<String> },
}

fn main() {
    if let Err(err) = run() {
        eprintln!("cryo-mock: {err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let path = PathBuf::from("scenario.toml");
    let scenario: Scenario = {
        let text =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?
    };

    let session = match &scenario.counter {
        Some(counter_path) => bump_counter(counter_path)?,
        None => 1,
    };

    let actions = select_actions(&scenario, session)?;

    for action in actions {
        if let Some(code) = run_action(action)? {
            std::process::exit(code);
        }
    }

    Ok(())
}

fn bump_counter(path: &str) -> Result<u32> {
    let current: u32 = fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);
    let next = current + 1;
    fs::write(path, next.to_string()).with_context(|| format!("writing counter {path}"))?;
    Ok(next)
}

fn select_actions(scenario: &Scenario, session: u32) -> Result<&[Action]> {
    if scenario.when.is_empty() {
        return Ok(&scenario.actions);
    }
    for block in &scenario.when {
        if matches_session(&block.session, session)? {
            return Ok(&block.actions);
        }
    }
    // No match → no-op (tests should always define a catch-all `*`).
    Ok(&[])
}

fn matches_session(spec: &str, session: u32) -> Result<bool> {
    let spec = spec.trim();
    if spec == "*" {
        return Ok(true);
    }
    if let Some(rest) = spec.strip_prefix("<=") {
        return Ok(session <= parse_u32(rest)?);
    }
    if let Some(rest) = spec.strip_prefix(">=") {
        return Ok(session >= parse_u32(rest)?);
    }
    if let Some(rest) = spec.strip_prefix('<') {
        return Ok(session < parse_u32(rest)?);
    }
    if let Some(rest) = spec.strip_prefix('>') {
        return Ok(session > parse_u32(rest)?);
    }
    Ok(session == parse_u32(spec)?)
}

fn parse_u32(s: &str) -> Result<u32> {
    s.trim()
        .parse::<u32>()
        .with_context(|| format!("invalid session number {s:?}"))
}

/// Execute one action. Returns `Some(code)` to signal an early process exit
/// (which must short-circuit remaining actions).
fn run_action(action: &Action) -> Result<Option<i32>> {
    match action {
        Action::Echo { text } => {
            println!("{}", expand(text));
            Ok(None)
        }
        Action::Sleep { seconds } => {
            std::thread::sleep(std::time::Duration::from_secs(*seconds));
            Ok(None)
        }
        Action::Exit { code } => Ok(Some(*code)),
        Action::Hibernate {
            complete,
            summary,
            exit,
        } => {
            let mut args: Vec<String> = vec!["hibernate".into()];
            if *complete {
                args.push("--complete".into());
            }
            if let Some(s) = summary {
                args.push("--summary".into());
                args.push(expand(s));
            }
            if let Some(code) = exit {
                args.push("--exit".into());
                args.push(code.to_string());
            }
            call_cryo_agent(&args)?;
            Ok(None)
        }
        Action::TodoAdd { text, at } => {
            let resolved = resolve_time(at)?;
            call_cryo_agent(&[
                "todo".to_string(),
                "add".to_string(),
                expand(text),
                "--at".to_string(),
                resolved,
            ])?;
            Ok(None)
        }
        Action::Send { message } => {
            call_cryo_agent(&["send".into(), expand(message)])?;
            Ok(None)
        }
        Action::DialogAssert {
            last,
            all,
            since,
            contains,
        } => {
            let mut args = vec!["dialog".to_string()];
            if let Some(count) = last {
                args.push("--last".into());
                args.push(count.to_string());
            }
            if *all {
                args.push("--all".into());
            }
            if let Some(iso) = since {
                args.push("--since".into());
                args.push(expand(iso));
            }

            let output = call_cryo_agent_output(&args)?;
            if !output.status.success() {
                anyhow::bail!(
                    "cryo-agent {:?} exited with {}: {}",
                    args,
                    output.status,
                    String::from_utf8_lossy(&output.stderr).trim(),
                );
            }

            let stdout = String::from_utf8_lossy(&output.stdout);
            for needle in contains {
                let needle = expand(needle);
                if !stdout.contains(&needle) {
                    anyhow::bail!(
                        "cryo-agent {:?} output missing {:?}: {}",
                        args,
                        needle,
                        stdout.trim(),
                    );
                }
            }
            Ok(None)
        }
        Action::WriteFile { path, content } => {
            fs::write(Path::new(path), expand(content))
                .with_context(|| format!("writing {path}"))?;
            Ok(None)
        }
        Action::SpawnOrphan { command, args } => {
            Command::new(command)
                .args(args)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .with_context(|| format!("spawning orphan {command}"))?;
            Ok(None)
        }
    }
}

/// Resolve a time spec into a string argument for `cryo-agent todo add --at`.
///
/// - `"now"` → current local time (ISO, minute-resolution)
/// - `"+10m"` / `"-10m"` / `"+1h"` / `"+2d"` → offset from now
/// - `"$VAR"` → env var lookup (fatal if unset)
/// - anything else → passthrough (absolute ISO or garbage for invalid-wake tests)
fn resolve_time(spec: &str) -> Result<String> {
    let spec = spec.trim();
    if spec == "now" {
        return Ok(Local::now().format("%Y-%m-%dT%H:%M").to_string());
    }
    if let Some(name) = spec.strip_prefix('$') {
        return std::env::var(name)
            .with_context(|| format!("env var {name} not set for time spec"));
    }
    if let Some(offset) = parse_offset(spec)? {
        return Ok((Local::now() + offset).format("%Y-%m-%dT%H:%M").to_string());
    }
    Ok(spec.to_string())
}

fn parse_offset(spec: &str) -> Result<Option<Duration>> {
    let sign = match spec.chars().next() {
        Some('+') => 1,
        Some('-') => -1,
        _ => return Ok(None),
    };
    let rest = &spec[1..];
    let Some(unit_idx) = rest.find(|c: char| !c.is_ascii_digit()) else {
        return Ok(None);
    };
    let (num, unit) = rest.split_at(unit_idx);
    let Ok(n) = num.parse::<i64>() else {
        return Ok(None);
    };
    let n = sign * n;
    let dur = match unit {
        "m" => Duration::minutes(n),
        "h" => Duration::hours(n),
        "d" => Duration::days(n),
        _ => return Ok(None),
    };
    Ok(Some(dur))
}

/// Expand `$VAR` occurrences using the environment (shell-like, minimal).
fn expand(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '$' {
            out.push(c);
            continue;
        }
        let mut name = String::new();
        while let Some(&nc) = chars.peek() {
            if nc.is_ascii_alphanumeric() || nc == '_' {
                name.push(nc);
                chars.next();
            } else {
                break;
            }
        }
        if name.is_empty() {
            out.push('$');
        } else {
            out.push_str(&std::env::var(&name).unwrap_or_default());
        }
    }
    out
}

/// Invoke `cryo-agent` and keep going regardless of its exit status.
///
/// The old shell scenarios ran without `set -e`, so a refused hibernate (for
/// example) never stopped the rest of the script. Tests assert on daemon log
/// output, not on the mock agent's own exit status, so we match that contract.
fn call_cryo_agent(args: &[String]) -> Result<()> {
    let output = call_cryo_agent_output(args)?;
    if !output.status.success() {
        eprintln!(
            "cryo-mock: cryo-agent {args:?} exited with {}",
            output.status
        );
    }
    Ok(())
}

fn call_cryo_agent_output(args: &[String]) -> Result<std::process::Output> {
    let output = Command::new("cryo-agent")
        .args(args)
        .output()
        .with_context(|| format!("invoking cryo-agent {args:?}"))?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_predicates() {
        assert!(matches_session("*", 1).unwrap());
        assert!(matches_session("1", 1).unwrap());
        assert!(!matches_session("1", 2).unwrap());
        assert!(matches_session("<3", 2).unwrap());
        assert!(!matches_session("<3", 3).unwrap());
        assert!(matches_session(">=2", 2).unwrap());
        assert!(matches_session("<=2", 2).unwrap());
        assert!(!matches_session(">2", 2).unwrap());
    }

    #[test]
    fn offset_parsing() {
        assert_eq!(parse_offset("+10m").unwrap(), Some(Duration::minutes(10)));
        assert_eq!(parse_offset("-10m").unwrap(), Some(Duration::minutes(-10)));
        assert_eq!(parse_offset("+1h").unwrap(), Some(Duration::hours(1)));
        assert_eq!(parse_offset("+2d").unwrap(), Some(Duration::days(2)));
        assert_eq!(parse_offset("banana").unwrap(), None);
        assert_eq!(parse_offset("+10").unwrap(), None);
    }

    #[test]
    fn env_expansion() {
        // SAFETY: tests in this binary do not run concurrently with other
        // tests that modify the same var.
        unsafe { std::env::set_var("CRYO_MOCK_TEST_VAR", "hello") };
        assert_eq!(expand("x=$CRYO_MOCK_TEST_VAR!"), "x=hello!");
        assert_eq!(expand("no var here"), "no var here");
        assert_eq!(expand("$MISSING_VAR"), "");
    }
}
