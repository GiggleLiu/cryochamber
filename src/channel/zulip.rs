use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::message::Message;

/// Chamber-relative directory where pulled Zulip attachments are stored.
/// Deliberately outside `messages/inbox/` so attachment writes never trigger
/// the daemon's inbox watcher.
pub const ATTACHMENTS_SUBDIR: &str = "messages/attachments";

/// Cap on a single attachment transfer, in both directions. Matches Zulip's
/// stock server upload limit; anything larger fails the transfer, leaving a
/// pulled link remote or an outbound link un-uploaded.
const MAX_ATTACHMENT_BYTES: u64 = 25 * 1024 * 1024;

/// Global timeout bounding an entire HTTP call (DNS -> connect -> read body).
/// Without it, a stalled connection blocks the single-threaded sync daemon
/// forever. ureq treats `timeout_global` as an end-to-end cap covering all
/// other timeouts.
const HTTP_GLOBAL_TIMEOUT: Duration = Duration::from_secs(30);
/// Tighter cap on just establishing the TCP+TLS connection, so a black-holed
/// host fails fast instead of eating the whole global budget.
const HTTP_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Build the Zulip HTTP agent with bounded timeouts (ureq 3.x). A daemon that
/// polls Zulip on a single thread must never block indefinitely on one call.
fn build_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(HTTP_GLOBAL_TIMEOUT))
        .timeout_connect(Some(HTTP_CONNECT_TIMEOUT))
        .build()
        .into()
}

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
            agent: build_agent(),
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

    /// GET a `/user_uploads/...` file with API authentication.
    /// Zulip may answer with a redirect to a signed URL (S3 backend); ureq
    /// follows it. The read is capped at `MAX_ATTACHMENT_BYTES`.
    pub fn download_upload(&self, server_path: &str) -> Result<Vec<u8>> {
        anyhow::ensure!(
            server_path.starts_with("/user_uploads/"),
            "not a user_uploads path: {server_path}"
        );
        // This request carries the bot's Basic auth, so a crafted link must
        // not be able to steer it outside /user_uploads/ (dot segments) or
        // smuggle query/fragment parts onto another endpoint.
        anyhow::ensure!(
            server_path.split('/').all(|seg| seg != ".." && seg != ".")
                && !server_path.contains('?')
                && !server_path.contains('#'),
            "unsafe user_uploads path: {server_path}"
        );
        let url = format!("{}{}", self.creds.site.trim_end_matches('/'), server_path);
        let bytes = self
            .agent
            .get(&url)
            .header("Authorization", &self.basic_auth())
            .call()
            .with_context(|| format!("GET {server_path} failed"))?
            .body_mut()
            .with_config()
            .limit(MAX_ATTACHMENT_BYTES)
            .read_to_vec()
            .with_context(|| format!("Failed to read attachment body for {server_path}"))?;
        Ok(bytes)
    }

    /// POST /api/v1/user_uploads -- upload a local file, returning its
    /// server path (`/user_uploads/...`). Zulip renders a link to such a path
    /// as an inline image preview on every supported server version.
    pub fn upload_file(&self, path: &Path) -> Result<String> {
        // Check the size before reading: an agent can link an arbitrarily
        // large file, and Zulip's stock limit would reject it anyway. Failing
        // here avoids buffering it in the sync daemon just to lose the round
        // trip. Mirrors the download cap.
        let size = std::fs::metadata(path)
            .with_context(|| format!("Failed to stat attachment {}", path.display()))?
            .len();
        anyhow::ensure!(
            size <= MAX_ATTACHMENT_BYTES,
            "attachment {} is {size} bytes, over the {MAX_ATTACHMENT_BYTES}-byte limit",
            path.display()
        );
        let bytes = std::fs::read(path)
            .with_context(|| format!("Failed to read attachment {}", path.display()))?;
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(sanitize_filename)
            .unwrap_or_else(|| "file".to_string());
        let boundary = multipart_boundary(&bytes);

        let mut body = Vec::new();
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(
            format!("Content-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\n")
                .as_bytes(),
        );
        body.extend_from_slice(b"Content-Type: application/octet-stream\r\n\r\n");
        body.extend_from_slice(&bytes);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

        let url = self.api_url("/user_uploads");
        let resp_str = self
            .agent
            .post(&url)
            .header("Authorization", &self.basic_auth())
            .header(
                "Content-Type",
                &format!("multipart/form-data; boundary={boundary}"),
            )
            .send(&body[..])
            .with_context(|| format!("POST /user_uploads failed for {}", path.display()))?
            .body_mut()
            .read_to_string()
            .context("Failed to read upload response body")?;
        let json: serde_json::Value =
            serde_json::from_str(&resp_str).context("Failed to parse upload response JSON")?;
        self.check_result(&json, "/user_uploads")?;
        parse_upload_response(&json)
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
            is_question: false,
        });
    }

    Ok((messages, found_newest, raw_max_id))
}

