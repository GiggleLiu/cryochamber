use anyhow::Result;
use chrono::Local;
use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::channel::MessageChannel;
use crate::message::{self, Message};

/// File-based message channel. Reads from messages/inbox/, writes to messages/outbox/.
pub struct FileChannel {
    dir: PathBuf,
}

impl FileChannel {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }
}

impl MessageChannel for FileChannel {
    fn read_inbox(&self) -> Result<Vec<Message>> {
        let inbox = message::read_inbox(&self.dir)?;
        Ok(inbox.into_iter().map(|(_, msg)| msg).collect())
    }

    fn post_reply(&self, body: &str) -> Result<()> {
        message::ensure_dirs(&self.dir)?;
        let msg = Message {
            from: "cryochamber".to_string(),
            subject: "Session Reply".to_string(),
            body: body.to_string(),
            timestamp: Local::now().naive_local(),
            metadata: BTreeMap::new(),
        };
        message::write_message(&self.dir, "outbox", &msg)?;
        Ok(())
    }
}
