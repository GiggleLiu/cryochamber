use anyhow::Result;
use chrono::Local;
use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::channel::MessageChannel;
use crate::message::{self, Message};

/// Local filesystem-backed message store. Reads from `messages/inbox/` and
/// writes to `messages/outbox/`.
type PageRow = (String, String, Message);

pub struct MessageStore {
    dir: PathBuf,
}

impl MessageStore {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn ensure_dirs(&self) -> Result<()> {
        message::ensure_dirs(&self.dir)
    }

    pub fn list_inbox_filenames(&self) -> Result<Vec<String>> {
        message::list_inbox(&self.dir)
    }

    pub fn read_inbox_named(&self) -> Result<Vec<(String, Message)>> {
        message::read_inbox(&self.dir)
    }

    pub fn read_inbox_archive_named(&self) -> Result<Vec<(String, Message)>> {
        message::read_inbox_archive(&self.dir)
    }

    pub fn read_outbox_named(&self) -> Result<Vec<(String, Message)>> {
        message::read_outbox(&self.dir)
    }

    pub fn read_outbox_archive_named(&self) -> Result<Vec<(String, Message)>> {
        message::read_outbox_archive(&self.dir)
    }

    pub fn read_and_archive_inbox(&self) -> Result<Vec<(String, Message)>> {
        let messages = self.read_inbox_named()?;
        if messages.is_empty() {
            return Ok(messages);
        }

        let filenames: Vec<String> = messages.iter().map(|(name, _)| name.clone()).collect();
        self.archive_inbox(&filenames)?;
        Ok(messages)
    }

    pub fn archive_inbox(&self, filenames: &[String]) -> Result<()> {
        message::archive_messages(&self.dir, filenames)
    }

    pub fn archive_outbox(&self, filenames: &[String]) -> Result<()> {
        message::archive_outbox_messages(&self.dir, filenames)
    }

    pub fn send_in(&self, msg: &Message) -> Result<PathBuf> {
        self.write_message("inbox", msg)
    }

    pub fn send_out(&self, msg: &Message) -> Result<PathBuf> {
        self.write_message("outbox", msg)
    }

    fn write_message(&self, box_name: &str, msg: &Message) -> Result<PathBuf> {
        self.ensure_dirs()?;
        message::write_message(&self.dir, box_name, msg)
    }
}

impl MessageChannel for MessageStore {
    fn read_inbox(&self) -> Result<Vec<Message>> {
        let inbox = self.read_inbox_named()?;
        Ok(inbox.into_iter().map(|(_, msg)| msg).collect())
    }

    fn post_reply(&self, body: &str) -> Result<()> {
        let msg = Message {
            from: "cryochamber".to_string(),
            subject: "Session Reply".to_string(),
            body: body.to_string(),
            timestamp: Local::now().naive_local(),
            metadata: BTreeMap::new(),
            is_question: false,
        };
        self.send_out(&msg)?;
        Ok(())
    }
}

impl MessageStore {
    /// Page by immutable mailbox filename, whose prefix is the persisted send
    /// timestamp. Archive moves preserve the cursor. Bodies outside this window
    /// are never opened; legacy nonstandard names retain lexical ordering.
    pub(crate) fn page(
        &self,
        before: Option<&str>,
        limit: usize,
    ) -> Result<(Vec<PageRow>, Option<String>)> {
        let mut files = std::collections::BTreeMap::new();
        for source in ["inbox", "inbox/archive", "outbox", "outbox/archive"] {
            let top = source.split('/').next().unwrap();
            for file in message::list_message_files(&self.dir.join("messages").join(source))? {
                let cursor = format!("{}|{top}", file.filename);
                if before.is_none_or(|before| cursor.as_str() < before) {
                    files.insert(cursor, (source.to_string(), file));
                }
            }
        }
        let mut page = Vec::new();
        let mut next = None;
        let mut files = files.into_iter().rev().peekable();
        while let Some((cursor, (source, file))) = files.next() {
            // A watcher/bridge may archive between listing and opening.
            let parsed = message::parse_message_file(&file.path).or_else(|error| {
                if source.ends_with("/archive") {
                    return Err(error);
                }
                message::parse_message_file(
                    &self
                        .dir
                        .join("messages")
                        .join(&source)
                        .join("archive")
                        .join(&file.filename),
                )
            });
            match parsed {
                Ok(msg) => page.push((source, file.filename, msg)),
                Err(error) => {
                    eprintln!("Skipping message while paging: {error}");
                    continue;
                }
            }
            if page.len() == limit {
                if files.peek().is_some() {
                    next = Some(cursor);
                }
                break;
            }
        }
        page.reverse();
        Ok((page, next))
    }
}
