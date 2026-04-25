use super::*;

#[tokio::test]
async fn get_chambers_lists_workspace_scans() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("alpha")).unwrap();
    let cfg = crate::config::CryoConfig::default();
    crate::config::save_config(&dir.path().join("alpha").join("cryo.toml"), &cfg).unwrap();

    let app = Arc::new(AppState::new(dir.path().to_path_buf()));
    app.refresh();

    let Json(v) = get_chambers(State(app)).await;
    let arr = v.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["name"], "alpha");
}

#[tokio::test]
async fn refresh_picks_up_new_chamber() {
    let dir = tempfile::tempdir().unwrap();
    let app = Arc::new(AppState::new(dir.path().to_path_buf()));
    let Json(initial) = get_chambers(State(app.clone())).await;
    assert_eq!(initial.as_array().unwrap().len(), 0);

    let new_dir = dir.path().join("beta");
    std::fs::create_dir_all(&new_dir).unwrap();
    let cfg = crate::config::CryoConfig::default();
    crate::config::save_config(&new_dir.join("cryo.toml"), &cfg).unwrap();

    let Json(after) = post_refresh(State(app)).await;
    let arr = after.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["name"], "beta");
}

#[tokio::test]
async fn get_chambers_refreshes_runtime_fields_from_disk() {
    let dir = tempfile::tempdir().unwrap();
    let alpha = dir.path().join("alpha");
    std::fs::create_dir_all(&alpha).unwrap();
    let cfg = crate::config::CryoConfig::default();
    crate::config::save_config(&alpha.join("cryo.toml"), &cfg).unwrap();
    crate::state::save_state(
        &crate::state::state_path(&alpha),
        &crate::state::CryoState {
            session_number: 5,
            pid: None,
            agent_override: None,
            max_session_duration_override: None,
            last_report_time: None,
            provider_index: None,
            instance_id: None,
            session_active: false,
            previous_session_crashed: false,
        },
    )
    .unwrap();
    std::fs::write(
        alpha.join("cryo.log"),
        "--- CRYO SESSION 5 | 2026-04-20T16:32:59Z ---\n\
         task: Continue the plan\n\
         agent: opencode\n\
         [16:33:52] hibernate: plan complete, exit=0, summary=\"done\"\n\
         --- CRYO END ---\n",
    )
    .unwrap();

    let app = Arc::new(AppState::new(dir.path().to_path_buf()));
    let mut stale = crate::hub::discovery::scan_workspace(dir.path());
    for entry in stale.values_mut() {
        entry.running = true;
        entry.completed = false;
    }
    *app.chambers.write().unwrap() = stale;

    let Json(v) = get_chambers(State(app)).await;
    let arr = v.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["running"], false);
    assert_eq!(arr[0]["completed"], true);
    assert_eq!(arr[0]["session"], 5);
}

#[tokio::test]
async fn get_chambers_includes_agent_running_from_disk() {
    let dir = tempfile::tempdir().unwrap();
    let alpha = dir.path().join("alpha");
    std::fs::create_dir_all(&alpha).unwrap();
    let cfg = crate::config::CryoConfig::default();
    crate::config::save_config(&alpha.join("cryo.toml"), &cfg).unwrap();
    std::fs::write(
        alpha.join("timer.json"),
        format!(
            r#"{{
                "session_number": 6,
                "pid": {},
                "session_active": true
            }}"#,
            std::process::id()
        ),
    )
    .unwrap();

    let app = Arc::new(AppState::new(dir.path().to_path_buf()));
    app.refresh();

    let Json(v) = get_chambers(State(app)).await;
    let arr = v.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["running"], true);
    assert_eq!(arr[0]["agent_running"], true);
}

mod validate_name {
    use crate::hub::routes::chambers::validate_chamber_name as v;

    #[test]
    fn accepts_simple_name() {
        assert!(v("alpha").is_ok());
        assert!(v("interstellar-traveler").is_ok());
        assert!(v("a_b_c-1").is_ok());
    }

    #[test]
    fn rejects_empty() {
        let err = v("").unwrap_err();
        assert!(err.contains("empty"));
    }

    #[test]
    fn rejects_too_long() {
        let n = "x".repeat(65);
        let err = v(&n).unwrap_err();
        assert!(err.contains("too long"));
    }

