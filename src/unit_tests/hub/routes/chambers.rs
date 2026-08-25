use super::*;

#[tokio::test]
async fn get_chambers_lists_workspace_scans() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("alpha")).unwrap();
    let cfg = crate::config::CryoConfig::default();
    crate::config::save_config(&dir.path().join("alpha").join("cryo.toml"), &cfg).unwrap();

    let app = Arc::new(AppState::local_only(dir.path().to_path_buf()));
    app.refresh();

    let Json(v) = get_chambers(State(app), None).await;
    let arr = v.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["name"], "alpha");
}

#[tokio::test]
async fn refresh_picks_up_new_chamber() {
    let dir = tempfile::tempdir().unwrap();
    let app = Arc::new(AppState::local_only(dir.path().to_path_buf()));
    let Json(initial) = get_chambers(State(app.clone()), None).await;
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

    let app = Arc::new(AppState::local_only(dir.path().to_path_buf()));
    let mut stale = crate::hub::discovery::scan_workspace(dir.path());
    for entry in stale.values_mut() {
        entry.running = true;
        entry.completed = false;
    }
    *app.chambers.write().unwrap() = stale;

    let Json(v) = get_chambers(State(app), None).await;
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

    let app = Arc::new(AppState::local_only(dir.path().to_path_buf()));
    app.refresh();

    let Json(v) = get_chambers(State(app), None).await;
    let arr = v.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["running"], true);
    assert_eq!(arr[0]["agent_running"], true);
}

