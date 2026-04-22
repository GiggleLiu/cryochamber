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
}

/// Create the messages directory structure: inbox/, outbox/, inbox/archive/.
pub fn ensure_dirs(dir: &Path) -> Result<()> {
    let messages = dir.join("messages");
    std::fs::create_dir_all(messages.join("inbox").join("archive"))?;
    std::fs::create_dir_all(messages.join("outbox"))?;
    Ok(())
}

/// Write a message to the given box (e.g. "inbox" or "outbox").
/// Returns the path of the written file.
pub fn write_message(dir: &Path, box_name: &str, msg: &Message) -> Result<PathBuf> {
    let box_dir = dir.join("messages").join(box_name);
    std::fs::create_dir_all(&box_dir)?;

    let slug = slugify(&msg.subject);
    let ts = msg.timestamp.format("%Y-%m-%dT%H-%M-%S");
    let base = if slug.is_empty() {
        message_hash(msg)
    } else {
        slug
    };
    let filename = format!("{ts}_{base}_{}.md", unique_suffix());
    let path = box_dir.join(&filename);

    // Atomic write: write to tmp, then rename
    let tmp_path = box_dir.join(format!(".tmp_{filename}"));
    let content = message_to_markdown(msg);
    std::fs::write(&tmp_path, &content)?;
    std::fs::rename(&tmp_path, &path)?;

    Ok(path)
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
    let inbox = dir.join("messages").join("inbox");
    if !inbox.exists() {
        return Ok(Vec::new());
    }

    let mut entries: Vec<_> = std::fs::read_dir(&inbox)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path().extension().is_some_and(|ext| ext == "md")
                && e.file_type().is_ok_and(|ft| ft.is_file())
        })
        .collect();

    entries.sort_by_key(|e| e.file_name());

    let mut messages = Vec::new();
    for entry in entries {
        let content = std::fs::read_to_string(entry.path())
            .with_context(|| format!("Failed to read {}", entry.path().display()))?;
        match parse_message(&content) {
            Ok(msg) => {
                let filename = entry.file_name().to_string_lossy().to_string();
                messages.push((filename, msg));
            }
            Err(e) => {
                eprintln!(
                    "Warning: skipping malformed message {}: {e}",
                    entry.path().display()
                );
            }
        }
    }

    Ok(messages)
}

/// List inbox filenames without parsing message bodies.
pub fn list_inbox(dir: &Path) -> Result<Vec<String>> {
    let inbox = dir.join("messages").join("inbox");
    if !inbox.exists() {
        return Ok(Vec::new());
    }

    let mut entries: Vec<_> = std::fs::read_dir(&inbox)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path().extension().is_some_and(|ext| ext == "md")
                && e.file_type().is_ok_and(|ft| ft.is_file())
        })
        .collect();

    entries.sort_by_key(|e| e.file_name());

    Ok(entries
        .into_iter()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect())
}

/// Read all messages from outbox/, sorted by filename (timestamp order).
pub fn read_outbox(dir: &Path) -> Result<Vec<(String, Message)>> {
    let outbox = dir.join("messages").join("outbox");
    if !outbox.exists() {
        return Ok(Vec::new());
    }

    let mut entries: Vec<_> = std::fs::read_dir(&outbox)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path().extension().is_some_and(|ext| ext == "md")
                && e.file_type().is_ok_and(|ft| ft.is_file())
        })
        .collect();

    entries.sort_by_key(|e| e.file_name());

    let mut messages = Vec::new();
    for entry in entries {
        let content = std::fs::read_to_string(entry.path())
            .with_context(|| format!("Failed to read {}", entry.path().display()))?;
        match parse_message(&content) {
            Ok(msg) => {
                let filename = entry.file_name().to_string_lossy().to_string();
                messages.push((filename, msg));
            }
            Err(e) => {
                eprintln!(
                    "Warning: skipping malformed message {}: {e}",
                    entry.path().display()
                );
            }
        }
    }

    Ok(messages)
}

/// Read all archived inbox messages from inbox/archive/, sorted by filename.
pub fn read_inbox_archive(dir: &Path) -> Result<Vec<(String, Message)>> {
    let archive = dir.join("messages").join("inbox").join("archive");
    if !archive.exists() {
        return Ok(Vec::new());
    }

    let mut entries: Vec<_> = std::fs::read_dir(&archive)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path().extension().is_some_and(|ext| ext == "md")
                && e.file_type().is_ok_and(|ft| ft.is_file())
        })
        .collect();

    entries.sort_by_key(|e| e.file_name());

    let mut messages = Vec::new();
    for entry in entries {
        let content = std::fs::read_to_string(entry.path())
            .with_context(|| format!("Failed to read {}", entry.path().display()))?;
        match parse_message(&content) {
            Ok(msg) => {
                let filename = entry.file_name().to_string_lossy().to_string();
                messages.push((filename, msg));
            }
            Err(e) => {
                eprintln!(
                    "Warning: skipping malformed archived message {}: {e}",
                    entry.path().display()
                );
            }
        }
    }

    Ok(messages)
}

/// Read all archived outbox messages from outbox/archive/, sorted by filename.
pub fn read_outbox_archive(dir: &Path) -> Result<Vec<(String, Message)>> {
    let archive = dir.join("messages").join("outbox").join("archive");
    if !archive.exists() {
        return Ok(Vec::new());
    }

    let mut entries: Vec<_> = std::fs::read_dir(&archive)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path().extension().is_some_and(|ext| ext == "md")
                && e.file_type().is_ok_and(|ft| ft.is_file())
        })
        .collect();

    entries.sort_by_key(|e| e.file_name());

    let mut messages = Vec::new();
    for entry in entries {
        let content = std::fs::read_to_string(entry.path())
            .with_context(|| format!("Failed to read {}", entry.path().display()))?;
        match parse_message(&content) {
            Ok(msg) => {
                let filename = entry.file_name().to_string_lossy().to_string();
                messages.push((filename, msg));
            }
            Err(e) => {
                eprintln!(
                    "Warning: skipping malformed archived message {}: {e}",
                    entry.path().display()
                );
            }
        }
    }

    Ok(messages)
}

/// Move processed messages from inbox/ to inbox/archive/.
pub fn archive_messages(dir: &Path, filenames: &[String]) -> Result<()> {
    let inbox = dir.join("messages").join("inbox");
    let archive = inbox.join("archive");
    std::fs::create_dir_all(&archive)?;

    for filename in filenames {
        let src = inbox.join(filename);
        let dst = archive.join(filename);
        if src.exists() {
            std::fs::rename(&src, &dst).with_context(|| format!("Failed to archive {filename}"))?;
        }
    }
    Ok(())
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
    Metadata { key: String, value: String },
    Skip,
}

fn parse_frontmatter_line(line: &str) -> FrontmatterLine {
    let line = line.trim();
    let Some((key, value)) = line.split_once(':') else {
        return FrontmatterLine::Skip;
    };

    let key = key.trim();
    let value = value.trim().to_string();
    match key {
        "" => FrontmatterLine::Skip,
        "from" => FrontmatterLine::From(value),
        "subject" => FrontmatterLine::Subject(value),
        "timestamp" => FrontmatterLine::Timestamp(value),
        _ => FrontmatterLine::Metadata {
            key: key.to_string(),
            value,
        },
    }
}

fn slugify(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

#[cfg(test)]
#[path = "unit_tests/message.rs"]
mod tests;
