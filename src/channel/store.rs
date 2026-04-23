use anyhow::Result;
use chrono::Local;
use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::channel::MessageChannel;
use crate::message::{self, Message};

/// Local filesystem-backed message store. Reads from `messages/inbox/` and
/// writes to `messages/outbox/`.
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
        };
        self.send_out(&msg)?;
        Ok(())
    }
}