#[tokio::test]
async fn chamber_list_is_filtered_for_invites() {
    // An invite must not learn that chambers outside its scope exist.
    let dir = tempfile::tempdir().unwrap();
    for name in ["alpha", "beta"] {
        let d = dir.path().join(name);
        std::fs::create_dir_all(&d).unwrap();
        let cfg = crate::config::CryoConfig::default();
        crate::config::save_config(&d.join("cryo.toml"), &cfg).unwrap();
    }
    let app = Arc::new(AppState::local_only(dir.path().to_path_buf()));
    app.refresh();
    let beta_id = app
        .chambers
        .read()
        .unwrap()
        .iter()
        .find(|(_, c)| c.name == "beta")
        .map(|(id, _)| id.clone())
        .expect("beta in index");

    let ids_of = |v: &serde_json::Value| -> Vec<String> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|c| c["id"].as_str().unwrap().to_string())
            .collect()
    };

    let role = crate::hub::tokens::Role::Invite {
        name: "Alice".into(),
        chambers: vec![beta_id.clone()],
    };
    let Json(scoped) = get_chambers(State(app.clone()), Some(axum::Extension(role))).await;
    assert_eq!(ids_of(&scoped), vec![beta_id]);

    // Owner and open mode still see both chambers.
    let Json(as_owner) = get_chambers(
        State(app.clone()),
        Some(axum::Extension(crate::hub::tokens::Role::Owner)),
    )
    .await;
    assert_eq!(ids_of(&as_owner).len(), 2);
    let Json(open) = get_chambers(State(app), None).await;
    assert_eq!(ids_of(&open).len(), 2);
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
    use crate::test_support::EnvVarGuard;
    use axum::{extract::State, response::Json};
    use serde_json::json;
    use std::sync::Arc;

    #[tokio::test]
    async fn creates_new_chamber_returns_201_and_id() {
        let dir = tempfile::tempdir().unwrap();
        let app = Arc::new(AppState::local_only(dir.path().to_path_buf()));
        app.refresh();

        let resp = post_new(
            State(app.clone()),
            Json(NewChamberPayload {
                name: "alpha".into(),
                api_key_provider: None,
                api_key: None,
                model: None,
            }),
        )
        .await;

        let (status, Json(body)) = resp;
        assert_eq!(status, axum::http::StatusCode::CREATED);
        assert!(body.get("id").is_some());

        let alpha = dir.path().join("alpha");
        assert!(alpha.join("cryo.toml").exists());
        assert!(!alpha.join("AGENTS.md").exists());
        assert!(!alpha.join("CLAUDE.md").exists());
        assert!(alpha.join("plan.md").exists());
        assert!(alpha.join("README.md").exists());
        assert!(alpha.join("NOTES.md").exists());
        assert!(alpha.join("messages/inbox").exists());
        assert!(alpha.join("messages/outbox").exists());

        let idx = app.chambers.read().unwrap();
        assert!(idx.values().any(|c| c.name == "alpha"));
    }

    #[tokio::test]
    async fn global_create_remembers_new_chamber_in_registry() {
        let state_home = tempfile::tempdir().unwrap();
        let _state = EnvVarGuard::set_path("XDG_STATE_HOME", state_home.path());
        let dir = tempfile::tempdir().unwrap();
        let app = Arc::new(AppState::with_discovery_options(
            dir.path().to_path_buf(),
            crate::hub::discovery::DiscoveryOptions::all_chambers(),
        ));

        let (status, Json(_body)) = post_new(
            State(app.clone()),
            Json(NewChamberPayload {
                name: "alpha".into(),
                api_key_provider: None,
                api_key: None,
                model: None,
            }),
        )
        .await;

        assert_eq!(status, axum::http::StatusCode::CREATED);
        let alpha = dir.path().join("alpha");
        assert!(alpha.join("cryo.toml").exists());
        assert!(app
            .chambers
            .read()
            .unwrap()
            .values()
            .any(|c| { c.name == "alpha" && c.path == alpha.canonicalize().unwrap() }));
        assert!(crate::registry::list()
            .unwrap()
            .iter()
            .any(|entry| entry.dir == alpha.display().to_string()));
    }

    #[tokio::test]
    async fn creates_new_chamber_without_api_key_provider() {
        let dir = tempfile::tempdir().unwrap();
        let app = Arc::new(AppState::local_only(dir.path().to_path_buf()));

        let (status, Json(_body)) = post_new(
            State(app),
            Json(NewChamberPayload {
                name: "alpha".into(),
                api_key_provider: None,
                api_key: None,
                model: None,
            }),
        )
        .await;

        assert_eq!(status, axum::http::StatusCode::CREATED);
        let cfg = crate::config::load_config(&dir.path().join("alpha").join("cryo.toml"))
            .unwrap()
            .unwrap();
        assert_eq!(cfg.agent, "pi");
        assert!(
            cfg.provider.is_none(),
            "folded provider section should be optional unless filled in"
        );
        assert!(cfg.providers.is_empty());
    }

    #[tokio::test]
    async fn creates_new_chamber_with_configured_host_agent() {
        let dir = tempfile::tempdir().unwrap();
        let app = Arc::new(AppState::with_discovery_options_and_agent(
            dir.path().to_path_buf(),
            crate::hub::discovery::DiscoveryOptions::local_only(),
            "codex --model gpt-5".to_string(),
        ));

        let (status, Json(_body)) = post_new(
            State(app),
            Json(NewChamberPayload {
                name: "alpha".into(),
                api_key_provider: None,
                api_key: None,
                model: None,
            }),
        )
        .await;

        assert_eq!(status, axum::http::StatusCode::CREATED);
        let cfg = crate::config::load_config(&dir.path().join("alpha/cryo.toml"))
            .unwrap()
            .unwrap();
        assert_eq!(cfg.agent, "codex --model gpt-5");
    }

    #[tokio::test]
    async fn opencode_host_default_keeps_opencode_provider_environment() {
        let dir = tempfile::tempdir().unwrap();
        let app = Arc::new(AppState::with_discovery_options_and_agent(
            dir.path().to_path_buf(),
            crate::hub::discovery::DiscoveryOptions::local_only(),
            "opencode".to_string(),
        ));

        let (status, Json(_body)) = post_new(
            State(app),
            Json(NewChamberPayload {
                name: "alpha".into(),
                api_key_provider: Some("anthropic".into()),
                api_key: Some("sk-test".into()),
                model: Some("claude-sonnet".into()),
            }),
        )
        .await;

        assert_eq!(status, axum::http::StatusCode::CREATED);
        let cfg = crate::config::load_config(&dir.path().join("alpha/cryo.toml"))
            .unwrap()
            .unwrap();
        assert_eq!(cfg.agent, "opencode");
        let env = &cfg.provider.unwrap().env;
        assert_eq!(env.get("OPENCODE_PROVIDER").unwrap(), "anthropic");
        assert_eq!(env.get("OPENCODE_MODEL").unwrap(), "claude-sonnet");
    }

    #[tokio::test]
    async fn creates_new_chamber_with_selected_api_key_provider() {
        let dir = tempfile::tempdir().unwrap();
        let app = Arc::new(AppState::local_only(dir.path().to_path_buf()));

        let (status, Json(_body)) = post_new(
            State(app),
            Json(NewChamberPayload {
                name: "alpha".into(),
                api_key_provider: Some("openai".into()),
                api_key: Some("sk-openai-test".into()),
                model: Some("gpt-5".into()),
            }),
        )
        .await;

        assert_eq!(status, axum::http::StatusCode::CREATED);
        let cfg = crate::config::load_config(&dir.path().join("alpha").join("cryo.toml"))
            .unwrap()
            .unwrap();
        assert_eq!(cfg.agent, "pi --provider openai --model gpt-5");
        let provider = cfg
            .provider
            .as_ref()
            .expect("provider should be configured");
        assert_eq!(provider.name, "openai");
        assert!(cfg.providers.is_empty());
        assert!(!provider.env.contains_key("OPENCODE_PROVIDER"));
        assert!(!provider.env.contains_key("OPENCODE_MODEL"));
        assert_eq!(
            provider.env.get("OPENAI_API_KEY").unwrap(),
            "sk-openai-test"
        );
    }

    #[tokio::test]
    async fn creates_new_chamber_with_custom_api_key_provider() {
        let dir = tempfile::tempdir().unwrap();
        let app = Arc::new(AppState::local_only(dir.path().to_path_buf()));

        let (status, Json(_body)) = post_new(
            State(app),
            Json(NewChamberPayload {
                name: "alpha".into(),
                api_key_provider: Some("my-provider".into()),
                api_key: Some("sk-custom-test".into()),
                model: Some("my-model".into()),
            }),
        )
        .await;

        assert_eq!(status, axum::http::StatusCode::CREATED);
        let cfg = crate::config::load_config(&dir.path().join("alpha").join("cryo.toml"))
            .unwrap()
            .unwrap();
        let provider = cfg.provider.as_ref().expect("provider");
        assert_eq!(cfg.agent, "pi --provider my-provider --model my-model");
        assert_eq!(provider.name, "my-provider");
        assert!(!provider.env.contains_key("OPENCODE_PROVIDER"));
        assert!(!provider.env.contains_key("OPENCODE_MODEL"));
        assert_eq!(
            provider.env.get("MY_PROVIDER_API_KEY").unwrap(),
            "sk-custom-test"
        );
    }

    #[tokio::test]
    async fn empty_name_rejected_400() {
        let dir = tempfile::tempdir().unwrap();
        let app = Arc::new(AppState::local_only(dir.path().to_path_buf()));
        let (status, Json(body)) = post_new(
            State(app),
            Json(NewChamberPayload {
                name: "".into(),
                api_key_provider: None,
                api_key: None,
                model: None,
            }),
        )
        .await;
        assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "name is empty");
    }

    #[tokio::test]
    async fn name_with_slash_rejected_400() {
        let dir = tempfile::tempdir().unwrap();
        let app = Arc::new(AppState::local_only(dir.path().to_path_buf()));
        let (status, Json(body)) = post_new(
            State(app),
            Json(NewChamberPayload {
                name: "a/b".into(),
                api_key_provider: None,
                api_key: None,
                model: None,
            }),
        )
        .await;
        assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "name contains illegal characters");
    }

    #[tokio::test]
    async fn empty_api_key_rejected_400() {
        let dir = tempfile::tempdir().unwrap();
        let app = Arc::new(AppState::local_only(dir.path().to_path_buf()));
        let (status, Json(body)) = post_new(
            State(app),
            Json(NewChamberPayload {
                name: "alpha".into(),
                api_key_provider: Some("anthropic".into()),
                api_key: Some("   ".into()),
                model: None,
            }),
        )
        .await;
        assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "api key is empty");
    }

    #[tokio::test]
    async fn invalid_api_key_provider_rejected_400() {
        let dir = tempfile::tempdir().unwrap();
        let app = Arc::new(AppState::local_only(dir.path().to_path_buf()));
        let (status, Json(body)) = post_new(
            State(app),
            Json(NewChamberPayload {
                name: "alpha".into(),
                api_key_provider: Some("../escape".into()),
                api_key: Some("sk-test".into()),
                model: None,
            }),
        )
        .await;
        assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
        assert_eq!(
            body["error"],
            "api key provider contains illegal characters"
        );
    }

    #[tokio::test]
    async fn collision_with_existing_chamber_rejected_400() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("alpha")).unwrap();
        let cfg = crate::config::CryoConfig::default();
        crate::config::save_config(&dir.path().join("alpha").join("cryo.toml"), &cfg).unwrap();

        let app = Arc::new(AppState::local_only(dir.path().to_path_buf()));
        app.refresh();
        let (status, Json(body)) = post_new(
            State(app),
            Json(NewChamberPayload {
                name: "alpha".into(),
                api_key_provider: None,
                api_key: None,
                model: None,
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
        let app = Arc::new(AppState::local_only(hub_dir.path().to_path_buf()));
        let _ = post_new(
            State(app),
            Json(NewChamberPayload {
                name: "x".into(),
                api_key_provider: None,
                api_key: None,
                model: None,
            }),
        )
        .await;

        for file in ["plan.md", "README.md", "NOTES.md"] {
            let cli_bytes = std::fs::read(cli_dir.path().join(file)).unwrap();
            let hub_bytes = std::fs::read(hub_dir.path().join("x").join(file)).unwrap();
            if file == "README.md" {
                assert_eq!(!cli_bytes.is_empty(), !hub_bytes.is_empty());
            } else {
                assert_eq!(
                    cli_bytes, hub_bytes,
                    "scaffold drift: {file} differs between cli and hub paths"
                );
            }
        }

        let _ = json!({});
    }

    #[tokio::test]
    async fn create_dir_failure_returns_500() {
        let dir = tempfile::tempdir().unwrap();
        let workspace_file = dir.path().join("workspace-file");
        std::fs::write(&workspace_file, "not a directory").unwrap();

        let app = Arc::new(AppState::local_only(workspace_file));
        let (status, Json(body)) = post_new(
            State(app),
            Json(NewChamberPayload {
                name: "alpha".into(),
                api_key_provider: None,
                api_key: None,
                model: None,
            }),
        )
        .await;

        assert_eq!(status, axum::http::StatusCode::INTERNAL_SERVER_ERROR);
        assert!(body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("failed to create chamber root"));
    }
}

mod path_under {
    use std::path::Path;

    use super::super::path_under as p;

    #[test]
    fn accepts_child_path() {
        assert!(p(
            Path::new("/tmp/workspace"),
            Path::new("/tmp/workspace/alpha")
        ));
    }

    #[test]
    fn rejects_same_path_and_parent_components() {
        assert!(!p(Path::new("/tmp/workspace"), Path::new("/tmp/workspace")));
        assert!(!p(
            Path::new("/tmp/workspace"),
            Path::new("/tmp/workspace/../escape")
        ));
    }
}