// --- Markdown link parsing ---

/// One inline markdown link destination located in a message body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownLink {
    /// Byte range of the destination alone — exactly the text to splice when
    /// rewriting, leaving link text and any title untouched.
    pub span: std::ops::Range<usize>,
    /// Byte offset of the `!` that makes this CommonMark image syntax, if
    /// present. Zulip renders `![alt](url)` literally before server version
    /// 12.0 (feature level 437), so the sync layer strips it.
    pub bang_at: Option<usize>,
}

/// Byte offset of the `[` matching the `]` at `close`, scanning backwards and
/// tracking bracket depth. `None` if the brackets are unbalanced.
fn matching_open_bracket(body: &str, close: usize) -> Option<usize> {
    let mut depth = 1u32;
    for (i, c) in body[..close].char_indices().rev() {
        match c {
            ']' => depth += 1,
            '[' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Locate inline markdown link destinations (`[text](dest)` and
/// `![text](dest)`), in order. Handles balanced parentheses inside the
/// destination and an optional CommonMark title (`[a](/url "title")`), both of
/// which stay outside the returned span.
pub fn markdown_links(body: &str) -> Vec<MarkdownLink> {
    let mut links = Vec::new();
    let mut pos = 0;
    while let Some(open) = body[pos..].find("](") {
        let bracket_close = pos + open;
        let content_start = bracket_close + 2;
        let Some(open_bracket) = matching_open_bracket(body, bracket_close) else {
            pos = content_start;
            continue;
        };
        // Find the matching close paren, allowing balanced pairs inside the
        // destination (CommonMark permits them unescaped).
        let mut depth = 1u32;
        let mut content_end = None;
        for (i, c) in body[content_start..].char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        content_end = Some(content_start + i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(content_end) = content_end else {
            break;
        };
        pos = content_end + 1;

        // The destination is the first whitespace-delimited token; anything
        // after it (a quoted title) stays outside the rewrite span.
        let content = &body[content_start..content_end];
        let dest_start = content_start + (content.len() - content.trim_start().len());
        let dest = &body[dest_start..content_end];
        let dest_end = dest_start + dest.find(char::is_whitespace).unwrap_or(dest.len());

        // Image syntax is `![alt](dest)`: find the `[` that *matches* this
        // link's `]` and check for a `!` immediately before it. Matching by
        // depth rather than the nearest `[` matters for nested links like
        // `[![alt](thumb.png)](full.png)`, where the nearest one belongs to
        // the inner image — attributing its `!` to the outer link would queue
        // two deletions of the same byte and mangle the message.
        let bang_at = body[..open_bracket]
            .ends_with('!')
            .then(|| open_bracket - 1);

        links.push(MarkdownLink {
            span: dest_start..dest_end,
            bang_at,
        });
    }
    links
}

// --- Attachment localization ---

/// A `/user_uploads/` markdown link destination found in a Zulip message body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadLink {
    /// Byte range of the destination inside the body — exactly the text to
    /// splice when rewriting, leaving link text and any title untouched.
    pub span: std::ops::Range<usize>,
    /// Server path beginning with `/user_uploads/`.
    pub server_path: String,
    /// Sanitized filename derived from the last path segment.
    pub filename: String,
}

/// Extract `/user_uploads/` markdown link destinations from a raw Zulip
/// message body. Accepts both site-relative destinations and absolute ones on
/// `site`. Handles balanced parentheses in the destination and an optional
/// CommonMark title (`[a](/url "title")`). Every occurrence is returned, in
/// order, each with its own span; deduplication of downloads is the caller's
/// job.
pub fn extract_upload_links(body: &str, site: &str) -> Vec<UploadLink> {
    let site = site.trim_end_matches('/');
    let mut links = Vec::new();
    for link in markdown_links(body) {
        let target = &body[link.span.clone()];
        let server_path = if target.starts_with("/user_uploads/") {
            target.to_string()
        } else if let Some(path) = target.strip_prefix(site) {
            if !path.starts_with("/user_uploads/") {
                continue;
            }
            path.to_string()
        } else {
            continue;
        };

        links.push(UploadLink {
            span: link.span,
            filename: sanitize_filename(server_path.rsplit('/').next().unwrap_or("")),
            server_path,
        });
    }
    links
}

/// Keep only filesystem-safe characters so a hostile upload name cannot
/// escape the attachments directory.
fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let cleaned = cleaned.trim_matches('.').to_string();
    if cleaned.is_empty() {
        "file".to_string()
    } else {
        cleaned
    }
}

/// Download every `/user_uploads/` link in `body` into
/// `<dir>/messages/attachments/` and rewrite the links to chamber-relative
/// local paths, so the agent can read the files directly.
///
/// Files are named `<msg_id>-<idx>_<filename>` with one download per unique
/// server path; an already-present file is not re-downloaded, which makes
/// re-pulls idempotent. A failed download leaves its links untouched. Only
/// the parsed destination spans are rewritten, so identical text elsewhere in
/// the body is never touched. Returns the (possibly rewritten) body plus
/// human-readable warnings for the caller to log; this function never fails
/// the pull.
pub fn localize_upload_links(
    body: &str,
    site: &str,
    msg_id: &str,
    dir: &Path,
    mut fetch: impl FnMut(&str) -> Result<Vec<u8>>,
) -> (String, Vec<String>) {
    let links = extract_upload_links(body, site);
    let mut warnings = Vec::new();
    let attach_dir = dir.join(ATTACHMENTS_SUBDIR);

    // One download per unique server path, indexed in first-appearance order.
    // A path maps to Some(local relative path) on success, None on failure.
    let mut resolved: BTreeMap<String, Option<String>> = BTreeMap::new();
    let mut next_idx = 0usize;
    for link in &links {
        if resolved.contains_key(&link.server_path) {
            continue;
        }
        let local_name = format!("{msg_id}-{next_idx}_{}", link.filename);
        next_idx += 1;
        let local_abs = attach_dir.join(&local_name);
        if !local_abs.exists() {
            // Write to a temp name and rename: a file at the final name is
            // always a complete download, which is what makes the exists()
            // skip above safe across crashes and re-pulls.
            let tmp_abs = attach_dir.join(format!("{local_name}.part"));
            let write_result = fetch(&link.server_path).and_then(|bytes| {
                std::fs::create_dir_all(&attach_dir)?;
                std::fs::write(&tmp_abs, bytes)?;
                std::fs::rename(&tmp_abs, &local_abs)?;
                Ok(())
            });
            if let Err(e) = write_result {
                warnings.push(format!(
                    "attachment download failed for {}: {e}",
                    link.server_path
                ));
                resolved.insert(link.server_path.clone(), None);
                continue;
            }
        }
        resolved.insert(
            link.server_path.clone(),
            Some(format!("{ATTACHMENTS_SUBDIR}/{local_name}")),
        );
    }

    // Splice destination spans back-to-front so earlier offsets stay valid.
    let mut new_body = body.to_string();
    for link in links.iter().rev() {
        if let Some(Some(local_rel)) = resolved.get(&link.server_path) {
            new_body.replace_range(link.span.clone(), local_rel);
        }
    }
    (new_body, warnings)
}

// --- Outbound attachment upload ---

/// Pick a multipart boundary that does not occur in the payload. Randomness is
/// unavailable (and unnecessary): the only requirement is absence from the
/// body, so extend a fixed marker until that holds.
fn multipart_boundary(payload: &[u8]) -> String {
    let mut boundary = "cryochamberBoundary7MA4YWxkTrZu0gW".to_string();
    while payload
        .windows(boundary.len())
        .any(|w| w == boundary.as_bytes())
    {
        boundary.push('X');
    }
    boundary
}

/// Extract the server path from a `POST /user_uploads` response. Zulip returns
/// `uri` on older servers and `url` on newer ones; accept either.
pub fn parse_upload_response(json: &serde_json::Value) -> Result<String> {
    json["url"]
        .as_str()
        .or_else(|| json["uri"].as_str())
        .map(|s| s.to_string())
        .context("upload response missing 'url'/'uri'")
}

/// Decide whether a markdown link destination names a chamber-local file this
/// sync may upload, returning its absolute path.
///
/// Refuses anything that is not a plain relative path to an existing regular
/// file inside the chamber. In particular `.cryo/` is never uploadable — it
/// holds `zuliprc`, i.e. the bot's API key, which must never leave the machine.
/// Symlinks are resolved before the containment and `.cryo` checks, so neither
/// can be used to escape.
///
/// This guards against an agent *accidentally* linking something sensitive,
/// not against a determined one: the agent runs as the same user and can read
/// `.cryo/zuliprc` (or hard-link it under an innocent name) directly. The
/// trust boundary is the agent, not this function.
pub fn resolve_local_attachment(dir: &Path, target: &str) -> Option<PathBuf> {
    if target.is_empty()
        || target.contains("://")
        || target.starts_with('/')
        || target.starts_with('#')
        || target.starts_with("mailto:")
    {
        return None;
    }
    let candidate = dir.join(target);
    if !candidate.is_file() {
        return None;
    }
    let root = dir.canonicalize().ok()?;
    let resolved = candidate.canonicalize().ok()?;
    if !resolved.starts_with(&root) {
        return None;
    }
    if resolved
        .strip_prefix(&root)
        .ok()?
        .components()
        .any(|c| c.as_os_str() == ".cryo")
    {
        return None;
    }
    Some(resolved)
}

/// Prepare an outbox body for Zulip so linked images actually render inline:
/// upload chamber-local files referenced by markdown links, rewrite those
/// links to absolute upload URLs on `site`, and drop the `!` from image
/// syntax.
///
/// Both edits are required. Verified against a live Zulip 11.4 server:
///
/// - `[text](/user_uploads/...)` (relative) renders as a bare link, no preview.
/// - `[text](https://site/user_uploads/...)` renders the link *and* an inline
///   image preview — absolute is what the preview pass matches on.
/// - `![text](...)` additionally leaves a literal `![text](` in the message,
///   because Zulip only implements CommonMark image syntax from server 12.0
///   (feature level 437).
///
/// Returns the rewritten body plus warnings for the caller to log. A failed
/// upload leaves its link untouched and never fails the push — the message
/// still reaches the operator, just without the inline image.
pub fn externalize_local_links(
    body: &str,
    dir: &Path,
    site: &str,
    mut upload: impl FnMut(&Path) -> Result<String>,
) -> (String, Vec<String>) {
    let links = markdown_links(body);
    let mut warnings = Vec::new();
    // One upload per distinct local file, keyed by resolved path.
    let mut uploaded: BTreeMap<PathBuf, Option<String>> = BTreeMap::new();
    // Edits as (range, replacement); applied back-to-front so offsets hold.
    let mut edits: Vec<(std::ops::Range<usize>, String)> = Vec::new();

    let site = site.trim_end_matches('/');
    for link in &links {
        let target = &body[link.span.clone()];
        // Resolve this link to an upload path, uploading a local file when
        // needed. Skipping means the link is not an attachment: leave it be.
        let server_path = if let Some(local) = resolve_local_attachment(dir, target) {
            let entry = uploaded
                .entry(local.clone())
                .or_insert_with(|| match upload(&local) {
                    Ok(path) => Some(path),
                    Err(e) => {
                        warnings.push(format!("attachment upload failed for {target}: {e:#}"));
                        None
                    }
                });
            let Some(path) = entry.clone() else { continue };
            path
        } else if let Some(rest) = target.strip_prefix(site) {
            // Already absolute on this server (e.g. the agent uploaded it
            // itself); only the `!` may still need stripping.
            if !rest.starts_with("/user_uploads/") {
                continue;
            }
            rest.to_string()
        } else if target.starts_with("/user_uploads/") {
            target.to_string()
        } else {
            continue;
        };

        if !server_path.starts_with("/user_uploads/") {
            continue;
        }
        let absolute = format!("{site}{server_path}");
        if absolute != target {
            edits.push((link.span.clone(), absolute));
        }
        if let Some(bang) = link.bang_at {
            edits.push((bang..bang + 1, String::new()));
        }
    }

    let mut new_body = body.to_string();
    edits.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
    // Applying two edits to the same range would splice the replacement twice
    // and eat neighbouring bytes. Nothing should queue duplicates, but a
    // corrupted message reaches the operator silently, so drop them here too.
    edits.dedup_by(|(a, _), (b, _)| a == b);
    for (range, replacement) in edits {
        new_body.replace_range(range, &replacement);
    }
    (new_body, warnings)
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
