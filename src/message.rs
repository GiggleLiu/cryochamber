// src/message.rs
use anyhow::{Context, Result};
use chrono::{Local, NaiveDateTime};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

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
    // When slug is empty (e.g. GitHub comments with no subject), use a short
    // hash of the body to avoid filename collisions within the same second.
    let disambig = if slug.is_empty() {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        msg.body.hash(&mut hasher);
        msg.from.hash(&mut hasher);
        format!("{:08x}", hasher.finish() as u32)
    } else {
        slug
    };
    let filename = format!("{ts}_{disambig}.md");
    let path = box_dir.join(&filename);

    // Atomic write: write to tmp, then rename
    let tmp_path = box_dir.join(format!(".tmp_{filename}"));
    let content = message_to_markdown(msg);
    std::fs::write(&tmp_path, &content)?;
    std::fs::rename(&tmp_path, &path)?;

    Ok(path)
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
    if !content.starts_with("---") {
        anyhow::bail!("Message missing frontmatter delimiter");
    }

    let rest = &content[3..];
    let end = rest
        .find("\n---")
        .context("Message missing closing frontmatter delimiter")?;
    let frontmatter = &rest[..end];
    let body = rest[end + 4..].trim().to_string();

    let mut from = String::new();
    let mut subject = String::new();
    let mut timestamp = Local::now().naive_local();
    let mut metadata = BTreeMap::new();

    for line in frontmatter.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim();
            let value = value.trim();
            match key {
                "from" => from = value.to_string(),
                "subject" => subject = value.to_string(),
                "timestamp" => {
                    if let Ok(ts) = NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S") {
                        timestamp = ts;
                    }
                }
                _ => {
                    metadata.insert(key.to_string(), value.to_string());
                }
            }
        }
    }

    Ok(Message {
        from,
        subject,
        body,
        timestamp,
        metadata,
    })
}

fn slugify(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}
