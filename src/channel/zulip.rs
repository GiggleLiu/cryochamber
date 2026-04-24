use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::path::Path;

use crate::message::Message;

/// Credentials parsed from a zuliprc INI file.
pub struct ZulipCredentials {
    pub email: String,
    pub api_key: String,
    pub site: String,
}

/// HTTP client for the Zulip REST API.
pub struct ZulipClient {
    creds: ZulipCredentials,
    agent: ureq::Agent,
}

#[derive(Debug, Clone)]
pub struct ZulipPullResult {
    pub messages: Vec<Message>,
    pub newest_seen_id: Option<u64>,
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
            agent: ureq::Agent::new_with_defaults(),
        })
    }

    /// Access credentials (for testing).
    pub fn credentials(&self) -> &ZulipCredentials {
        &self.creds
    }

    /// Build a full API URL.
    fn api_url(&self, endpoint: &str) -> String {
        format!(
            "{}/api/v1{}",
            self.creds.site.trim_end_matches('/'),
            endpoint
        )
    }

    /// Make an authenticated GET request, return parsed JSON.
    fn get(&self, endpoint: &str, params: &[(&str, &str)]) -> Result<serde_json::Value> {
        let url = self.api_url(endpoint);
        let mut req = self
            .agent
            .get(&url)
            .header("Authorization", &self.basic_auth());
        for &(key, value) in params {
            req = req.query(key, value);
        }
        let resp_str = req
            .call()
            .with_context(|| format!("GET {endpoint} failed"))?
            .body_mut()
            .read_to_string()
            .context("Failed to read response body")?;
        let body: serde_json::Value =
            serde_json::from_str(&resp_str).context("Failed to parse response JSON")?;
        self.check_result(&body, endpoint)?;
        Ok(body)
    }

    /// Make an authenticated POST request with form data, return parsed JSON.
    fn post(&self, endpoint: &str, params: &[(&str, &str)]) -> Result<serde_json::Value> {
        let url = self.api_url(endpoint);
        let form: Vec<(&str, &str)> = params.to_vec();
        let resp_str = self
            .agent
            .post(&url)
            .header("Authorization", &self.basic_auth())
            .send_form(form)
            .with_context(|| format!("POST {endpoint} failed"))?
            .body_mut()
            .read_to_string()
            .context("Failed to read response body")?;
        let body: serde_json::Value =
            serde_json::from_str(&resp_str).context("Failed to parse response JSON")?;
        self.check_result(&body, endpoint)?;
        Ok(body)
    }

    fn basic_auth(&self) -> String {
        let credentials = format!("{}:{}", self.creds.email, self.creds.api_key);
        format!("Basic {}", base64_encode(credentials.as_bytes()))
    }

    fn check_result(&self, json: &serde_json::Value, endpoint: &str) -> Result<()> {
        if json["result"].as_str() != Some("success") {
            let msg = json["msg"].as_str().unwrap_or("unknown error");
            anyhow::bail!("Zulip API error on {endpoint}: {msg}");
        }
        Ok(())
    }

    /// GET /api/v1/users/me -- returns (user_id, email).
    pub fn get_profile(&self) -> Result<(u64, String)> {
        let json = self.get("/users/me", &[])?;
        parse_get_profile_response(&json)
    }

    /// GET /api/v1/get_stream_id -- returns stream_id.
    pub fn get_stream_id(&self, stream_name: &str) -> Result<u64> {
        let json = self.get("/get_stream_id", &[("stream", stream_name)])?;
        parse_get_stream_id_response(&json)
    }

    /// GET /api/v1/messages -- fetch messages from a stream since anchor.
    /// Returns (messages, found_newest, raw_max_id).
    pub fn get_messages(
        &self,
        stream_id: u64,
        topic: Option<&str>,
        anchor: &str,
        num_after: u32,
        skip_email: Option<&str>,
    ) -> Result<(Vec<Message>, bool, Option<u64>)> {
        self.get_messages_window(stream_id, topic, anchor, 0, num_after, skip_email)
    }

    fn get_messages_window(
        &self,
        stream_id: u64,
        topic: Option<&str>,
        anchor: &str,
        num_before: u32,
        num_after: u32,
        skip_email: Option<&str>,
    ) -> Result<(Vec<Message>, bool, Option<u64>)> {
        let mut narrow = vec![serde_json::json!({
            "operator": "stream",
            "operand": stream_id
        })];
        if let Some(topic) = topic {
            narrow.push(serde_json::json!({
                "operator": "topic",
                "operand": topic
            }));
        }
        let narrow = serde_json::to_string(&narrow)?;
        let num_before_str = num_before.to_string();
        let num_after_str = num_after.to_string();
        let json = self.get(
            "/messages",
            &[
                ("narrow", &narrow),
                ("anchor", anchor),
                ("num_before", &num_before_str),
                ("num_after", &num_after_str),
                ("apply_markdown", "false"),
            ],
        )?;
        parse_get_messages_response(&json, skip_email, topic)
    }

    /// Return the newest existing message ID in a stream/topic, if one exists.
    pub fn newest_message_id(&self, stream_id: u64, topic: Option<&str>) -> Result<Option<u64>> {
        let (_, _, raw_max_id) =
            self.get_messages_window(stream_id, topic, "newest", 1, 0, None)?;
        Ok(raw_max_id)
    }

    /// POST /api/v1/messages -- send a message to a stream+topic.
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
        let msg_id = json["id"]
            .as_u64()
            .context("send_message: response JSON missing numeric 'id' field")?;
        Ok(msg_id)
    }

    /// Pull all messages since last_message_id.
    /// This performs remote transport and response filtering only; callers own
    /// local inbox persistence and sync-state cursor updates.
    pub fn fetch_messages_since(
        &self,
        stream_id: u64,
        topic: Option<&str>,
        last_message_id: Option<u64>,
        skip_email: Option<&str>,
    ) -> Result<ZulipPullResult> {
        let mut anchor = match last_message_id {
            Some(id) => id.to_string(),
            None => "oldest".to_string(),
        };
        let mut newest_seen_id = None;
        let mut pulled = Vec::new();

        loop {
            let (messages, found_newest, raw_max_id) =
                self.get_messages(stream_id, topic, &anchor, 1000, skip_email)?;

            newest_seen_id = max_optional_id(newest_seen_id, raw_max_id);

            for msg in messages {
                if message_zulip_id(&msg) == last_message_id {
                    // Skip the anchor message itself when resuming.
                    continue;
                }
                pulled.push(msg);
            }

            if found_newest {
                break;
            }

            // Advance cursor using raw max ID (before filtering),
            // so we don't get stuck when all messages are filtered out.
            if let Some(max_id) = raw_max_id {
                anchor = max_id.to_string();
            } else {
                // Empty raw page — no more messages
                break;
            }
        }

        Ok(ZulipPullResult {
            messages: pulled,
            newest_seen_id,
        })
    }
}

