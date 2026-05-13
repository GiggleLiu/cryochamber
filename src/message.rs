// src/message.rs
use anyhow::{Context, Result};
use chrono::{Local, NaiveDateTime};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static MESSAGE_SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
pub struct Message {
    pub from: String,
    pub subject: String,
    pub body: String,
    pub timestamp: NaiveDateTime,
    pub metadata: BTreeMap<String, String>,
    pub is_question: bool,
}

/// Create the messages directory structure: inbox/, outbox/, inbox/archive/.
pub fn ensure_dirs(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(inbox_archive_dir(dir))?;
    std::fs::create_dir_all(message_box_dir(dir, "outbox"))?;
    Ok(())
}

/// Write a message to the given box (e.g. "inbox" or "outbox").
/// Returns the path of the written file.
pub fn write_message(dir: &Path, box_name: &str, msg: &Message) -> Result<PathBuf> {
    let box_dir = message_box_dir(dir, box_name);
    std::fs::create_dir_all(&box_dir)?;

    let ts = msg.timestamp.format("%Y-%m-%dT%H-%M-%S");
    let base = message_filename_base(msg);
    let filename = format!("{ts}_{}_{}.md", base.as_str(), unique_suffix());
    let path = box_dir.join(&filename);

    // Atomic write: write to tmp, then rename
    let tmp_path = box_dir.join(format!(".tmp_{filename}"));
    let content = message_to_markdown(msg);
    std::fs::write(&tmp_path, &content)?;
    std::fs::rename(&tmp_path, &path)?;

    Ok(path)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum MessageFilenameBase {
    Slug(String),
    Hash(String),
}

impl MessageFilenameBase {
    fn as_str(&self) -> &str {
        match self {
            Self::Slug(value) | Self::Hash(value) => value,
        }
    }
}

fn message_filename_base(msg: &Message) -> MessageFilenameBase {
    let slug = slugify(&msg.subject);
    if slug.is_empty() {
        MessageFilenameBase::Hash(message_hash(msg))
    } else {
        MessageFilenameBase::Slug(slug)
    }
}

fn message_hash(msg: &Message) -> String {
    use std::hash::{Hash, Hasher};

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    msg.body.hash(&mut hasher);
    msg.from.hash(&mut hasher);
    msg.subject.hash(&mut hasher);
    format!("{:08x}", hasher.finish() as u32)
}

fn unique_suffix() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let seq = MESSAGE_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("{nanos:032x}_{:08x}_{seq:04x}", std::process::id())
}

/// Read all unread messages from inbox/, sorted by filename (timestamp order).
/// Returns (filename, Message) pairs.
pub fn read_inbox(dir: &Path) -> Result<Vec<(String, Message)>> {
    read_message_dir(&message_box_dir(dir, "inbox"), "message")
}

/// List inbox filenames without parsing message bodies.
pub fn list_inbox(dir: &Path) -> Result<Vec<String>> {
    Ok(list_message_files(&message_box_dir(dir, "inbox"))?
        .into_iter()
        .map(|file| file.filename)
        .collect())
}

/// Read all messages from outbox/, sorted by filename (timestamp order).
pub fn read_outbox(dir: &Path) -> Result<Vec<(String, Message)>> {
    read_message_dir(&message_box_dir(dir, "outbox"), "message")
}

/// Read all archived inbox messages from inbox/archive/, sorted by filename.
pub fn read_inbox_archive(dir: &Path) -> Result<Vec<(String, Message)>> {
    read_message_dir(&inbox_archive_dir(dir), "archived message")
}

