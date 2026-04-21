use super::*;

#[tokio::test]
async fn watcher_emits_new_message_event_with_chamber_id() {
    let dir = tempfile::tempdir().unwrap();
    crate::message::ensure_dirs(dir.path()).unwrap();

    let (tx, mut rx) = tokio::sync::broadcast::channel::<SseEvent>(16);
    let reg = WatcherRegistry::new();
    reg.ensure_watching("cham-1".into(), dir.path(), tx.clone());

    // Give the OS watcher a moment to register before writing the file.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let msg = crate::message::Message {
        from: "tester".into(),
        subject: "hi".into(),
        body: "yo".into(),
        timestamp: chrono::Local::now().naive_local(),
        metadata: Default::default(),
    };
    crate::message::write_message(dir.path(), "inbox", &msg).unwrap();

    // Wait up to 3 seconds for the event (notify + fs flush is racy)
    let event = tokio::time::timeout(std::time::Duration::from_secs(3), rx.recv())
        .await
        .expect("timed out waiting for watcher event")
        .expect("channel closed");

    match event {
        SseEvent::NewMessage {
            chamber_id,
            direction,
            ..
        } => {
            assert_eq!(chamber_id, "cham-1");
            assert_eq!(direction, "inbox");
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

    reg.ensure_watching("x".into(), dir.path(), tx);
    assert_eq!(reg.inner.lock().unwrap().len(), 1);
}
