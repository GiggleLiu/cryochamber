/// Per-session reply-obligation state for the active inbox batch.
///
/// Runtime state for the active session. Filesystem effects journal the reply
/// obligation before archiving. Restart reports interruption without replaying
/// claimed work; the sender decides whether to resend.
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
        for filename in filenames {
            if !self.claimed_filenames.contains(filename) {
                self.claimed_filenames.push(filename.clone());
            }
        }
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

use crate::channel::store::MessageStore;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

const RECEIPT: &str = "reply_obligation";

#[derive(serde::Serialize, serde::Deserialize)]
struct Obligation {
    id: String,
    filenames: Vec<String>,
}

fn journal_path(dir: &Path) -> PathBuf {
    dir.join(".cryo/reply-obligation.json")
}

fn load(dir: &Path) -> Result<Option<Obligation>> {
    match std::fs::read(journal_path(dir)) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes).context(
            "Invalid reply obligation; preserve the journal and restore it from backup",
        )?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn clear(dir: &Path) -> Result<()> {
    std::fs::remove_file(journal_path(dir))?;
    std::fs::File::open(dir.join(".cryo"))?.sync_all()?;
    Ok(())
}

pub(super) fn claim(dir: &Path) -> Result<Vec<(String, crate::message::Message)>> {
    // Dialog can claim more mail while an earlier batch awaits a reply.
    // Only startup recovers; an active session extends the same obligation.
    let existing = load(dir)?;
    let store = MessageStore::new(dir.to_path_buf());
    let mut messages = store.read_inbox_named()?;
    let mut thread = if let Some(pending) = &existing {
        store
            .get(&format!(
                "inbox/{}",
                pending
                    .filenames
                    .first()
                    .context("Empty reply obligation")?
            ))?
            .metadata
            .get("thread_id")
            .cloned()
    } else {
        messages
            .first()
            .and_then(|(_, msg)| msg.metadata.get("thread_id").cloned())
    };
    // A clock rollback can sort a follow-up before its still-unclaimed parent.
    // Receive the parent in the main conversation first, never preview it as history.
    if existing.is_none()
        && thread
            .as_deref()
            .and_then(|id| id.strip_prefix("inbox/"))
            .is_some_and(|parent| messages.iter().any(|(name, _)| name == parent))
    {
        thread = None;
    }
    messages.retain(|(_, msg)| msg.metadata.get("thread_id") == thread.as_ref());
    if messages.is_empty() {
        return Ok(messages);
    }
    std::fs::create_dir_all(dir.join(".cryo"))?;
    std::fs::File::open(dir)?.sync_all()?;
    let filenames: Vec<String> = messages.iter().map(|(name, _)| name.clone()).collect();
    let mut obligation = existing.unwrap_or_else(|| Obligation {
        id: crate::state::new_instance_id(),
        filenames: Vec::new(),
    });
    for filename in &filenames {
        if !obligation.filenames.contains(filename) {
            obligation.filenames.push(filename.clone());
        }
    }
    crate::persistence::write_durable(&journal_path(dir), serde_json::to_vec(&obligation)?)?;
    store.archive_inbox(&filenames)?;
    for (_, msg) in &mut messages {
        msg.body = store.agent_body(&msg.body);
    }
    if let Some(root) = thread {
        // Context is returned to the agent only; stored message bodies stay unchanged.
        let context = store.thread_context(&root, &filenames)?;
        messages[0].1.body = format!(
            "{context}\nNew follow-up (reply stays in this thread):\n{}",
            messages[0].1.body
        );
    }
    Ok(messages)
}

pub(super) fn send(dir: &Path, mut message: crate::message::Message) -> Result<()> {
    let obligation = load(dir)?;
    if let Some(ref pending) = obligation {
        message.metadata.insert(RECEIPT.into(), pending.id.clone());
        if let Some(filename) = pending.filenames.first() {
            if let Some(thread) = MessageStore::new(dir.into())
                .get(&format!("inbox/{filename}"))?
                .metadata
                .get("thread_id")
            {
                message.metadata.insert("thread_id".into(), thread.clone());
            }
        }
    }
    MessageStore::new(dir.to_path_buf()).send_out(&message)?;
    if obligation.is_some() {
        clear(dir)?;
    }
    Ok(())
}

