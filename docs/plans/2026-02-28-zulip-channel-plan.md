# Zulip Message Channel Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a Zulip bot backend that syncs messages between a Zulip stream and the local inbox/outbox, via a separate `cryo-zulip` binary mirroring `cryo-gh`.

**Architecture:** A `ZulipClient` struct wraps `ureq` HTTP calls to the Zulip REST API. A `ZulipSyncState` persists sync state in `zulip-sync.json`. The `cryo-zulip` binary provides init/pull/push/sync/unsync/status subcommands and a sync daemon loop that polls for new messages and pushes outbox files.

**Tech Stack:** Rust, ureq (blocking HTTP), serde/serde_json, clap, chrono, notify, signal-hook

**Design doc:** `docs/plans/2026-02-28-zulip-channel-design.md`

---

### Task 1: Add ureq dependency and cryo-zulip binary entry

**Files:**
- Modify: `Cargo.toml:18-41`

**Step 1: Add ureq to dependencies and cryo-zulip binary entry**

In `Cargo.toml`, add after the `cryo-gh` binary entry (line 16):

```toml
[[bin]]
name = "cryo-zulip"
path = "src/bin/cryo_zulip.rs"
```

Add to `[dependencies]` (after line 36):

```toml
ureq = "3"
```

**Step 2: Verify it compiles**

Create a minimal `src/bin/cryo_zulip.rs`:

```rust
fn main() {
    println!("cryo-zulip placeholder");
}
```

Run: `cargo build --bin cryo-zulip`
Expected: BUILD SUCCESS

**Step 3: Commit**

```bash
git add Cargo.toml src/bin/cryo_zulip.rs
git commit -m "chore: add ureq dependency and cryo-zulip binary entry"
```

---

### Task 2: Implement ZulipSyncState persistence

**Files:**
- Create: `src/zulip_sync.rs`
- Modify: `src/lib.rs:6` (add `pub mod zulip_sync` after `pub mod gh_sync`)
- Create: `tests/zulip_sync_tests.rs`

**Step 1: Write the failing tests**

Create `tests/zulip_sync_tests.rs`:

```rust
use cryochamber::zulip_sync::{load_sync_state, save_sync_state, ZulipSyncState};

#[test]
fn test_zulip_sync_state_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("zulip-sync.json");

    let state = ZulipSyncState {
        site: "https://zulip.example.com".to_string(),
        stream: "cryochamber".to_string(),
        stream_id: 42,
        self_email: "bot@example.com".to_string(),
        topic: Some("my-project".to_string()),
        last_message_id: Some(12345),
        last_pushed_session: Some(3),
    };
    save_sync_state(&path, &state).unwrap();
    let loaded = load_sync_state(&path).unwrap().unwrap();

    assert_eq!(loaded.site, "https://zulip.example.com");
    assert_eq!(loaded.stream, "cryochamber");
    assert_eq!(loaded.stream_id, 42);
    assert_eq!(loaded.self_email, "bot@example.com");
    assert_eq!(loaded.topic, Some("my-project".to_string()));
    assert_eq!(loaded.last_message_id, Some(12345));
    assert_eq!(loaded.last_pushed_session, Some(3));
}

#[test]
fn test_zulip_sync_state_missing_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("zulip-sync.json");
    let loaded = load_sync_state(&path).unwrap();
    assert!(loaded.is_none());
}

#[test]
fn test_zulip_sync_state_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("zulip-sync.json");

    let state = ZulipSyncState {
        site: "https://z.example.com".to_string(),
        stream: "test".to_string(),
        stream_id: 1,
        self_email: "bot@z.example.com".to_string(),
        topic: None,
        last_message_id: None,
        last_pushed_session: None,
    };
    save_sync_state(&path, &state).unwrap();
    let loaded = load_sync_state(&path).unwrap().unwrap();
    assert!(loaded.topic.is_none());
    assert!(loaded.last_message_id.is_none());
    assert!(loaded.last_pushed_session.is_none());
}

#[test]
fn test_zulip_sync_state_legacy_json_compat() {
    // Simulate a zulip-sync.json without optional fields
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("zulip-sync.json");
    std::fs::write(
        &path,
        r#"{"site":"https://z.example.com","stream":"test","stream_id":1,"self_email":"bot@z.example.com"}"#,
    )
    .unwrap();
    let loaded = load_sync_state(&path).unwrap().unwrap();
    assert!(loaded.topic.is_none());
    assert!(loaded.last_message_id.is_none());
    assert!(loaded.last_pushed_session.is_none());
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test zulip_sync`
Expected: FAIL — module `zulip_sync` does not exist

**Step 3: Implement ZulipSyncState**

Create `src/zulip_sync.rs` (mirror `src/gh_sync.rs` pattern):

