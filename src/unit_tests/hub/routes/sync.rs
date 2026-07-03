use super::*;
use crate::hub::discovery::encode_id;

#[tokio::test]
async fn get_sync_returns_empty_for_unconfigured_chamber() {
    let dir = tempfile::tempdir().unwrap();
    let alpha = dir.path().join("alpha");
    std::fs::create_dir_all(&alpha).unwrap();
    let cfg = crate::config::CryoConfig::default();
    crate::config::save_config(&alpha.join("cryo.toml"), &cfg).unwrap();

    let app = Arc::new(AppState::local_only(dir.path().to_path_buf()));
    app.refresh();
    let id = encode_id(&alpha.canonicalize().unwrap());
    let res = get_sync(State(app), AxumPath(id)).await.unwrap();
    assert_eq!(res.0, serde_json::json!([]));
}

#[tokio::test]
async fn get_sync_reports_configured_zulip_backend() {
    let dir = tempfile::tempdir().unwrap();
    let alpha = dir.path().join("alpha");
    std::fs::create_dir_all(&alpha).unwrap();
    let cfg = crate::config::CryoConfig::default();
    crate::config::save_config(&alpha.join("cryo.toml"), &cfg).unwrap();
    let state = crate::zulip_sync::ZulipSyncState {
        site: "https://z.example.com".into(),
        stream: "notes".into(),
        stream_id: 1,
        self_email: "bot@z.example.com".into(),
        topic: None,
        last_message_id: None,
        last_pushed_session: None,
    };
    crate::zulip_sync::save_sync_state(&alpha.join("zulip-sync.json"), &state).unwrap();

    let app = Arc::new(AppState::local_only(dir.path().to_path_buf()));
    app.refresh();
    let id = encode_id(&alpha.canonicalize().unwrap());
    let res = get_sync(State(app), AxumPath(id)).await.unwrap();
    let arr = res.0.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["backend"], "zulip");
    assert_eq!(
        arr[0]["target"],
        "https://z.example.com · notes / cryochamber"
    );
}

#[tokio::test]
async fn post_sync_action_rejects_unknown_backend() {
    let dir = tempfile::tempdir().unwrap();
    let alpha = dir.path().join("alpha");
    std::fs::create_dir_all(&alpha).unwrap();
    let cfg = crate::config::CryoConfig::default();
    crate::config::save_config(&alpha.join("cryo.toml"), &cfg).unwrap();
    let app = Arc::new(AppState::local_only(dir.path().to_path_buf()));
    app.refresh();
    let id = encode_id(&alpha.canonicalize().unwrap());
    let err = post_sync_action(State(app), AxumPath((id, "bogus".into(), "start".into())))
        .await
        .unwrap_err();
    assert_eq!(err, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn post_sync_action_rejects_unknown_verb() {
    let dir = tempfile::tempdir().unwrap();
    let alpha = dir.path().join("alpha");
    std::fs::create_dir_all(&alpha).unwrap();
    let cfg = crate::config::CryoConfig::default();
    crate::config::save_config(&alpha.join("cryo.toml"), &cfg).unwrap();
    let app = Arc::new(AppState::local_only(dir.path().to_path_buf()));
    app.refresh();
    let id = encode_id(&alpha.canonicalize().unwrap());
    let err = post_sync_action(State(app), AxumPath((id, "gh".into(), "dance".into())))
        .await
        .unwrap_err();
    assert_eq!(err, StatusCode::BAD_REQUEST);
}
