use anyhow::Result;
use chrono::NaiveDateTime;
use std::path::{Path, PathBuf};

use super::schedule::next_wake_from_todos;

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

    fn todo_path(&self) -> PathBuf {
        self.dir.join("todo.json")
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
        let todo_path = self.todo_path();
        let mut list = crate::todo::TodoList::load(&todo_path)?;
        let id = list.add(text.to_string(), at.to_string());
        list.save(&todo_path)?;
        Ok(id)
    }

    fn todo_done(&mut self, id: u32) -> Result<()> {
        let todo_path = self.todo_path();
        let mut list = crate::todo::TodoList::load(&todo_path)?;
        list.done(id)?;
        list.save(&todo_path)?;
        Ok(())
    }

    fn todo_remove(&mut self, id: u32) -> Result<()> {
        let todo_path = self.todo_path();
        let mut list = crate::todo::TodoList::load(&todo_path)?;
        list.remove(id)?;
        list.save(&todo_path)?;
        Ok(())
    }

    fn todo_list(&mut self) -> Result<String> {
        let list = crate::todo::TodoList::load(&self.todo_path())?;
        Ok(list.display())
    }

    fn has_pending_todo_with_valid_wake(&self) -> bool {
        next_wake_from_todos(self.dir).is_some()
    }
}