    #[test]
    fn rejects_slash_or_backslash() {
        assert!(v("a/b").is_err());
        assert!(v("a\\b").is_err());
    }

    #[test]
    fn rejects_dotdot_and_leading_dot() {
        assert!(v("..").is_err());
        assert!(v(".alpha").is_err());
    }

    #[test]
    fn rejects_whitespace() {
        assert!(v(" alpha").is_err());
        assert!(v("alpha beta").is_err());
        assert!(v("alpha\t").is_err());
    }

    #[test]
    fn rejects_control_chars() {
        assert!(v("alpha\nbeta").is_err());
        assert!(v("alpha\u{0007}").is_err());
    }
}

mod post_new {
    use crate::hub::routes::chambers::{post_new, NewChamberPayload};
    use crate::hub::state::AppState;
    use axum::{extract::State, response::Json};
    use serde_json::json;
    use std::sync::Arc;

    #[tokio::test]
    async fn creates_new_chamber_returns_201_and_id() {
        let dir = tempfile::tempdir().unwrap();
        let app = Arc::new(AppState::new(dir.path().to_path_buf()));
        app.refresh();

        let resp = post_new(
            State(app.clone()),
            Json(NewChamberPayload {
                name: "alpha".into(),
            }),
        )
        .await;

        let (status, Json(body)) = resp;
        assert_eq!(status, axum::http::StatusCode::CREATED);
        assert!(body.get("id").is_some());

        let alpha = dir.path().join("alpha");
        assert!(alpha.join("cryo.toml").exists());
        assert!(alpha.join("AGENTS.md").exists());
        assert!(alpha.join("plan.md").exists());
        assert!(alpha.join("README.md").exists());
        assert!(alpha.join("NOTES.md").exists());
        assert!(alpha.join("messages/inbox").exists());
        assert!(alpha.join("messages/outbox").exists());

        let idx = app.chambers.read().unwrap();
        assert!(idx.values().any(|c| c.name == "alpha"));
    }

    #[tokio::test]
    async fn empty_name_rejected_400() {
        let dir = tempfile::tempdir().unwrap();
        let app = Arc::new(AppState::new(dir.path().to_path_buf()));
        let (status, Json(body)) = post_new(
            State(app),
            Json(NewChamberPayload { name: "".into() }),
        )
        .await;
        assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "name is empty");
    }

    #[tokio::test]
    async fn name_with_slash_rejected_400() {
        let dir = tempfile::tempdir().unwrap();
        let app = Arc::new(AppState::new(dir.path().to_path_buf()));
        let (status, Json(body)) = post_new(
            State(app),
            Json(NewChamberPayload {
                name: "a/b".into(),
            }),
        )
        .await;
        assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "name contains illegal characters");
    }

    #[tokio::test]
    async fn collision_with_existing_chamber_rejected_400() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("alpha")).unwrap();
        let cfg = crate::config::CryoConfig::default();
        crate::config::save_config(&dir.path().join("alpha").join("cryo.toml"), &cfg).unwrap();

        let app = Arc::new(AppState::new(dir.path().to_path_buf()));
        app.refresh();
        let (status, Json(body)) = post_new(
            State(app),
            Json(NewChamberPayload {
                name: "alpha".into(),
            }),
        )
        .await;
        assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "chamber already exists");
    }

    #[tokio::test]
    async fn scaffold_parity_with_cryo_init() {
        let cli_dir = tempfile::tempdir().unwrap();
        crate::protocol::scaffold_chamber(cli_dir.path(), "opencode").unwrap();

        let hub_dir = tempfile::tempdir().unwrap();
        let app = Arc::new(AppState::new(hub_dir.path().to_path_buf()));
        let _ = post_new(
            State(app),
            Json(NewChamberPayload { name: "x".into() }),
        )
        .await;

        for file in ["cryo.toml", "AGENTS.md", "plan.md", "README.md", "NOTES.md"] {
            let cli_bytes = std::fs::read(cli_dir.path().join(file)).unwrap();
            let hub_bytes = std::fs::read(hub_dir.path().join("x").join(file)).unwrap();
            if file == "README.md" {
                assert_eq!(cli_bytes.len() > 0, hub_bytes.len() > 0);
            } else {
                assert_eq!(
                    cli_bytes, hub_bytes,
                    "scaffold drift: {file} differs between cli and hub paths"
                );
            }
        }

        let _ = json!({});
    }
}
