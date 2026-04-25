use anyhow::Result;
use chrono::NaiveDateTime;
use std::path::Path;

pub(super) trait SessionEffects {
    fn claim_inbox_batch(&mut self) -> Result<Vec<(String, crate::message::Message)>>;
    fn write_reply(
        &mut self,
        author: ReplyAuthor,
        text: &str,
        timestamp: NaiveDateTime,
        is_question: bool,
    ) -> Result<()>;
    fn todo_add(&mut self, text: &str, at: &str) -> Result<u32>;
    fn todo_done(&mut self, id: u32) -> Result<()>;
    fn todo_remove(&mut self, id: u32) -> Result<()>;
    fn todo_list(&mut self) -> Result<String>;
    /// Returns true iff at least one pending TODO has an `at` time parseable
    /// by the daemon wake-time format. Used to reject hibernate attempts that
    /// would leave the chamber without a scheduled next wake.
    fn has_pending_todo_with_valid_wake(&self) -> bool;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReplyAuthor {
    Agent,
    Daemon,
}

impl ReplyAuthor {
    fn from(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::Daemon => "cryochamber",
        }
    }

    fn subject(self) -> &'static str {
        match self {
            Self::Agent => "Reply",
            Self::Daemon => "Daemon Reply",
        }
    }
}

pub(super) struct FsSessionEffects<'a> {
    dir: &'a Path,
}

impl<'a> FsSessionEffects<'a> {
    pub(super) fn new(dir: &'a Path) -> Self {
        Self { dir }
    }

    fn message_store(&self) -> crate::channel::store::MessageStore {
        crate::channel::store::MessageStore::new(self.dir.to_path_buf())
    }

    fn todo_file(&self) -> crate::todo::TodoFile {
        crate::todo::TodoFile::new(self.dir.join("todo.json"))
    }
}

impl SessionEffects for FsSessionEffects<'_> {
    fn claim_inbox_batch(&mut self) -> Result<Vec<(String, crate::message::Message)>> {
        self.message_store().read_and_archive_inbox()
    }

    fn write_reply(
        &mut self,
        author: ReplyAuthor,
        text: &str,
        timestamp: NaiveDateTime,
        is_question: bool,
    ) -> Result<()> {
        let body = if is_question {
            format!("Question: {text}")
        } else {
            text.to_string()
        };
        let msg = crate::message::Message {
            from: author.from().to_string(),
            subject: author.subject().to_string(),
            body,
            timestamp,
            metadata: std::collections::BTreeMap::new(),
            is_question,
        };
        self.message_store().send_out(&msg)?;
        Ok(())
    }

    fn todo_add(&mut self, text: &str, at: &str) -> Result<u32> {
        self.todo_file().add(text.to_string(), at.to_string())
    }

    fn todo_done(&mut self, id: u32) -> Result<()> {
        self.todo_file().done(id)
    }

    fn todo_remove(&mut self, id: u32) -> Result<()> {
        self.todo_file().remove(id)
    }

    fn todo_list(&mut self) -> Result<String> {
        self.todo_file().display()
    }

    fn has_pending_todo_with_valid_wake(&self) -> bool {
        self.todo_file().next_valid_wake().ok().flatten().is_some()
    }
}