fn message_zulip_id(msg: &Message) -> Option<u64> {
    msg.metadata
        .get("zulip_message_id")
        .and_then(|id| id.parse::<u64>().ok())
}

fn max_optional_id(current: Option<u64>, next: Option<u64>) -> Option<u64> {
    match (current, next) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (Some(id), None) | (None, Some(id)) => Some(id),
        (None, None) => None,
    }
}

// --- Response Parsers (public for testing) ---

/// Parse GET /users/me response. Returns (user_id, email).
pub fn parse_get_profile_response(json: &serde_json::Value) -> Result<(u64, String)> {
    let user_id = json["user_id"].as_u64().context("Missing user_id")?;
    let email = json["email"].as_str().context("Missing email")?.to_string();
    Ok((user_id, email))
}

/// Parse GET /get_stream_id response. Returns stream_id.
pub fn parse_get_stream_id_response(json: &serde_json::Value) -> Result<u64> {
    json["stream_id"].as_u64().context("Missing stream_id")
}

/// Parse GET /messages response. Filters out messages from `skip_email` if provided.
/// Returns (filtered_messages, found_newest, raw_max_id).
/// `raw_max_id` is the highest message ID in the raw response (before filtering),
/// used for cursor advancement even when all messages are filtered out.
pub fn parse_get_messages_response(
    json: &serde_json::Value,
    skip_email: Option<&str>,
    topic: Option<&str>,
) -> Result<(Vec<Message>, bool, Option<u64>)> {
    let found_newest = json["found_newest"].as_bool().unwrap_or(false);
    let msgs = json["messages"]
        .as_array()
        .context("Missing messages array")?;

    let mut raw_max_id: Option<u64> = None;
    let mut messages = Vec::new();
    for msg in msgs {
        let msg_id = msg["id"].as_u64().unwrap_or(0);

        // Track highest raw ID before filtering
        if msg_id > 0 {
            raw_max_id = Some(raw_max_id.map_or(msg_id, |prev| prev.max(msg_id)));
        }

        let sender_email = msg["sender_email"].as_str().unwrap_or("");
        if let Some(skip) = skip_email {
            if sender_email == skip {
                continue;
            }
        }

        let sender_name = msg["sender_full_name"]
            .as_str()
            .unwrap_or("unknown")
            .to_string();
        let content = msg["content"].as_str().unwrap_or("").to_string();
        let subject = msg["subject"].as_str().unwrap_or("").to_string();
        if let Some(topic) = topic {
            if subject != topic {
                continue;
            }
        }
        // Zulip returns unix seconds (UTC). The rest of the codebase stores
        // message timestamps as naive *local* datetimes (see `Message::from`
        // callers that use `Local::now().naive_local()`), so convert to local
        // here — otherwise the thread renders Zulip messages off by the local
        // UTC offset.
        let ts_unix = msg["timestamp"].as_i64().unwrap_or(0);
        let timestamp = chrono::DateTime::from_timestamp(ts_unix, 0)
            .map(|dt| dt.with_timezone(&chrono::Local).naive_local())
            .unwrap_or_default();

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

    Ok((messages, found_newest, raw_max_id))
}

const BASE64_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Simple base64 encoding (no external dependency needed).
fn base64_encode(data: &[u8]) -> String {
    let mut result = Vec::with_capacity(data.len().div_ceil(3) * 4);
    let mut i = 0;

    while i + 2 < data.len() {
        let b0 = data[i] as usize;
        let b1 = data[i + 1] as usize;
        let b2 = data[i + 2] as usize;
        result.push(BASE64_CHARS[b0 >> 2]);
        result.push(BASE64_CHARS[((b0 & 3) << 4) | (b1 >> 4)]);
        result.push(BASE64_CHARS[((b1 & 0xf) << 2) | (b2 >> 6)]);
        result.push(BASE64_CHARS[b2 & 0x3f]);
        i += 3;
    }

    append_base64_tail(&mut result, &data[i..]);

    String::from_utf8(result).unwrap()
}

fn append_base64_tail(result: &mut Vec<u8>, tail: &[u8]) {
    match tail {
        [] => {}
        [b0] => {
            let b0 = *b0 as usize;
            result.push(BASE64_CHARS[b0 >> 2]);
            result.push(BASE64_CHARS[(b0 & 3) << 4]);
            result.push(b'=');
            result.push(b'=');
        }
        [b0, b1] => {
            let b0 = *b0 as usize;
            let b1 = *b1 as usize;
            result.push(BASE64_CHARS[b0 >> 2]);
            result.push(BASE64_CHARS[((b0 & 3) << 4) | (b1 >> 4)]);
            result.push(BASE64_CHARS[(b1 & 0xf) << 2]);
            result.push(b'=');
        }
        _ => unreachable!("base64 tail must contain at most two bytes"),
    }
}

#[cfg(test)]
#[path = "../unit_tests/channel/zulip.rs"]
mod tests;