/// Complete interrupted claims and write one visible notice on restart.
/// A durable reply receipt closes the crash window between sending and clearing
/// the journal, including when a bridge already moved the reply to archive.
pub(super) fn recover(dir: &Path) -> Result<()> {
    let Some(pending) = load(dir)? else {
        return Ok(());
    };
    anyhow::ensure!(
        pending.filenames.iter().all(|name| {
            Path::new(name).components().count() == 1
                && matches!(
                    Path::new(name).components().next(),
                    Some(std::path::Component::Normal(_))
                )
        }),
        "Invalid filename in reply obligation"
    );
    let store = MessageStore::new(dir.to_path_buf());
    store.ensure_dirs()?;
    store.archive_inbox(&pending.filenames)?;
    let answered = store
        .read_outbox_named()?
        .into_iter()
        .chain(store.read_outbox_archive_named()?)
        .any(|(_, message)| message.metadata.get(RECEIPT) == Some(&pending.id));
    if !answered {
        let mut message = crate::message::Message {
            from: "cryochamber".into(),
            subject: "Interrupted conversation".into(),
            body: format!("The daemon stopped while receiving or answering {} message(s). Work may have partially completed. Review the conversation and resend your instruction if you still want it carried out. Nothing has been replayed automatically.", pending.filenames.len()),
            timestamp: chrono::Local::now().naive_local(),
            metadata: [(RECEIPT.into(), pending.id)].into(),
            is_question: false,
        };
        if let Some(filename) = pending.filenames.first() {
            if let Some(thread) = store
                .get(&format!("inbox/{filename}"))?
                .metadata
                .get("thread_id")
            {
                message.metadata.insert("thread_id".into(), thread.clone());
            }
        }
        store.send_out(&message)?;
    }
    clear(dir)
}

#[cfg(test)]
mod recovery_tests {
    use super::*;

    #[test]
    fn dialog_extends_live_reply_obligation_without_interruption_notice() {
        use crate::daemon::{
            effects::{FsSessionEffects, ReplyAuthor, SessionEffects},
            request::handle_dialog_request,
        };
        let dir = tempfile::tempdir().unwrap();
        let store = MessageStore::new(dir.path().into());
        let message = crate::message::Message {
            from: "human".into(),
            subject: "Task".into(),
            body: "Read before replying".into(),
            timestamp: chrono::Local::now().naive_local(),
            metadata: Default::default(),
            is_question: false,
        };
        store.send_in(&message).unwrap();
        let mut effects = FsSessionEffects::new(dir.path());
        let first = effects.claim_inbox_batch().unwrap();
        let id = load(dir.path()).unwrap().unwrap().id;
        store.send_in(&message).unwrap();
        let dialog = handle_dialog_request(
            crate::socket::DialogFilter::All,
            &[first[0].0.clone()],
            &mut effects,
        );
        assert!(dialog.ok);
        assert_eq!(dialog.claimed_filenames.len(), 1);
        let pending = load(dir.path()).unwrap().unwrap();
        assert_eq!(pending.id, id);
        assert_eq!(pending.filenames.len(), 2);
        let repeated = handle_dialog_request(
            crate::socket::DialogFilter::All,
            &pending.filenames,
            &mut effects,
        );
        assert!(repeated.ok);
        assert!(store.read_outbox_named().unwrap().is_empty());
        effects
            .write_reply(ReplyAuthor::Agent, "Both done", message.timestamp, false)
            .unwrap();
        recover(dir.path()).unwrap();
        let replies = store.read_outbox_named().unwrap();
        assert_eq!(replies.len(), 1);
        assert_eq!(replies[0].1.body, "Both done");
        assert_eq!(replies[0].1.metadata.get(RECEIPT), Some(&id));
    }

    #[test]
    fn crash_worker() {
        let Ok(dir) = std::env::var("CRYO_RECOVERY_CRASH_TEST") else {
            return;
        };
        let dir = Path::new(&dir);
        claim(dir).unwrap();
        std::fs::write(dir.join("claimed.ready"), "ready").unwrap();
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }

