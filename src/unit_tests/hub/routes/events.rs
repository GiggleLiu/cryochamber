use super::*;

use axum::response::IntoResponse;

/// The mailbox id every helper-built event carries — same shape as the ids
/// `GET /api/chambers/{id}/messages` returns (`<source>/<filename>`).
const MESSAGE_ID: &str = "inbox/2026-08-15T10-00-00_hi_0001.md";

fn new_message(chamber_id: &str, body: &str) -> SseEvent {
    SseEvent::NewMessage {
        id: MESSAGE_ID.into(),
        chamber_id: chamber_id.into(),
        direction: "inbox".into(),
        from: "x".into(),
        subject: "".into(),
        body: body.into(),
        timestamp: "t".into(),
        is_question: false,
    }
}

/// Drain the SSE response body until `events` `\n\n`-terminated frames have
/// arrived (or 2s elapse), returning the raw wire text.
async fn drain(response: axum::response::Response, events: usize) -> String {
    let mut stream = response.into_body().into_data_stream();
    let mut text = String::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
    while text.matches("\n\n").count() < events {
        let chunk = tokio::time::timeout_at(deadline, stream.next()).await;
        match chunk {
            Ok(Some(Ok(bytes))) => text.push_str(&String::from_utf8_lossy(&bytes)),
            Ok(Some(Err(e))) => panic!("stream error: {e}"),
            Ok(None) => break,
            Err(_) => break, // timed out: return what we have
        }
    }
    text
}

#[tokio::test]
async fn invite_stream_only_carries_scoped_chambers() {
    let dir = tempfile::tempdir().unwrap();
    let app = Arc::new(AppState::local_only(dir.path().to_path_buf()));
    let role = crate::hub::tokens::Role::Invite {
        name: "Alice".into(),
        chambers: vec!["mine".into()],
    };

    let sse = get_events(State(app.clone()), Some(axum::Extension(role)), None, None).await;
    let response = sse.into_response();

    app.tx.send(new_message("mine", "visible")).unwrap();
    app.tx.send(new_message("other", "SECRET")).unwrap();
    app.tx.send(SseEvent::IndexChanged).unwrap();

    let text = drain(response, 2).await;

    assert!(text.contains("visible"), "scoped message missing: {text}");
    assert!(
        text.contains("event: index"),
        "index event must reach every client: {text}"
    );
    assert!(
        !text.contains("SECRET"),
        "out-of-scope chamber leaked into the invite stream: {text}"
    );
    assert!(!text.contains("\"chamber_id\":\"other\""), "got {text}");
}

#[tokio::test]
async fn owner_and_open_mode_streams_are_unfiltered() {
    let dir = tempfile::tempdir().unwrap();
    let app = Arc::new(AppState::local_only(dir.path().to_path_buf()));

    for role in [Some(axum::Extension(crate::hub::tokens::Role::Owner)), None] {
        let response = get_events(State(app.clone()), role, None, None)
            .await
            .into_response();
        app.tx.send(new_message("mine", "visible")).unwrap();
        app.tx.send(new_message("other", "also-visible")).unwrap();

        let text = drain(response, 2).await;
        assert!(text.contains("visible"), "got {text}");
        assert!(text.contains("also-visible"), "got {text}");
    }
}

#[tokio::test]
async fn message_frames_carry_the_mailbox_message_id() {
    // The client dedupes a pushed message against the fetched messages list by
    // id, so the wire frame must carry the real mailbox id, not a synthetic one.
    let dir = tempfile::tempdir().unwrap();
    let app = Arc::new(AppState::local_only(dir.path().to_path_buf()));

    let response = get_events(State(app.clone()), None, None, None)
        .await
        .into_response();
    app.tx.send(new_message("mine", "visible")).unwrap();

    let text = drain(response, 1).await;
    assert!(
        text.contains(&format!("\"id\":\"{MESSAGE_ID}\"")),
        "message frame must carry the mailbox id: {text}"
    );
}

