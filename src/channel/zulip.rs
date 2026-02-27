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
    /// Returns (messages, found_newest).
    pub fn get_messages(
        &self,
        stream_id: u64,
        anchor: &str,
        num_after: u32,
        skip_email: Option<&str>,
    ) -> Result<(Vec<Message>, bool)> {
        let narrow = format!(r#"[{{"operator":"stream","operand":{}}}]"#, stream_id);
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
        let timestamp = chrono::DateTime::from_timestamp(ts_unix, 0)
            .map(|dt| dt.naive_utc())
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

/// Simple base64 encoding (no external dependency needed).
fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = Vec::with_capacity(data.len().div_ceil(3) * 4);
    let mut i = 0;

    while i + 2 < data.len() {
        let b0 = data[i] as usize;
        let b1 = data[i + 1] as usize;
        let b2 = data[i + 2] as usize;
        result.push(CHARS[b0 >> 2]);
        result.push(CHARS[((b0 & 3) << 4) | (b1 >> 4)]);
        result.push(CHARS[((b1 & 0xf) << 2) | (b2 >> 6)]);
        result.push(CHARS[b2 & 0x3f]);
        i += 3;
    }

    let remaining = data.len() - i;
    if remaining == 1 {
        let b0 = data[i] as usize;
        result.push(CHARS[b0 >> 2]);
        result.push(CHARS[(b0 & 3) << 4]);
        result.push(b'=');
        result.push(b'=');
    } else if remaining == 2 {
        let b0 = data[i] as usize;
        let b1 = data[i + 1] as usize;
        result.push(CHARS[b0 >> 2]);
        result.push(CHARS[((b0 & 3) << 4) | (b1 >> 4)]);
        result.push(CHARS[(b1 & 0xf) << 2]);
        result.push(b'=');
    }

    String::from_utf8(result).unwrap()
}