/// Read all archived outbox messages from outbox/archive/, sorted by filename.
pub fn read_outbox_archive(dir: &Path) -> Result<Vec<(String, Message)>> {
    read_message_dir(
        &message_box_dir(dir, "outbox").join("archive"),
        "archived message",
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MessageFile {
    filename: String,
    path: PathBuf,
}

fn list_message_files(message_dir: &Path) -> Result<Vec<MessageFile>> {
    if !message_dir.exists() {
        return Ok(Vec::new());
    }

    let mut files: Vec<_> = std::fs::read_dir(message_dir)?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry.path().extension().is_some_and(|ext| ext == "md")
                && entry.file_type().is_ok_and(|file_type| file_type.is_file())
        })
        .map(|entry| MessageFile {
            filename: entry.file_name().to_string_lossy().to_string(),
            path: entry.path(),
        })
        .collect();

    files.sort_by(|left, right| left.filename.cmp(&right.filename));
    Ok(files)
}

fn read_message_dir(message_dir: &Path, malformed_label: &str) -> Result<Vec<(String, Message)>> {
    let mut messages = Vec::new();
    for file in list_message_files(message_dir)? {
        let content = std::fs::read_to_string(&file.path)
            .with_context(|| format!("Failed to read {}", file.path.display()))?;
        match parse_message(&content) {
            Ok(message) => messages.push((file.filename, message)),
            Err(e) => {
                eprintln!(
                    "Warning: skipping malformed {malformed_label} {}: {e}",
                    file.path.display()
                );
            }
        }
    }
    Ok(messages)
}

/// Render a list of inbox messages as the block the agent sees — same
/// format `cryo-agent receive` prints, so the daemon's wake-time prompt
/// inlining and the IPC path share this formatter. Empty input returns
/// "No messages.\n". Pure; does not archive.
pub fn format_inbox(messages: &[(String, Message)]) -> String {
    if messages.is_empty() {
        return "No messages.\n".to_string();
    }
    let mut body = String::new();
    for (filename, msg) in messages {
        body.push_str(&format!("--- {filename} ---\n"));
        if !msg.from.is_empty() {
            body.push_str(&format!("From: {}\n", msg.from));
        }
        if !msg.subject.is_empty() {
            body.push_str(&format!("Subject: {}\n", msg.subject));
        }
        body.push('\n');
        body.push_str(&msg.body);
        body.push('\n');
        body.push('\n');
    }
    body
}

/// Move processed messages from inbox/ to inbox/archive/.
pub fn archive_messages(dir: &Path, filenames: &[String]) -> Result<()> {
    move_messages(
        &message_box_dir(dir, "inbox"),
        &inbox_archive_dir(dir),
        filenames,
        "archive",
    )
}

/// Move processed messages from outbox/ to outbox/archive/.
pub fn archive_outbox_messages(dir: &Path, filenames: &[String]) -> Result<()> {
    move_messages(
        &message_box_dir(dir, "outbox"),
        &message_box_dir(dir, "outbox").join("archive"),
        filenames,
        "archive",
    )
}

fn move_messages(
    source_dir: &Path,
    destination_dir: &Path,
    filenames: &[String],
    action: &str,
) -> Result<()> {
    std::fs::create_dir_all(destination_dir)?;

    for filename in filenames {
        let src = source_dir.join(filename);
        let dst = destination_dir.join(filename);
        if src.exists() {
            std::fs::rename(&src, &dst)
                .with_context(|| format!("Failed to {action} {filename}"))?;
        }
    }
    Ok(())
}

fn message_box_dir(dir: &Path, box_name: &str) -> PathBuf {
    dir.join("messages").join(box_name)
}

fn inbox_archive_dir(dir: &Path) -> PathBuf {
    message_box_dir(dir, "inbox").join("archive")
}

/// Render a message as markdown with frontmatter.
pub fn message_to_markdown(msg: &Message) -> String {
    let mut lines = Vec::new();
    lines.push("---".to_string());
    lines.push(format!("from: {}", msg.from));
    lines.push(format!("subject: {}", msg.subject));
    lines.push(format!(
        "timestamp: {}",
        msg.timestamp.format("%Y-%m-%dT%H:%M:%S")
    ));
    if msg.is_question {
        lines.push("question: true".to_string());
    }
    for (key, value) in &msg.metadata {
        lines.push(format!("{key}: {value}"));
    }
    lines.push("---".to_string());
    lines.push(String::new());
    lines.push(msg.body.clone());
    lines.push(String::new());
    lines.join("\n")
}

