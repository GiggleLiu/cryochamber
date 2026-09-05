use std::collections::BTreeMap;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use cryochamber::channel::store::MessageStore;
use cryochamber::config;
use cryochamber::hub::config::HubConfig;
use cryochamber::hub::{build_router_with_config, discovery, state::AppState};
use cryochamber::message::Message;
use serde_json::{json, Value};
use tower::ServiceExt;

struct TestHub {
    _tmp: tempfile::TempDir,
    app: Arc<AppState>,
    alpha: String,
    beta: String,
}

impl TestHub {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        for name in ["alpha", "beta"] {
            let dir = tmp.path().join(name);
            std::fs::create_dir_all(&dir).unwrap();
            config::save_config(&dir.join("cryo.toml"), &config::CryoConfig::default()).unwrap();
        }
        let app = Arc::new(AppState::local_only(tmp.path().to_path_buf()));
        let mut chambers = discovery::scan_workspace(tmp.path());
        discovery::populate_runtime(&mut chambers);
        *app.chambers.write().unwrap() = chambers;
        let id = |name: &str| {
            app.chambers
                .read()
                .unwrap()
                .values()
                .find(|entry| entry.name == name)
                .unwrap()
                .id
                .clone()
        };
        Self {
            alpha: id("alpha"),
            beta: id("beta"),
            app,
            _tmp: tmp,
        }
    }

    async fn request(&self, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .body(body.map_or_else(Body::empty, |value| Body::from(value.to_string())))
            .unwrap();
        let response = build_router_with_config(self.app.clone(), HubConfig::default())
            .oneshot(request)
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, body)
    }

    async fn send(&self, chamber: &str, body: Value) -> (StatusCode, Value) {
        self.request("POST", &format!("/api/chambers/{chamber}/send"), Some(body))
            .await
    }

    fn store(&self, chamber: &str) -> MessageStore {
        let path = discovery::decode_id(chamber).unwrap();
        MessageStore::new(path)
    }
}

fn message(body: &str, thread_id: Option<&str>) -> Message {
    Message {
        from: "agent".into(),
        subject: "Reply".into(),
        body: body.into(),
        timestamp: chrono::Local::now().naive_local(),
        metadata: thread_id
            .map(|id| BTreeMap::from([("thread_id".into(), id.into())]))
            .unwrap_or_default(),
        is_question: false,
    }
}