    #[test]
    fn killed_process_and_restored_backup_report_the_unanswered_batch() {
        let original = tempfile::tempdir().unwrap();
        let restored = tempfile::tempdir().unwrap();
        let backup = tempfile::NamedTempFile::new().unwrap();
        let store = MessageStore::new(original.path().into());
        store.ensure_dirs().unwrap();
        std::fs::write(
            original.path().join("messages/inbox/task.md"),
            "---\nfrom: human\nsubject: Work\n---\n\nDo this once",
        )
        .unwrap();
        let mut worker = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "daemon::inbox::recovery_tests::crash_worker"])
            .env("CRYO_RECOVERY_CRASH_TEST", original.path())
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !original.path().join("claimed.ready").exists()
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        worker.kill().unwrap(); // SIGKILL: no destructors or graceful fallback.
        worker.wait().unwrap();
        assert!(original.path().join("claimed.ready").exists());
        assert!(std::process::Command::new("tar")
            .arg("-cf")
            .arg(backup.path())
            .arg("-C")
            .arg(original.path())
            .arg(".")
            .status()
            .unwrap()
            .success());
        assert!(std::process::Command::new("tar")
            .arg("-xf")
            .arg(backup.path())
            .arg("-C")
            .arg(restored.path())
            .status()
            .unwrap()
            .success());
        recover(restored.path()).unwrap();
        recover(restored.path()).unwrap();
        let restored_store = MessageStore::new(restored.path().into());
        assert!(restored_store.read_inbox_named().unwrap().is_empty());
        assert_eq!(restored_store.read_inbox_archive_named().unwrap().len(), 1);
        let notices = restored_store.read_outbox_named().unwrap();
        assert_eq!(notices.len(), 1);
        assert!(notices[0].1.body.contains("resend"));
        assert_eq!(notices[0].1.from, "cryochamber");
    }

    #[test]
    fn interruption_is_reported_once_without_replaying_and_reply_receipts_survive_archive() {
        let dir = tempfile::tempdir().unwrap();
        let store = MessageStore::new(dir.path().into());
        let message = crate::message::Message {
            from: "human".into(),
            subject: "Work".into(),
            body: "Do it once".into(),
            timestamp: chrono::Local::now().naive_local(),
            metadata: Default::default(),
            is_question: false,
        };
        for _ in 0..3 {
            store.send_in(&message).unwrap();
        }
        claim(dir.path()).unwrap();
        assert!(store.read_inbox_named().unwrap().is_empty());
        recover(dir.path()).unwrap();
        recover(dir.path()).unwrap();
        let notices = store.read_outbox_named().unwrap();
        assert_eq!(notices.len(), 1);
        assert!(notices[0].1.body.contains("3 message(s)"));
        assert_eq!(store.read_inbox_archive_named().unwrap().len(), 3);

        store.send_in(&message).unwrap();
        claim(dir.path()).unwrap();
        let journal = std::fs::read(journal_path(dir.path())).unwrap();
        send(dir.path(), message).unwrap();
        let replies: Vec<_> = store
            .read_outbox_named()
            .unwrap()
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        store.archive_outbox(&replies).unwrap();
        // Crash after the reply persisted but before the journal was removed.
        crate::persistence::write_durable(&journal_path(dir.path()), journal).unwrap();
        recover(dir.path()).unwrap();
        assert!(store.read_outbox_named().unwrap().is_empty());
        assert_eq!(store.read_outbox_archive_named().unwrap().len(), 2);
        assert!(!journal_path(dir.path()).exists());
    }

    #[test]
    fn crash_before_archive_finishes_claim_and_corrupt_journal_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let store = MessageStore::new(dir.path().into());
        store.ensure_dirs().unwrap();
        std::fs::create_dir_all(dir.path().join(".cryo")).unwrap();
        let pending = Obligation {
            id: "test".into(),
            filenames: vec!["one.md".into()],
        };
        std::fs::write(dir.path().join("messages/inbox/one.md"), "---\nfrom: human\nsubject: Work\ntimestamp: 2026-09-05T12:00:00\n---\n\nunfinished work").unwrap();
        crate::persistence::write_durable(
            &journal_path(dir.path()),
            serde_json::to_vec(&pending).unwrap(),
        )
        .unwrap();
        recover(dir.path()).unwrap();
        assert!(store.read_inbox_named().unwrap().is_empty());
        assert_eq!(store.read_inbox_archive_named().unwrap().len(), 1);
        assert_eq!(store.read_outbox_named().unwrap().len(), 1);
        std::fs::write(journal_path(dir.path()), "broken").unwrap();
        assert!(claim(dir.path()).is_err());
        assert!(journal_path(dir.path()).exists());
    }

    fn threaded_message(body: &str, thread: Option<&str>) -> crate::message::Message {
        crate::message::Message {
            from: "human".into(),
            subject: "Thread".into(),
            body: body.into(),
            timestamp: chrono::Local::now().naive_local(),
            metadata: thread
                .map(|id| [("thread_id".into(), id.into())].into())
                .unwrap_or_default(),
            is_question: false,
        }
    }

    #[test]
    fn conversations_are_claimed_and_answered_one_at_a_time() {
        use crate::daemon::effects::{FsSessionEffects, ReplyAuthor, SessionEffects};

        let dir = tempfile::tempdir().unwrap();
        let store = MessageStore::new(dir.path().into());
        let first_root = store
            .send_in(&threaded_message("first root", None))
            .unwrap();
        let first_root = format!(
            "inbox/{}",
            first_root.file_name().unwrap().to_string_lossy()
        );
        store
            .archive_inbox(&[first_root.strip_prefix("inbox/").unwrap().into()])
            .unwrap();
        store
            .send_in(&threaded_message("first follow-up", Some(&first_root)))
            .unwrap();
        let second_root = store
            .send_in(&threaded_message("second root", None))
            .unwrap();
        let second_root = format!(
            "inbox/{}",
            second_root.file_name().unwrap().to_string_lossy()
        );
        store
            .archive_inbox(&[second_root.strip_prefix("inbox/").unwrap().into()])
            .unwrap();
        store
            .send_in(&threaded_message("second follow-up", Some(&second_root)))
            .unwrap();

        let mut effects = FsSessionEffects::new(dir.path());
        let first = effects.claim_inbox_batch().unwrap();
        assert_eq!(first.len(), 1);
        assert!(first[0].1.body.contains("first root"));
        assert!(first[0].1.body.contains("first follow-up"));
        assert_eq!(first[0].1.body.matches("first follow-up").count(), 1);
        effects
            .write_reply(
                ReplyAuthor::Agent,
                "answered first",
                chrono::Local::now().naive_local(),
                false,
            )
            .unwrap();
        assert!(!journal_path(dir.path()).exists());

        let next = effects.claim_inbox_batch().unwrap();
        assert_eq!(next.len(), 1);
        assert!(next[0].1.body.contains("second root"));
        assert!(next[0].1.body.contains("second follow-up"));
        assert_eq!(next[0].1.body.matches("second follow-up").count(), 1);
        effects
            .write_reply(
                ReplyAuthor::Agent,
                "answered follow-up",
                chrono::Local::now().naive_local(),
                false,
            )
            .unwrap();
        let replies = store.read_outbox_named().unwrap();
        assert_eq!(replies.len(), 2);
        assert_eq!(
            replies[0].1.metadata.get("thread_id").map(String::as_str),
            Some(first_root.as_str())
        );
        assert_eq!(
            replies[1].1.metadata.get("thread_id").map(String::as_str),
            Some(second_root.as_str())
        );

        store
            .send_in(&threaded_message("new main conversation", None))
            .unwrap();
        let main = effects.claim_inbox_batch().unwrap();
        assert_eq!(main.len(), 1);
        assert_eq!(main[0].1.body, "new main conversation");
        effects
            .write_reply(
                ReplyAuthor::Agent,
                "answered main",
                chrono::Local::now().naive_local(),
                false,
            )
            .unwrap();
        let replies = store.read_outbox_named().unwrap();
        assert_eq!(replies.len(), 3);
        assert!(!replies[2].1.metadata.contains_key("thread_id"));
        assert!(store.read_inbox_named().unwrap().is_empty());
    }

    #[test]
    fn dialog_last_for_a_new_follow_up_shows_only_its_localized_thread() {
        use crate::daemon::{effects::FsSessionEffects, request::handle_dialog_request};

        let dir = tempfile::tempdir().unwrap();
        let store = MessageStore::new(dir.path().into());
        let chamber_id = crate::hub::discovery::encode_id(dir.path());
        let root = store
            .send_in(&threaded_message(
                &format!("parent attachment: /api/chambers/{chamber_id}/files/photo.png"),
                None,
            ))
            .unwrap();
        let root_name = root.file_name().unwrap().to_string_lossy().to_string();
        let root = format!("inbox/{root_name}");
        store.archive_inbox(&[root_name]).unwrap();
        let unrelated = store
            .send_in(&threaded_message("UNRELATED CONVERSATION", None))
            .unwrap();
        store
            .archive_inbox(&[unrelated.file_name().unwrap().to_string_lossy().into()])
            .unwrap();
        store
            .send_out(&threaded_message("earlier thread answer", Some(&root)))
            .unwrap();
        store
            .send_out(&threaded_message("UNRELATED OUTBOX", None))
            .unwrap();
        store
            .send_in(&threaded_message("new follow-up", Some(&root)))
            .unwrap();

        let mut effects = FsSessionEffects::new(dir.path());
        let dialog = handle_dialog_request(
            crate::socket::DialogFilter::LastN { count: 1 },
            &[],
            &mut effects,
        );
        assert!(dialog.ok);
        assert!(dialog.message.contains("parent attachment"));
        assert!(dialog.message.contains("messages/attachments/photo.png"));
        assert!(!dialog.message.contains("/api/chambers/"));
        assert!(dialog.message.contains("earlier thread answer"));
        assert!(dialog.message.contains("new follow-up"));
        assert!(!dialog.message.contains("UNRELATED CONVERSATION"));
        assert!(!dialog.message.contains("UNRELATED OUTBOX"));
    }

    #[test]
    fn an_unread_parent_is_claimed_before_its_clock_skewed_follow_up() {
        let dir = tempfile::tempdir().unwrap();
        let store = MessageStore::new(dir.path().into());
        store.ensure_dirs().unwrap();
        let root_name = "2026-09-05T12-00-00_parent.md";
        let root = format!("inbox/{root_name}");
        std::fs::write(
            dir.path().join("messages/inbox/2026-09-05T11-00-00_child.md"),
            format!(
                "---\nfrom: human\nsubject: Follow-up\ntimestamp: 2026-09-05T11:00:00\nthread_id: {root}\n---\n\nclock-skewed child"
            ),
        )
        .unwrap();
        std::fs::write(
            dir.path().join("messages/inbox").join(root_name),
            "---\nfrom: human\nsubject: Parent\ntimestamp: 2026-09-05T12:00:00\n---\n\nunread parent",
        )
        .unwrap();

        let first = claim(dir.path()).unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].0, root_name);
        assert_eq!(first[0].1.body, "unread parent");
        let pending = store.read_inbox_named().unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].1.body, "clock-skewed child");

        send(dir.path(), threaded_message("parent answered", None)).unwrap();
        let follow_up = claim(dir.path()).unwrap();
        assert_eq!(follow_up.len(), 1);
        assert!(follow_up[0].1.body.contains("unread parent"));
        assert!(follow_up[0].1.body.contains("clock-skewed child"));
    }

    #[test]
    fn recovery_notice_stays_in_the_interrupted_thread() {
        let dir = tempfile::tempdir().unwrap();
        let store = MessageStore::new(dir.path().into());
        let root = store.send_in(&threaded_message("root", None)).unwrap();
        let root = format!("inbox/{}", root.file_name().unwrap().to_string_lossy());
        store
            .archive_inbox(&[root.strip_prefix("inbox/").unwrap().into()])
            .unwrap();
        store
            .send_in(&threaded_message("unfinished follow-up", Some(&root)))
            .unwrap();

        assert_eq!(claim(dir.path()).unwrap().len(), 1);
        recover(dir.path()).unwrap();
        recover(dir.path()).unwrap();
        let notices = store.read_outbox_named().unwrap();
        assert_eq!(notices.len(), 1);
        assert_eq!(
            notices[0].1.metadata.get("thread_id").map(String::as_str),
            Some(root.as_str())
        );
        assert!(notices[0].1.body.contains("resend"));
    }
}