/// Parse a markdown message with frontmatter.
pub fn parse_message(content: &str) -> Result<Message> {
    let content = content.trim();
    let sections = split_message_markdown(content)?;
    let fields = parse_frontmatter_fields(sections.frontmatter, Local::now().naive_local());

    Ok(Message {
        from: fields.from,
        subject: fields.subject,
        body: sections.body.to_string(),
        timestamp: fields.timestamp,
        metadata: fields.metadata,
        is_question: fields.is_question,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MessageMarkdownSections<'a> {
    frontmatter: &'a str,
    body: &'a str,
}

fn split_message_markdown(content: &str) -> Result<MessageMarkdownSections<'_>> {
    let content = content.trim();
    if !content.starts_with("---") {
        anyhow::bail!("Message missing frontmatter delimiter");
    }

    let rest = &content[3..];
    let end = rest
        .find("\n---")
        .context("Message missing closing frontmatter delimiter")?;

    Ok(MessageMarkdownSections {
        frontmatter: &rest[..end],
        body: rest[end + 4..].trim(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FrontmatterFields {
    from: String,
    subject: String,
    timestamp: NaiveDateTime,
    metadata: BTreeMap<String, String>,
    is_question: bool,
}

fn parse_frontmatter_fields(
    frontmatter: &str,
    fallback_timestamp: NaiveDateTime,
) -> FrontmatterFields {
    let mut fields = FrontmatterFields {
        from: String::new(),
        subject: String::new(),
        timestamp: fallback_timestamp,
        metadata: BTreeMap::new(),
        is_question: false,
    };

    for line in frontmatter.lines() {
        match parse_frontmatter_line(line) {
            FrontmatterLine::From(value) => fields.from = value,
            FrontmatterLine::Subject(value) => fields.subject = value,
            FrontmatterLine::Timestamp(value) => {
                if let Ok(ts) = NaiveDateTime::parse_from_str(&value, "%Y-%m-%dT%H:%M:%S") {
                    fields.timestamp = ts;
                }
            }
            FrontmatterLine::Question(value) => fields.is_question = value,
            FrontmatterLine::Metadata { key, value } => {
                fields.metadata.insert(key, value);
            }
            FrontmatterLine::Skip => {}
        }
    }

    fields
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FrontmatterLine {
    From(String),
    Subject(String),
    Timestamp(String),
    Question(bool),
    Metadata { key: String, value: String },
    Skip,
}

fn clean_frontmatter_value(value: &str) -> String {
    let bytes = value.as_bytes();
    let stripped = if bytes.len() >= 2 && bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"' {
        &value[1..value.len() - 1]
    } else {
        value
    };
    stripped.replace("\\\"", "\"")
}

fn parse_frontmatter_line(line: &str) -> FrontmatterLine {
    let line = line.trim();
    let Some((key, value)) = line.split_once(':') else {
        return FrontmatterLine::Skip;
    };

    let key = key.trim();
    let value = clean_frontmatter_value(value.trim());
    match key {
        "" => FrontmatterLine::Skip,
        "from" => FrontmatterLine::From(value),
        "subject" => FrontmatterLine::Subject(value),
        "timestamp" => FrontmatterLine::Timestamp(value),
        "question" => FrontmatterLine::Question(value.eq_ignore_ascii_case("true")),
        _ => FrontmatterLine::Metadata {
            key: key.to_string(),
            value,
        },
    }
}

fn slugify(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(slug_char)
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn slug_char(c: char) -> char {
    match c.is_alphanumeric() {
        true => c,
        false => '-',
    }
}

#[cfg(test)]
#[path = "unit_tests/message.rs"]
mod tests;