#[tokio::test]
async fn invite_stream_filters_status_and_log_events_too() {
    let dir = tempfile::tempdir().unwrap();
    let app = Arc::new(AppState::local_only(dir.path().to_path_buf()));
    let role = crate::hub::tokens::Role::Invite {
        name: "Alice".into(),
        chambers: vec!["mine".into()],
    };

    let response = get_events(State(app.clone()), Some(axum::Extension(role)), None, None)
        .await
        .into_response();

    app.tx
        .send(SseEvent::LogLine {
            chamber_id: "other".into(),
            line: "SECRET-LOG".into(),
        })
        .unwrap();
    app.tx
        .send(SseEvent::StatusChange {
            chamber_id: "other".into(),
        })
        .unwrap();
    app.tx
        .send(SseEvent::StatusChange {
            chamber_id: "mine".into(),
        })
        .unwrap();

    let text = drain(response, 1).await;
    assert!(!text.contains("SECRET-LOG"), "log line leaked: {text}");
    assert!(!text.contains("\"chamber_id\":\"other\""), "got {text}");
    assert!(text.contains("\"chamber_id\":\"mine\""), "got {text}");
}

#[tokio::test]
async fn revoking_an_invite_silences_its_live_stream() {
    // An SSE stream is long-lived: the guard only runs once, at connect time.
    // Revocation must therefore be enforced per event against the live store,
    // otherwise a revoked guest keeps receiving chamber traffic indefinitely.
    let dir = tempfile::tempdir().unwrap();
    let app = Arc::new(AppState::local_only(dir.path().to_path_buf()));

    let mut tf = crate::hub::tokens::TokenFile::default();
    tf.ensure_owner().unwrap();
    let alice = tf
        .create_invite("Alice", vec!["mine".into()])
        .unwrap()
        .token;
    let path = dir.path().join("tokens.json");
    crate::hub::tokens::save_tokens(&path, &tf).unwrap();
    let ctx = crate::hub::auth::AuthCtx::load(&path).unwrap();

    let role = crate::hub::tokens::Role::Invite {
        name: "Alice".into(),
        chambers: vec!["mine".into()],
    };
    let response = get_events(
        State(app.clone()),
        Some(axum::Extension(role)),
        Some(axum::Extension(ctx.clone())),
        Some(axum::Extension(crate::hub::auth::BearerToken(alice))),
    )
    .await
    .into_response();
    let mut stream = response.into_body().into_data_stream();

    // Positive path first: the stream really is delivering before revocation.
    app.tx.send(new_message("mine", "before-revoke")).unwrap();
    let first = tokio::time::timeout(std::time::Duration::from_secs(2), stream.next())
        .await
        .expect("first event should arrive")
        .expect("stream open")
        .unwrap();
    let first = String::from_utf8_lossy(&first).into_owned();
    assert!(first.contains("before-revoke"), "got {first}");

    // Revoke through the shared store the guard reads from.
    ctx.mutate(|store| {
        anyhow::ensure!(store.revoke("Alice"), "Alice should have been active");
        Ok(())
    })
    .expect("revoke should persist");

    app.tx.send(new_message("mine", "AFTER-REVOKE")).unwrap();
    let mut tail = String::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(500);
    while let Ok(Some(Ok(bytes))) = tokio::time::timeout_at(deadline, stream.next()).await {
        tail.push_str(&String::from_utf8_lossy(&bytes));
    }
    assert!(
        !tail.contains("AFTER-REVOKE"),
        "revoked invite kept receiving events: {tail}"
    );
}

#[tokio::test]
async fn broadcast_multiplexes_by_chamber_id() {
    let (tx, mut rx_a) = tokio::sync::broadcast::channel::<SseEvent>(16);
    let mut rx_b = tx.subscribe();
    tx.send(SseEvent::StatusChange {
        chamber_id: "alpha".into(),
    })
    .unwrap();
    let a = rx_a.recv().await.unwrap();
    let b = rx_b.recv().await.unwrap();
    match (a, b) {
        (SseEvent::StatusChange { chamber_id: ca }, SseEvent::StatusChange { chamber_id: cb }) => {
            assert_eq!(ca, "alpha");
            assert_eq!(cb, "alpha");
        }
        _ => panic!("expected StatusChange"),
    }
}
