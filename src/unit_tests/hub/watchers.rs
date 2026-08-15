use super::*;

#[test]
fn message_direction_for_path_classifies_inbox_and_outbox_paths() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("messages").join("inbox");
    let outbox = dir.path().join("messages").join("outbox");

    assert_eq!(
        message_direction_for_path(&inbox.join("incoming.md"), &inbox, &outbox),
        Some(MessageDirection::Inbox)
    );
    assert_eq!(
        message_direction_for_path(&outbox.join("reply.md"), &inbox, &outbox),
        Some(MessageDirection::Outbox)
    );
    assert_eq!(
        message_direction_for_path(&dir.path().join("elsewhere.md"), &inbox, &outbox),
        None
    );
}

#[test]
fn classify_message_event_paths_filters_kind_extension_and_direction() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("messages").join("inbox");
    let outbox = dir.path().join("messages").join("outbox");
    let event = NotifyEvent {
        kind: EventKind::Create(notify::event::CreateKind::File),
        paths: vec![
            inbox.join("incoming.md"),
            outbox.join("reply.md"),
            inbox.join("ignore.txt"),
            dir.path().join("elsewhere.md"),
        ],
        attrs: Default::default(),
    };

    let classified = classify_message_event_paths(&event, &inbox, &outbox);

    assert_eq!(
        classified,
        vec![
            MessageEventPath {
                path: inbox.join("incoming.md"),
                direction: MessageDirection::Inbox,
            },
            MessageEventPath {
                path: outbox.join("reply.md"),
                direction: MessageDirection::Outbox,
            },
        ]
    );
}

#[tokio::test]
async fn watcher_emits_new_message_event_with_chamber_id_and_mailbox_id() {
    let dir = tempfile::tempdir().unwrap();
    crate::message::ensure_dirs(dir.path()).unwrap();

    let (tx, mut rx) = tokio::sync::broadcast::channel::<SseEvent>(16);
    let reg = WatcherRegistry::new();
    for _ in 0..20 {
        reg.ensure_watching("cham-1".into(), dir.path(), tx.clone());
        if reg.inner.lock().unwrap().len() == 1 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!(reg.inner.lock().unwrap().len(), 1);

    // Give the OS watcher a moment to register before writing the file.
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;

    let msg = crate::message::Message {
        from: "tester".into(),
        subject: "hi".into(),
        body: "yo".into(),
        timestamp: chrono::Local::now().naive_local(),
        metadata: Default::default(),
        is_question: false,
    };
    crate::message::write_message(dir.path(), "inbox", &msg).unwrap();

    // Wait up to 3 seconds for the event (notify + fs flush is racy)
    let event = tokio::time::timeout(std::time::Duration::from_secs(3), rx.recv())
        .await
        .expect("timed out waiting for watcher event")
        .expect("channel closed");

    // The id must be the one the messages list reports for the same file, so
    // the client can dedupe the pushed message against a later refetch.
    let listed = crate::chamber_status::messages(dir.path());
    assert_eq!(listed.len(), 1);

    match event {
        SseEvent::NewMessage {
            id,
            chamber_id,
            direction,
            ..
        } => {
            assert_eq!(chamber_id, "cham-1");
            assert_eq!(direction, "inbox");
            assert!(!id.is_empty(), "watcher must emit a mailbox id");
            assert_eq!(id, listed[0].id);
        }
        other => panic!("expected NewMessage, got {:?}", other),
    }
}

#[test]
fn ensure_watching_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    crate::message::ensure_dirs(dir.path()).unwrap();
    let (tx, _rx) = tokio::sync::broadcast::channel::<SseEvent>(16);
    let reg = WatcherRegistry::new();
    reg.ensure_watching("x".into(), dir.path(), tx.clone());
    reg.ensure_watching("x".into(), dir.path(), tx);
    assert_eq!(reg.inner.lock().unwrap().len(), 1);
}

#[test]
fn drop_watcher_allows_ensure_watching_to_rebuild() {
    // Reset archives `messages/` and then re-creates it; the stale notify
    // handle is left watching the moved directory. `drop_watcher` lets the
    // refresh pass rebuild the watcher for the fresh path.
    let dir = tempfile::tempdir().unwrap();
    crate::message::ensure_dirs(dir.path()).unwrap();
    let (tx, _rx) = tokio::sync::broadcast::channel::<SseEvent>(16);
    let reg = WatcherRegistry::new();
    reg.ensure_watching("x".into(), dir.path(), tx.clone());
    assert_eq!(reg.inner.lock().unwrap().len(), 1);

    reg.drop_watcher(dir.path());
    assert_eq!(reg.inner.lock().unwrap().len(), 0);

    for _ in 0..10 {
        reg.ensure_watching("x".into(), dir.path(), tx.clone());
        if reg.inner.lock().unwrap().len() == 1 {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    assert_eq!(reg.inner.lock().unwrap().len(), 1);
}
