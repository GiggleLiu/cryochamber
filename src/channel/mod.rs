pub mod store;
pub mod zulip;

use anyhow::Result;

use crate::message::Message;

/// Abstraction over message I/O, implemented by the local file-based
/// store. The agent always sees files; the sync utility selects the
/// channel.
pub trait MessageChannel {
    /// Read unread messages from the channel.
    fn read_inbox(&self) -> Result<Vec<Message>>;

    /// Post a reply visible to humans.
    fn post_reply(&self, body: &str) -> Result<()>;
}