```rust
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Persistent state for the Zulip sync utility.
/// Stored in `zulip-sync.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZulipSyncState {
    /// Zulip server URL (e.g. "https://zulip.example.com")
    pub site: String,
    /// Zulip stream name
    pub stream: String,
    /// Zulip stream numeric ID
    pub stream_id: u64,
    /// Bot's email address (used to filter own messages on pull)
    pub self_email: String,
    /// Topic name for outgoing messages (default: "cryochamber")
    #[serde(default)]
    pub topic: Option<String>,
    /// ID of the last fetched message (anchor for polling)
    #[serde(default)]
    pub last_message_id: Option<u64>,
    /// Last session number that was pushed (to prevent duplicate posts)
    #[serde(default)]
    pub last_pushed_session: Option<u32>,
}

impl ZulipSyncState {
    /// Get the topic name, defaulting to "cryochamber".
    pub fn topic_name(&self) -> &str {
        self.topic.as_deref().unwrap_or("cryochamber")
    }
}

pub fn save_sync_state(path: &Path, state: &ZulipSyncState) -> Result<()> {
    let json = serde_json::to_string_pretty(state)?;
    std::fs::write(path, json)?;
    Ok(())
}

pub fn load_sync_state(path: &Path) -> Result<Option<ZulipSyncState>> {
    if !path.exists() {
        return Ok(None);
    }
    let contents = std::fs::read_to_string(path)?;
    let state: ZulipSyncState = serde_json::from_str(&contents)?;
    Ok(Some(state))
}
```

Add to `src/lib.rs` after `pub mod gh_sync;` (line 6):

```rust
pub mod zulip_sync;
```

**Step 4: Run tests to verify they pass**

Run: `cargo test zulip_sync`
Expected: all 4 tests PASS

**Step 5: Commit**

```bash
git add src/zulip_sync.rs src/lib.rs tests/zulip_sync_tests.rs
git commit -m "feat: add ZulipSyncState persistence module"
```

---

### Task 3: Implement ZulipClient — zuliprc parsing and request helpers

**Files:**
- Create: `src/channel/zulip.rs`
- Modify: `src/channel/mod.rs:2` (add `pub mod zulip` after `pub mod github`)
- Create: `tests/zulip_channel_tests.rs`

**Step 1: Write the failing tests for zuliprc parsing**

Create `tests/zulip_channel_tests.rs`:

```rust
use cryochamber::channel::zulip::ZulipClient;

#[test]
fn test_parse_zuliprc() {
    let dir = tempfile::tempdir().unwrap();
    let rc_path = dir.path().join("zuliprc");
    std::fs::write(
        &rc_path,
        "[api]\nemail=bot@example.com\nkey=abc123secret\nsite=https://zulip.example.com\n",
    )
    .unwrap();

    let client = ZulipClient::from_zuliprc(&rc_path).unwrap();
    let creds = client.credentials();
    assert_eq!(creds.email, "bot@example.com");
    assert_eq!(creds.api_key, "abc123secret");
    assert_eq!(creds.site, "https://zulip.example.com");
}

#[test]
fn test_parse_zuliprc_with_spaces() {
    let dir = tempfile::tempdir().unwrap();
    let rc_path = dir.path().join("zuliprc");
    std::fs::write(
        &rc_path,
        "[api]\nemail = bot@example.com\nkey = abc123\nsite = https://zulip.example.com\n",
    )
    .unwrap();

    let client = ZulipClient::from_zuliprc(&rc_path).unwrap();
    let creds = client.credentials();
    assert_eq!(creds.email, "bot@example.com");
    assert_eq!(creds.api_key, "abc123");
}

