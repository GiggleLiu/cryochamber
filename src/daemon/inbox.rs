/// Per-session reply-obligation state for the active inbox batch.
///
/// Deliberately in-memory only, rebuilt fresh for each session. It backs
/// invariant 2 ("every inbox message is answered") across agent crashes,
/// timeouts, and graceful daemon shutdown, because `finalize_human_replies`
/// runs on all of those paths. It does NOT survive a hard kill of the daemon
/// process (SIGKILL / OOM / power loss): a batch archived by `cryo-agent
/// receive` but not yet answered when the daemon is force-killed is stranded,
/// and the sender must resend. Making this durable would require a file-backed
/// pending flow, which the inbox contract deliberately avoids (see CLAUDE.md).
/// Accepted limitation, not a bug.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct SessionInboxState {
    claimed_filenames: Vec<String>,
    agent_outbound_sent: bool,
}

impl SessionInboxState {
    pub(super) fn new() -> Self {
        Self::default()
    }

    pub(super) fn record_claimed_batch(&mut self, filenames: &[String]) {
        self.claimed_filenames = filenames.to_vec();
    }

    pub(super) fn has_claimed_batch(&self) -> bool {
        !self.claimed_filenames.is_empty()
    }

    pub(super) fn record_status_send(&mut self) {
        self.agent_outbound_sent = true;
    }

    pub(super) fn complete_agent_send(&mut self) {
        self.agent_outbound_sent = true;
        self.claimed_filenames.clear();
    }

    pub(super) fn complete_daemon_fallback(&mut self) {
        self.claimed_filenames.clear();
    }

    pub(super) fn has_agent_outbound_message(&self) -> bool {
        self.agent_outbound_sent
    }

    pub(super) fn claimed_filenames(&self) -> &[String] {
        &self.claimed_filenames
    }

    pub(super) fn claimed_message_count(&self) -> usize {
        self.claimed_filenames.len()
    }
}
