use anyhow::Result;
use chrono::NaiveDateTime;
use std::path::Path;

pub(super) trait SessionEffects {
    /// List unread inbox filenames without parsing message bodies.
    fn list_inbox_filenames(&self) -> Result<Vec<String>>;
    fn write_reply(
        &mut self,
        author: ReplyAuthor,
        text: &str,
        timestamp: NaiveDateTime,
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

    fn todo_file(&self) -> crate::todo::TodoFile {
        crate::todo::TodoFile::new(self.dir.join("todo.json"))
    }
}

impl SessionEffects for FsSessionEffects<'_> {
    fn list_inbox_filenames(&self) -> Result<Vec<String>> {
        crate::message::list_inbox(self.dir)
    }

    fn write_reply(
        &mut self,
        author: ReplyAuthor,
        text: &str,
        timestamp: NaiveDateTime,
    ) -> Result<()> {
        let msg = crate::message::Message {
            from: author.from().to_string(),
            subject: author.subject().to_string(),
            body: text.to_string(),
            timestamp,
            metadata: std::collections::BTreeMap::new(),
        };
        crate::message::write_message(self.dir, "outbox", &msg)?;
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