#[test]
fn test_parse_zuliprc_missing_field() {
    let dir = tempfile::tempdir().unwrap();
    let rc_path = dir.path().join("zuliprc");
    std::fs::write(&rc_path, "[api]\nemail=bot@example.com\n").unwrap();

    let result = ZulipClient::from_zuliprc(&rc_path);
    assert!(result.is_err());
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test zulip_channel`
Expected: FAIL — module `zulip` does not exist

**Step 3: Implement ZulipClient basics**

Create `src/channel/zulip.rs`:

```rust
use anyhow::{Context, Result};
use std::path::Path;

pub struct ZulipCredentials {
    pub email: String,
    pub api_key: String,
    pub site: String,
}

pub struct ZulipClient {
    creds: ZulipCredentials,
    agent: ureq::Agent,
}

impl ZulipClient {
    /// Parse a zuliprc INI file and create a client.
    pub fn from_zuliprc(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read zuliprc at {}", path.display()))?;

        let mut email = None;
        let mut api_key = None;
        let mut site = None;
        let mut in_api_section = false;

        for line in content.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                in_api_section = line == "[api]";
                continue;
            }
            if !in_api_section {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim();
                let value = value.trim();
                match key {
                    "email" => email = Some(value.to_string()),
                    "key" => api_key = Some(value.to_string()),
                    "site" => site = Some(value.to_string()),
                    _ => {}
                }
            }
        }

        let creds = ZulipCredentials {
            email: email.context("zuliprc missing 'email' in [api] section")?,
            api_key: api_key.context("zuliprc missing 'key' in [api] section")?,
            site: site.context("zuliprc missing 'site' in [api] section")?,
        };

        Ok(Self {
            creds,
            agent: ureq::Agent::new(),
        })
    }

    /// Access credentials (for testing).
    pub fn credentials(&self) -> &ZulipCredentials {
        &self.creds
    }

    /// Build a full API URL.
    fn api_url(&self, endpoint: &str) -> String {
        format!("{}/api/v1{}", self.creds.site.trim_end_matches('/'), endpoint)
    }

    /// Make an authenticated GET request, return parsed JSON.
    fn get(&self, endpoint: &str, params: &[(&str, &str)]) -> Result<serde_json::Value> {
        let url = self.api_url(endpoint);
        let mut req = self.agent.get(&url)
            .set("Authorization", &self.basic_auth());
        for (key, value) in params {
            req = req.query(key, value);
        }
        let body: serde_json::Value = req
            .call()
            .with_context(|| format!("GET {endpoint} failed"))?
            .body_mut()
            .read_json()
            .context("Failed to parse response JSON")?;
        self.check_result(&body, endpoint)?;
        Ok(body)
    }

    /// Make an authenticated POST request with form data, return parsed JSON.
    fn post(&self, endpoint: &str, params: &[(&str, &str)]) -> Result<serde_json::Value> {
        let url = self.api_url(endpoint);
        let form: Vec<(&str, &str)> = params.to_vec();
        let body: serde_json::Value = self.agent.post(&url)
            .set("Authorization", &self.basic_auth())
            .send_form(&form)
            .with_context(|| format!("POST {endpoint} failed"))?
            .body_mut()
            .read_json()
            .context("Failed to parse response JSON")?;
        self.check_result(&body, endpoint)?;
        Ok(body)
    }

    fn basic_auth(&self) -> String {
        use std::io::Write;
        let mut buf = b"Basic ".to_vec();
        {
            let mut encoder = base64_encoder(&mut buf);
            write!(encoder, "{}:{}", self.creds.email, self.creds.api_key).unwrap();
        }
        String::from_utf8(buf).unwrap()
    }

    fn check_result(&self, json: &serde_json::Value, endpoint: &str) -> Result<()> {
        if json["result"].as_str() != Some("success") {
            let msg = json["msg"].as_str().unwrap_or("unknown error");
            anyhow::bail!("Zulip API error on {endpoint}: {msg}");
        }
        Ok(())
    }
}

/// Simple base64 encoding for Basic auth.
fn base64_encoder(buf: &mut Vec<u8>) -> Base64Writer<'_> {
    Base64Writer { buf }
}

struct Base64Writer<'a> {
    buf: &'a mut Vec<u8>,
}

impl<'a> std::io::Write for Base64Writer<'a> {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        // Use a simple base64 implementation
        const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut i = 0;
        while i + 2 < data.len() {
            let b0 = data[i] as usize;
            let b1 = data[i + 1] as usize;
            let b2 = data[i + 2] as usize;
            self.buf.push(CHARS[b0 >> 2]);
            self.buf.push(CHARS[((b0 & 3) << 4) | (b1 >> 4)]);
            self.buf.push(CHARS[((b1 & 0xf) << 2) | (b2 >> 6)]);
            self.buf.push(CHARS[b2 & 0x3f]);
            i += 3;
        }
        let remaining = data.len() - i;
        if remaining == 1 {
            let b0 = data[i] as usize;
            self.buf.push(CHARS[b0 >> 2]);
            self.buf.push(CHARS[(b0 & 3) << 4]);
            self.buf.push(b'=');
            self.buf.push(b'=');
        } else if remaining == 2 {
            let b0 = data[i] as usize;
            let b1 = data[i + 1] as usize;
            self.buf.push(CHARS[b0 >> 2]);
            self.buf.push(CHARS[((b0 & 3) << 4) | (b1 >> 4)]);
            self.buf.push(CHARS[(b1 & 0xf) << 2]);
            self.buf.push(b'=');
        }
        Ok(data.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
```

Add to `src/channel/mod.rs` after `pub mod github;` (line 2):

```rust
pub mod zulip;
```

**Step 4: Run tests to verify they pass**

Run: `cargo test zulip_channel`
Expected: all 3 tests PASS

**Step 5: Run full check**

Run: `cargo clippy --all-targets -- -D warnings`
Expected: no warnings

**Step 6: Commit**

```bash
git add src/channel/zulip.rs src/channel/mod.rs tests/zulip_channel_tests.rs
git commit -m "feat: add ZulipClient with zuliprc parsing"
```

---

### Task 4: Implement ZulipClient API methods — get_profile, get_stream_id, get_messages, send_message

**Files:**
- Modify: `src/channel/zulip.rs`
- Modify: `tests/zulip_channel_tests.rs`

**Step 1: Write the failing tests for response parsing**

Since we can't make live API calls in tests, test the response parsing logic. Add to `tests/zulip_channel_tests.rs`:

```rust
use cryochamber::channel::zulip::{parse_get_messages_response, parse_get_profile_response, parse_get_stream_id_response};

#[test]
fn test_parse_get_profile_response() {
    let json = serde_json::json!({
        "result": "success",
        "msg": "",
        "user_id": 42,
        "email": "bot@example.com",
        "full_name": "Test Bot"
    });
    let (user_id, email) = parse_get_profile_response(&json).unwrap();
    assert_eq!(user_id, 42);
    assert_eq!(email, "bot@example.com");
}

#[test]
fn test_parse_get_stream_id_response() {
    let json = serde_json::json!({
        "result": "success",
        "msg": "",
        "stream_id": 15
    });
    let stream_id = parse_get_stream_id_response(&json).unwrap();
    assert_eq!(stream_id, 15);
}

#[test]
fn test_parse_get_messages_response() {
    let json = serde_json::json!({
        "result": "success",
        "msg": "",
        "messages": [
            {
                "id": 100,
                "sender_id": 42,
                "sender_email": "alice@example.com",
                "sender_full_name": "Alice",
                "content": "Hello from Zulip",
                "subject": "general-topic",
                "timestamp": 1740700000,
                "type": "stream"
            },
            {
                "id": 101,
                "sender_id": 43,
                "sender_email": "bot@example.com",
                "sender_full_name": "Bot",
                "content": "I am the bot",
                "subject": "general-topic",
                "timestamp": 1740700060,
                "type": "stream"
            }
        ],
        "found_newest": true,
        "found_oldest": false
    });
    let (messages, found_newest) = parse_get_messages_response(&json, Some("bot@example.com")).unwrap();
    // Should filter out bot's own message
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].from, "Alice");
    assert_eq!(messages[0].body, "Hello from Zulip");
    assert_eq!(messages[0].metadata.get("source"), Some(&"zulip".to_string()));
    assert_eq!(messages[0].metadata.get("zulip_message_id"), Some(&"100".to_string()));
    assert!(found_newest);
}