#[tokio::test]
async fn thread_rest_api_preserves_metadata_across_archive_and_sharing() {
    let hub = TestHub::new();
    let (status, root) = hub
        .send(&hub.alpha, json!({"body": "original request"}))
        .await;
    assert_eq!(status, StatusCode::OK);
    let root = root["id"].as_str().unwrap();

    let (status, _) = hub
        .send(
            &hub.alpha,
            json!({"body": "human follow-up", "thread_id": root}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let store = hub.store(&hub.alpha);
    let agent_path = store
        .send_out(&message("private answer", Some(root)))
        .unwrap();
    let agent_id = format!(
        "outbox/{}",
        agent_path.file_name().unwrap().to_string_lossy()
    );

    let (status, shared) = hub
        .send(
            &hub.alpha,
            json!({"body": "ignored replacement", "share_message_id": agent_id}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let shared_id = shared["id"].as_str().unwrap().to_string();
    let shared = store.get(&shared_id).unwrap();
    assert_eq!(shared.body, "private answer");
    assert_eq!(
        shared.metadata.get("shared_from").map(String::as_str),
        Some(root)
    );
    assert!(!shared.metadata.contains_key("thread_id"));
    let (status, reply) = hub
        .send(
            &hub.alpha,
            json!({"body": "reply through stream copy", "thread_id": shared_id}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        store
            .get(reply["id"].as_str().unwrap())
            .unwrap()
            .metadata
            .get("thread_id")
            .map(String::as_str),
        Some(root)
    );
    assert_eq!(store.read_inbox_named().unwrap().len(), 3);
    assert_eq!(store.read_outbox_named().unwrap().len(), 2);

    let root_name = root.strip_prefix("inbox/").unwrap().to_string();
    store.archive_inbox(&[root_name]).unwrap();
    let (status, summaries) = hub
        .request("GET", &format!("/api/chambers/{}/threads", hub.alpha), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(summaries.as_array().unwrap().len(), 1);
    assert_eq!(summaries[0]["root"]["id"], root);
    assert_eq!(summaries[0]["count"], 3);
    assert!(summaries[0]["latest"].is_string());

    let (status, thread) = hub
        .request(
            "GET",
            &format!(
                "/api/chambers/{}/threads?root={}",
                hub.alpha,
                root.replace('/', "%2F")
            ),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let thread = thread.as_array().unwrap();
    assert_eq!(thread.len(), 4);
    assert!(thread.iter().any(|row| row["id"] == root));
    assert_eq!(
        thread.iter().filter(|row| row["thread_id"] == root).count(),
        3
    );
    assert!(thread.iter().all(|row| row["shared_from"].is_null()));
}

#[test]
fn thread_context_reads_history_without_previewing_pending_work() {
    let hub = TestHub::new();
    let store = hub.store(&hub.alpha);
    let root = store.send_in(&message("root", None)).unwrap();
    let root_name = root.file_name().unwrap().to_string_lossy().to_string();
    let root = format!("inbox/{root_name}");
    store.archive_inbox(&[root_name]).unwrap();
    let archived = store
        .send_out(&message("archived answer", Some(&root)))
        .unwrap();
    store
        .archive_outbox(&[archived.file_name().unwrap().to_string_lossy().into()])
        .unwrap();
    store
        .send_in(&message("still pending follow-up", Some(&root)))
        .unwrap();

    let context = store.thread_context(&root, &[]).unwrap();
    assert!(context.contains("root"));
    assert!(context.contains("archived answer"));
    assert!(!context.contains("still pending follow-up"));
}

#[tokio::test]
async fn replies_require_a_local_root_and_message_ids_cannot_traverse() {
    let hub = TestHub::new();
    let (_, alpha_root) = hub.send(&hub.alpha, json!({"body": "alpha root"})).await;
    let alpha_root = alpha_root["id"].as_str().unwrap();
    let (_, beta_root) = hub.send(&hub.beta, json!({"body": "beta root"})).await;
    let beta_root = beta_root["id"].as_str().unwrap();
    let (_, reply) = hub
        .send(
            &hub.alpha,
            json!({"body": "valid reply", "thread_id": alpha_root}),
        )
        .await;
    let reply = reply["id"].as_str().unwrap();

    let inbox_before_share = hub.store(&hub.alpha).read_inbox_named().unwrap().len();
    let (status, shared) = hub
        .send(&hub.alpha, json!({"body": "", "share_message_id": reply}))
        .await;
    assert_eq!(status, StatusCode::OK);
    let shared = hub
        .store(&hub.alpha)
        .get(shared["id"].as_str().unwrap())
        .unwrap();
    assert_eq!(shared.body, "valid reply");
    assert_eq!(
        shared.metadata.get("shared_from").map(String::as_str),
        Some(alpha_root)
    );
    assert_eq!(
        hub.store(&hub.alpha).read_inbox_named().unwrap().len(),
        inbox_before_share,
        "sharing must not create new agent work"
    );

    for (id, expected) in [
        (reply, StatusCode::BAD_REQUEST),
        (beta_root, StatusCode::NOT_FOUND),
        ("inbox/../cryo.toml", StatusCode::NOT_FOUND),
        ("outbox/../../beta/cryo.toml", StatusCode::NOT_FOUND),
    ] {
        let (status, _) = hub
            .send(&hub.alpha, json!({"body": "must fail", "thread_id": id}))
            .await;
        assert_eq!(status, expected, "unexpected reply result for {id}");
    }
    for id in ["inbox/../cryo.toml", "outbox/../../beta/cryo.toml"] {
        assert!(hub.store(&hub.alpha).get(id).is_err());
    }
    assert_eq!(hub.store(&hub.alpha).read_inbox_named().unwrap().len(), 2);
}
