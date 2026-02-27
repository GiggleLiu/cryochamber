use anyhow::{Context, Result};
use std::path::Path;

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
    #[allow(dead_code)]
    fn get(&self, endpoint: &str, params: &[(&str, &str)]) -> Result<serde_json::Value> {
        let url = self.api_url(endpoint);
        let mut req = self.agent.get(&url).header("Authorization", &self.basic_auth());
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
    #[allow(dead_code)]
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