#[test]
fn test_parse_get_messages_response_empty() {
    let json = serde_json::json!({
        "result": "success",
        "msg": "",
        "messages": [],
        "found_newest": true,
        "found_oldest": true
    });
    let (messages, found_newest) = parse_get_messages_response(&json, None).unwrap();
    assert!(messages.is_empty());
    assert!(found_newest);
}

#[test]
fn test_parse_get_messages_no_self_filter() {
    let json = serde_json::json!({
        "result": "success",
        "msg": "",
        "messages": [
            {
                "id": 100,
                "sender_id": 42,
                "sender_email": "alice@example.com",
                "sender_full_name": "Alice",
                "content": "Hello",
                "subject": "topic",
                "timestamp": 1740700000,
                "type": "stream"
            }
        ],
        "found_newest": false,
        "found_oldest": false
    });
    let (messages, found_newest) = parse_get_messages_response(&json, None).unwrap();
    assert_eq!(messages.len(), 1);
    assert!(!found_newest);
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test zulip_channel`
Expected: FAIL — functions not found

**Step 3: Implement API methods and response parsers**

Add to `src/channel/zulip.rs` (public response parsing functions and client methods):

```rust
use crate::message::Message;
use chrono::NaiveDateTime;
use std::collections::BTreeMap;

// --- Response Parsers (public for testing) ---

pub fn parse_get_profile_response(json: &serde_json::Value) -> Result<(u64, String)> {
    let user_id = json["user_id"].as_u64().context("Missing user_id")?;
    let email = json["email"]
        .as_str()
        .context("Missing email")?
        .to_string();
    Ok((user_id, email))
}

pub fn parse_get_stream_id_response(json: &serde_json::Value) -> Result<u64> {
    json["stream_id"].as_u64().context("Missing stream_id")
}

/// Parse GET /messages response. Filters out messages from `skip_email` if provided.
/// Returns (messages, found_newest).
pub fn parse_get_messages_response(
    json: &serde_json::Value,
    skip_email: Option<&str>,
) -> Result<(Vec<Message>, bool)> {
    let found_newest = json["found_newest"].as_bool().unwrap_or(false);
    let msgs = json["messages"]
        .as_array()
        .context("Missing messages array")?;

    let mut messages = Vec::new();
    for msg in msgs {
        let sender_email = msg["sender_email"].as_str().unwrap_or("");
        if let Some(skip) = skip_email {
            if sender_email == skip {
                continue;
            }
        }

        let msg_id = msg["id"].as_u64().unwrap_or(0);
        let sender_name = msg["sender_full_name"]
            .as_str()
            .unwrap_or("unknown")
            .to_string();
        let content = msg["content"].as_str().unwrap_or("").to_string();
        let subject = msg["subject"].as_str().unwrap_or("").to_string();
        let ts_unix = msg["timestamp"].as_i64().unwrap_or(0);
        let timestamp = NaiveDateTime::from_timestamp_opt(ts_unix, 0)
            .unwrap_or_else(|| chrono::Local::now().naive_local());

        let mut metadata = BTreeMap::from([("source".to_string(), "zulip".to_string())]);
        if msg_id > 0 {
            metadata.insert("zulip_message_id".to_string(), msg_id.to_string());
        }

        messages.push(Message {
            from: sender_name,
            subject,
            body: content,
            timestamp,
            metadata,
        });
    }

    Ok((messages, found_newest))
}
```

Add API methods to `ZulipClient` impl block:

```rust
impl ZulipClient {
    // ... existing methods ...

    /// GET /api/v1/users/me — returns (user_id, email).
    pub fn get_profile(&self) -> Result<(u64, String)> {
        let json = self.get("/users/me", &[])?;
        parse_get_profile_response(&json)
    }

    /// GET /api/v1/get_stream_id — returns stream_id.
    pub fn get_stream_id(&self, stream_name: &str) -> Result<u64> {
        let json = self.get("/get_stream_id", &[("stream", stream_name)])?;
        parse_get_stream_id_response(&json)
    }

    /// GET /api/v1/messages — fetch messages from a stream since anchor.
    /// Returns (messages, found_newest).
    pub fn get_messages(
        &self,
        stream_id: u64,
        anchor: &str,
        num_after: u32,
        skip_email: Option<&str>,
    ) -> Result<(Vec<Message>, bool)> {
        let narrow = format!(
            r#"[{{"operator":"stream","operand":{}}}]"#,
            stream_id
        );
        let num_after_str = num_after.to_string();
        let json = self.get(
            "/messages",
            &[
                ("narrow", &narrow),
                ("anchor", anchor),
                ("num_before", "0"),
                ("num_after", &num_after_str),
                ("apply_markdown", "false"),
            ],
        )?;
        parse_get_messages_response(&json, skip_email)
    }

    /// POST /api/v1/messages — send a message to a stream+topic.
    pub fn send_message(&self, stream_id: u64, topic: &str, content: &str) -> Result<u64> {
        let stream_id_str = stream_id.to_string();
        let json = self.post(
            "/messages",
            &[
                ("type", "stream"),
                ("to", &stream_id_str),
                ("topic", topic),
                ("content", content),
            ],
        )?;
        let msg_id = json["id"].as_u64().unwrap_or(0);
        Ok(msg_id)
    }

    /// Pull all messages since last_message_id, writing each to inbox.
    /// Returns the new last_message_id.
    pub fn pull_messages(
        &self,
        stream_id: u64,
        last_message_id: Option<u64>,
        skip_email: Option<&str>,
        work_dir: &Path,
    ) -> Result<Option<u64>> {
        crate::message::ensure_dirs(work_dir)?;
        let mut anchor = match last_message_id {
            Some(id) => id.to_string(),
            None => "oldest".to_string(),
        };
        // When resuming from a known ID, we use include_anchor=false behavior
        // by fetching num_after messages AFTER the anchor (anchor itself excluded
        // because num_before=0 and the anchor message was already processed).
        let mut newest_id = last_message_id;

        loop {
            let (messages, found_newest) =
                self.get_messages(stream_id, &anchor, 1000, skip_email)?;

            for msg in &messages {
                if let Some(id_str) = msg.metadata.get("zulip_message_id") {
                    if let Ok(id) = id_str.parse::<u64>() {
                        // Skip the anchor message itself when resuming
                        if Some(id) == last_message_id {
                            continue;
                        }
                        if newest_id.is_none() || id > newest_id.unwrap() {
                            newest_id = Some(id);
                        }
                    }
                }
                crate::message::write_message(work_dir, "inbox", msg)?;
            }

            if found_newest || messages.is_empty() {
                break;
            }

            // Paginate: next anchor is the last message ID
            if let Some(last) = messages.last() {
                if let Some(id_str) = last.metadata.get("zulip_message_id") {
                    anchor = id_str.clone();
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        Ok(newest_id)
    }
}
```

**Note:** `NaiveDateTime::from_timestamp_opt` is available in chrono 0.4. If it's deprecated, use `DateTime::from_timestamp(ts, 0).map(|dt| dt.naive_utc())` instead. Check what compiles.

**Step 4: Run tests to verify they pass**

Run: `cargo test zulip_channel`
Expected: all 8 tests PASS

**Step 5: Run clippy**

Run: `cargo clippy --all-targets -- -D warnings`
Expected: no warnings. Fix any issues.

**Step 6: Commit**

```bash
git add src/channel/zulip.rs tests/zulip_channel_tests.rs
git commit -m "feat: add ZulipClient API methods and response parsers"
```

---

### Task 5: Implement cryo-zulip CLI — init, pull, push, status subcommands

**Files:**
- Modify: `src/bin/cryo_zulip.rs` (replace placeholder)

**Step 1: Implement the CLI**

Replace `src/bin/cryo_zulip.rs` with the full CLI. Follow `src/bin/cryo_gh.rs` structure exactly:

```rust
// src/bin/cryo_zulip.rs
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use cryochamber::channel::zulip::ZulipClient;

#[derive(Parser)]
#[command(name = "cryo-zulip", about = "Cryochamber Zulip sync")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize: validate credentials, resolve stream, write zulip-sync.json
    Init {
        /// Path to zuliprc file
        #[arg(long)]
        config: String,
        /// Zulip stream name
        #[arg(long)]
        stream: String,
        /// Topic name for outgoing messages (default: "cryochamber")
        #[arg(long)]
        topic: Option<String>,
    },
    /// Pull new messages from Zulip stream into messages/inbox/
    Pull,
    /// Push session summary to Zulip stream
    Push,
    /// Start background sync daemon
    Sync {
        /// Polling interval in seconds
        #[arg(long, default_value = "30")]
        interval: u64,
    },
    /// Stop the running sync daemon
    Unsync,
    /// Show sync status
    Status,
    /// Run the sync loop (internal — use `cryo-zulip sync` instead)
    #[command(hide = true)]
    SyncDaemon {
        #[arg(long, default_value = "30")]
        interval: u64,
    },
}

fn zulip_sync_path(dir: &Path) -> PathBuf {
    dir.join("zulip-sync.json")
}

fn load_client(dir: &Path) -> Result<(ZulipClient, cryochamber::zulip_sync::ZulipSyncState)> {
    let sync_state = cryochamber::zulip_sync::load_sync_state(&zulip_sync_path(dir))?
        .context("zulip-sync.json not found. Run 'cryo-zulip init' first.")?;
    // We need to find the zuliprc path. Store it? Or re-derive from site+creds?
    // For pull/push, we reconstruct the client from sync state + a stored config path.
    // Simplification: store the zuliprc path in ZulipSyncState.
    // Alternative: require --config on every command.
    // Decision: store config_path in ZulipSyncState.
    anyhow::bail!("TODO: implement client loading from sync state")
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init { config, stream, topic } => cmd_init(&config, &stream, topic.as_deref()),
        Commands::Pull => cmd_pull(),
        Commands::Push => cmd_push(),
        Commands::Sync { interval } => cmd_sync(interval),
        Commands::Unsync => cmd_unsync(),
        Commands::Status => cmd_status(),
        Commands::SyncDaemon { interval } => cmd_sync_daemon(interval),
    }
}

fn cmd_init(config_path: &str, stream_name: &str, topic: Option<&str>) -> Result<()> {
    let dir = cryochamber::work_dir()?;

    let client = ZulipClient::from_zuliprc(Path::new(config_path))?;

    println!("Validating credentials...");
    let (_user_id, self_email) = client.get_profile()?;
    println!("Authenticated as {self_email}");

    println!("Resolving stream '{stream_name}'...");
    let stream_id = client.get_stream_id(stream_name)?;
    println!("Stream ID: {stream_id}");

    let sync_state = cryochamber::zulip_sync::ZulipSyncState {
        site: client.credentials().site.clone(),
        stream: stream_name.to_string(),
        stream_id,
        self_email,
        topic: topic.map(|t| t.to_string()),
        last_message_id: None,
        last_pushed_session: None,
    };
    cryochamber::zulip_sync::save_sync_state(&zulip_sync_path(&dir), &sync_state)?;

    // Copy zuliprc to .cryo/ for later use by pull/push/sync
    let cryo_dir = dir.join(".cryo");
    std::fs::create_dir_all(&cryo_dir)?;
    std::fs::copy(config_path, cryo_dir.join("zuliprc"))?;

    println!("Saved zulip-sync.json");
    println!("Copied zuliprc to .cryo/zuliprc");
    Ok(())
}

fn load_client_from_project(dir: &Path) -> Result<(ZulipClient, cryochamber::zulip_sync::ZulipSyncState)> {
    let sync_state = cryochamber::zulip_sync::load_sync_state(&zulip_sync_path(dir))?
        .context("zulip-sync.json not found. Run 'cryo-zulip init' first.")?;
    let rc_path = dir.join(".cryo").join("zuliprc");
    let client = ZulipClient::from_zuliprc(&rc_path)
        .context("Failed to load .cryo/zuliprc. Re-run 'cryo-zulip init'.")?;
    Ok((client, sync_state))
}

fn cmd_pull() -> Result<()> {
    let dir = cryochamber::work_dir()?;
    let (client, mut sync_state) = load_client_from_project(&dir)?;

    println!("Pulling messages from stream '{}'...", sync_state.stream);
    let new_last_id = client.pull_messages(
        sync_state.stream_id,
        sync_state.last_message_id,
        Some(&sync_state.self_email),
        &dir,
    )?;

    if let Some(id) = new_last_id {
        if sync_state.last_message_id != Some(id) {
            sync_state.last_message_id = Some(id);
            cryochamber::zulip_sync::save_sync_state(&zulip_sync_path(&dir), &sync_state)?;
        }
    }

    let inbox = cryochamber::message::read_inbox(&dir)?;
    println!("Inbox: {} message(s)", inbox.len());
    Ok(())
}

fn cmd_push() -> Result<()> {
    let dir = cryochamber::work_dir()?;
    let (client, mut sync_state) = load_client_from_project(&dir)?;

    let log = cryochamber::log::log_path(&dir);
    let latest = cryochamber::log::read_latest_session(&log)?;

    let Some(session_output) = latest else {
        println!("No session log found. Nothing to push.");
        return Ok(());
    };

    let state_file = cryochamber::state::state_path(&dir);
    let session_num = cryochamber::state::load_state(&state_file)?
        .map(|s| s.session_number)
        .unwrap_or(0);

    if sync_state.last_pushed_session == Some(session_num) {
        println!("Session {session_num} already pushed. Skipping.");
        return Ok(());
    }

    let topic = sync_state.topic_name();
    let comment = format!("## Session {session_num}\n\n```\n{session_output}\n```");

    println!("Posting session summary to stream '{}'...", sync_state.stream);
    client.send_message(sync_state.stream_id, topic, &comment)?;

    sync_state.last_pushed_session = Some(session_num);
    cryochamber::zulip_sync::save_sync_state(&zulip_sync_path(&dir), &sync_state)?;

    println!("Push complete.");
    Ok(())
}

fn cmd_sync(interval: u64) -> Result<()> {
    let dir = cryochamber::work_dir()?;

    if !cryochamber::config::config_path(&dir).exists() {
        anyhow::bail!("No cryochamber project in this directory. Run `cryo init` first.");
    }

    let sync_path = zulip_sync_path(&dir);
    let sync_state = cryochamber::zulip_sync::load_sync_state(&sync_path)?
        .context("zulip-sync.json not found. Run 'cryo-zulip init' first.")?;

    cryochamber::message::ensure_dirs(&dir)?;

    let exe = std::env::current_exe().context("Failed to resolve cryo-zulip executable path")?;
    let interval_str = interval.to_string();
    let log_path = dir.join("cryo-zulip-sync.log");
    cryochamber::service::install(
        "zulip-sync",
        &dir,
        &exe,
        &["sync-daemon", "--interval", &interval_str],
        &log_path,
        true,
    )?;

    println!(
        "Sync service installed for stream '{}' on {}",
        sync_state.stream, sync_state.site
    );
    println!("Log: cryo-zulip-sync.log");
    println!("Survives reboot. Stop with: cryo-zulip unsync");
    Ok(())
}

fn cmd_unsync() -> Result<()> {
    let dir = cryochamber::work_dir()?;

    if cryochamber::service::uninstall("zulip-sync", &dir)? {
        println!("Sync service stopped and removed.");
    } else {
        println!("No sync service installed for this directory.");
    }
    Ok(())
}

fn cmd_sync_daemon(interval: u64) -> Result<()> {
    let dir = cryochamber::work_dir()?;
    let sync_path = zulip_sync_path(&dir);

    eprintln!("Zulip sync daemon started (PID {})", std::process::id());

    let shutdown = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&shutdown))?;
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&shutdown))?;

    use notify::Watcher;
    let (tx, rx) = std::sync::mpsc::channel();
    let outbox_path = dir.join("messages").join("outbox");
    let _watcher = {
        let tx = tx.clone();
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res {
                if event.kind.is_create() {
                    let _ = tx.send(());
                }
            }
        })
        .context("Failed to create outbox watcher")?;
        watcher
            .watch(&outbox_path, notify::RecursiveMode::NonRecursive)
            .context("Failed to watch messages/outbox/")?;
        watcher
    };

    let shutdown_flag = Arc::clone(&shutdown);
    std::thread::spawn(move || {
        while !shutdown_flag.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        let _ = tx.send(());
    });

    let interval_dur = std::time::Duration::from_secs(interval);

    loop {
        if shutdown.load(Ordering::Relaxed) {
            eprintln!("Zulip sync: shutting down");
            break;
        }

        let (client, mut sync_state) = match load_client_from_project(&dir) {
            Ok(pair) => pair,
            Err(e) => {
                eprintln!("Zulip sync: config error: {e}");
                std::thread::sleep(interval_dur);
                continue;
            }
        };

        // Pull: Zulip → inbox
        match client.pull_messages(
            sync_state.stream_id,
            sync_state.last_message_id,
            Some(&sync_state.self_email),
            &dir,
        ) {
            Ok(new_last_id) => {
                if let Some(id) = new_last_id {
                    if sync_state.last_message_id != Some(id) {
                        sync_state.last_message_id = Some(id);
                        if let Err(e) = cryochamber::zulip_sync::save_sync_state(&sync_path, &sync_state) {
                            eprintln!("Zulip sync: failed to save state: {e}");
                        }
                    }
                }
            }
            Err(e) => eprintln!("Zulip sync: pull error: {e}"),
        }

        // Push: outbox → Zulip
        if let Err(e) = push_outbox(&dir, &client, &sync_state) {
            eprintln!("Zulip sync: push error: {e}");
        }

        match rx.recv_timeout(interval_dur) {
            Ok(()) => {
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    eprintln!("Zulip sync: stopped");
    Ok(())
}

fn push_outbox(
    dir: &Path,
    client: &ZulipClient,
    sync_state: &cryochamber::zulip_sync::ZulipSyncState,
) -> Result<()> {
    let messages = cryochamber::message::read_outbox(dir)?;
    if messages.is_empty() {
        return Ok(());
    }

    let outbox = dir.join("messages").join("outbox");
    let archive = outbox.join("archive");
    std::fs::create_dir_all(&archive)?;

    let topic = sync_state.topic_name();

    for (filename, msg) in &messages {
        let body = format!("**{}** ({})\n\n{}", msg.from, msg.subject, msg.body);
        match client.send_message(sync_state.stream_id, topic, &body) {
            Ok(_) => {
                eprintln!("Zulip sync: posted outbox/{filename}");
                let src = outbox.join(filename);
                let dst = archive.join(filename);
                if src.exists() {
                    std::fs::rename(&src, &dst)?;
                }
            }
            Err(e) => {
                eprintln!("Zulip sync: failed to post outbox/{filename}: {e}");
            }
        }
    }

    Ok(())
}

fn cmd_status() -> Result<()> {
    let dir = cryochamber::work_dir()?;
    match cryochamber::zulip_sync::load_sync_state(&zulip_sync_path(&dir))? {
        None => println!("Zulip sync not configured. Run 'cryo-zulip init' first."),
        Some(state) => {
            println!("Site: {}", state.site);
            println!("Stream: {} (ID: {})", state.stream, state.stream_id);
            println!("Topic: {}", state.topic_name());
            println!("Bot email: {}", state.self_email);
            println!(
                "Last message ID: {}",
                state
                    .last_message_id
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "(none — will read all)".to_string())
            );
            println!(
                "Last pushed session: {}",
                state
                    .last_pushed_session
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "(none)".to_string())
            );
        }
    }
    Ok(())
}
```

**Step 2: Remove the dead `load_client` function**

Remove the unused `load_client` function since we use `load_client_from_project` instead.

**Step 3: Verify it compiles**

Run: `cargo build --bin cryo-zulip`
Expected: BUILD SUCCESS

**Step 4: Run clippy**

Run: `cargo clippy --all-targets -- -D warnings`
Expected: no warnings

**Step 5: Commit**

```bash
git add src/bin/cryo_zulip.rs
git commit -m "feat: implement cryo-zulip CLI with init/pull/push/sync/unsync/status"
```

---

### Task 6: Wire zulip-sync cleanup into cryo clean

**Files:**
- Modify: `src/bin/cryo.rs` (around line 484-510, the `cmd_clean` function)

**Step 1: Add zulip-sync service uninstall and file cleanup**

In `cmd_clean()`, after the `gh-sync` uninstall line (line 488), add:

```rust
    if cryochamber::service::uninstall("zulip-sync", &dir)? {
        println!("Removed zulip-sync service.");
    }
```

In the `runtime_files` array (line 504), add `"cryo-zulip-sync.log"` and `"zulip-sync.json"`:

```rust
    let runtime_files = [
        "timer.json",
        "cryo.log",
        "cryo-agent.log",
        "cryo-gh-sync.log",
        "gh-sync.json",
        "cryo-zulip-sync.log",
        "zulip-sync.json",
    ];
```

**Step 2: Verify it compiles**

Run: `cargo build --bin cryo`
Expected: BUILD SUCCESS

**Step 3: Run all tests**

Run: `cargo test`
Expected: all tests PASS

**Step 4: Run full check**

Run: `make check`
Expected: fmt, clippy, tests all pass

**Step 5: Commit**

```bash
git add src/bin/cryo.rs
git commit -m "feat: add zulip-sync cleanup to cryo clean command"
```

---

### Task 7: Final verification and cleanup

**Files:**
- All modified files

**Step 1: Run the full check suite**

Run: `make check`
Expected: `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test` all pass

**Step 2: Verify binary builds**

Run: `cargo build --bin cryo-zulip`
Expected: BUILD SUCCESS

**Step 3: Test CLI help output**

Run: `cargo run --bin cryo-zulip -- --help`
Expected: Shows init, pull, push, sync, unsync, status subcommands

Run: `cargo run --bin cryo-zulip -- init --help`
Expected: Shows --config, --stream, --topic options

**Step 4: Review all new/modified files**

Verify:
- `src/channel/zulip.rs` — ZulipClient, parsers, API methods
- `src/channel/mod.rs` — `pub mod zulip` added
- `src/zulip_sync.rs` — ZulipSyncState
- `src/lib.rs` — `pub mod zulip_sync` added
- `src/bin/cryo_zulip.rs` — full CLI
- `src/bin/cryo.rs` — zulip-sync cleanup in cmd_clean
- `Cargo.toml` — ureq dep + binary entry
- `tests/zulip_sync_tests.rs` — sync state tests
- `tests/zulip_channel_tests.rs` — API parsing tests

**Step 5: Commit any remaining fixes**

```bash
git add -A
git commit -m "feat: complete Zulip message channel implementation (#6)"
```
