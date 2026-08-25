//! Reaching a hub whose certificate the user pinned.
//!
//! The WebView will not do it: a self-signed hub is refused by the system
//! trust store, and there is no way to tell the WebView "trust exactly this
//! certificate". So the request is made here instead, over a client whose only
//! certificate judgement is [`CapturingVerifier`] comparing the fingerprint
//! against the pin — the same verifier the probe used to show the user which
//! certificate they were pinning.
//!
//! Two shapes, because the console asks for two: everything is buffered into
//! one response, except `/api/events`, which is a stream that stays open and
//! is fed to the console down a channel.

use std::collections::HashMap;
use std::sync::Mutex;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use futures_util::StreamExt;
use tauri::async_runtime::JoinHandle;
use tauri::ipc::Channel;

use crate::probe::{client_with_verifier_timeout, CapturingVerifier};

/// One request the console would otherwise have made with `fetch`. The body
/// crosses as base64: the IPC bridge speaks JSON, and an upload is bytes.
#[derive(serde::Deserialize)]
pub struct PinnedRequest {
    pub url: String,
    pub method: String,
    pub headers: Vec<(String, String)>,
    pub body_b64: Option<String>,
    /// The pinned end-entity fingerprint, lowercase hex. Every connection this
    /// module opens is judged against it.
    pub sha256: String,
}

#[derive(serde::Serialize)]
pub struct PinnedResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body_b64: String,
}

/// The request body, decoded. Separate from the command so the decode is
/// testable without a hub on the other end.
pub fn decode_body(req: &PinnedRequest) -> Result<Option<Vec<u8>>, String> {
    match &req.body_b64 {
        None => Ok(None),
        Some(encoded) => STANDARD
            .decode(encoded)
            .map(Some)
            .map_err(|_| "The request body could not be read.".to_string()),
    }
}

/// rustls reports the pin refusal several layers down, and `Display` on a
/// reqwest error shows only the top one ("error sending request").
fn detail_of(error: &reqwest::Error) -> String {
    let mut detail = error.to_string();
    let mut source = std::error::Error::source(error);
    while let Some(cause) = source {
        detail.push_str(": ");
        detail.push_str(&cause.to_string());
        source = cause.source();
    }
    detail
}

/// What [`CapturingVerifier`] says when the certificate is not the pinned one.
const PIN_MISMATCH: &str = "pinned fingerprint mismatch";

/// The sentence the console shows. A refused pin is the one failure a user can
/// act on, and reqwest's own words for it name nothing they would recognise.
pub fn transport_message(detail: &str) -> String {
    if detail.contains(PIN_MISMATCH) {
        "This hub is presenting a different certificate than the one you pinned.".to_string()
    } else {
        detail.to_string()
    }
}

fn transport_error(error: &reqwest::Error) -> String {
    transport_message(&detail_of(error))
}

/// A client pinned to this fingerprint. No whole-request deadline: an upload
/// and an event stream both outlive any number the shell could pick.
fn pinned_client(sha256: &str) -> Result<reqwest::Client, String> {
    let verifier = std::sync::Arc::new(CapturingVerifier::new(Some(sha256.to_string())));
    client_with_verifier_timeout(verifier, None)
}

fn header_pairs(headers: &reqwest::header::HeaderMap) -> Vec<(String, String)> {
    headers
        .iter()
        .filter_map(|(name, value)| {
            // A header whose bytes are not text cannot cross into the console;
            // dropping it beats failing the whole response over it.
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_string(), value.to_string()))
        })
        .collect()
}

/// One buffered request through the pinned client.
#[tauri::command]
pub async fn pinned_fetch(req: PinnedRequest) -> Result<PinnedResponse, String> {
    let body = decode_body(&req)?;
    let method = reqwest::Method::from_bytes(req.method.as_bytes())
        .map_err(|_| format!("{} is not an HTTP method.", req.method))?;
    let client = pinned_client(&req.sha256)?;
    let mut request = client.request(method, &req.url);
    for (name, value) in &req.headers {
        request = request.header(name, value);
    }
    if let Some(body) = body {
        request = request.body(body);
    }
    let response = request.send().await.map_err(|e| transport_error(&e))?;
    let status = response.status().as_u16();
    let headers = header_pairs(response.headers());
    let bytes = response.bytes().await.map_err(|e| transport_error(&e))?;
    Ok(PinnedResponse {
        status,
        headers,
        body_b64: STANDARD.encode(&bytes),
    })
}

/// What the console's `ReadableStream` is fed. Untagged, so each message is
/// just its fields and the console tells them apart by which field is there.
#[derive(Clone, serde::Serialize)]
#[serde(untagged)]
pub enum SseEvent {
    Open { status: u16 },
    Chunk { chunk_b64: String },
    Done { done: bool },
}

/// The live event streams, by the id the console minted for each. A stream
/// outlives the call that started it only in the sense that the console can
/// reach in and stop it; nothing else looks in here.
#[derive(Default)]
pub struct SseStreams(pub Mutex<HashMap<u64, JoinHandle<()>>>);

impl SseStreams {
    fn insert(&self, stream_id: u64, handle: JoinHandle<()>) {
        // A poisoned lock means another stream panicked while holding it; the
        // map itself is still sound, so carry on rather than take the app down.
        let mut streams = self.0.lock().unwrap_or_else(|e| e.into_inner());
        streams.insert(stream_id, handle);
    }

    fn take(&self, stream_id: u64) -> Option<JoinHandle<()>> {
        let mut streams = self.0.lock().unwrap_or_else(|e| e.into_inner());
        streams.remove(&stream_id)
    }
}

/// Stop one stream and forget it. Unknown ids are ordinary: the console cancels
/// on every teardown path, including ones where the stream already ended.
pub fn cancel_stream(state: &SseStreams, stream_id: u64) {
    if let Some(handle) = state.take(stream_id) {
        handle.abort();
    }
}

/// The stream itself: connect, say what the hub answered, then forward bytes
/// as they arrive. Every send failure ends the stream — a channel that no
/// longer delivers means the console has gone.
async fn run_stream(
    url: String,
    sha256: String,
    headers: Vec<(String, String)>,
    on_event: Channel<SseEvent>,
) -> Result<(), String> {
    let client = pinned_client(&sha256)?;
    let mut request = client.get(&url);
    for (name, value) in &headers {
        request = request.header(name, value);
    }
    let response = request.send().await.map_err(|e| transport_error(&e))?;
    on_event
        .send(SseEvent::Open {
            status: response.status().as_u16(),
        })
        .map_err(|_| "The console stopped listening to this stream.".to_string())?;
    let mut body = response.bytes_stream();
    while let Some(chunk) = body.next().await {
        let chunk = chunk.map_err(|e| transport_error(&e))?;
        on_event
            .send(SseEvent::Chunk {
                chunk_b64: STANDARD.encode(&chunk),
            })
            .map_err(|_| "The console stopped listening to this stream.".to_string())?;
    }
    on_event
        .send(SseEvent::Done { done: true })
        .map_err(|_| "The console stopped listening to this stream.".to_string())?;
    Ok(())
}

/// Hold the console's `/api/events` connection open until it ends or is
/// cancelled. The work runs in a task so `pinned_sse_cancel` has something to
/// abort; the outcome comes back over a channel rather than by joining, because
/// an aborted task has no outcome to report and cancellation is not a failure.
#[tauri::command]
pub async fn pinned_sse(
    state: tauri::State<'_, SseStreams>,
    stream_id: u64,
    url: String,
    sha256: String,
    headers: Vec<(String, String)>,
    on_event: Channel<SseEvent>,
) -> Result<(), String> {
    let (tx, mut rx) = tauri::async_runtime::channel::<Result<(), String>>(1);
    let handle = tauri::async_runtime::spawn(async move {
        let outcome = run_stream(url, sha256, headers, on_event).await;
        let _ = tx.send(outcome).await;
    });
    state.insert(stream_id, handle);
    // A cancelled task drops its sender, which closes the channel: that is the
    // `None` below, and it means the console asked for this and is not waiting
    // to be told anything went wrong.
    let outcome = rx.recv().await.unwrap_or(Ok(()));
    cancel_stream(&state, stream_id);
    outcome
}

/// Stop a live stream. The console calls this when its reader is released or
/// its `AbortSignal` fires.
#[tauri::command]
pub fn pinned_sse_cancel(state: tauri::State<'_, SseStreams>, stream_id: u64) {
    cancel_stream(&state, stream_id);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request_with(body_b64: Option<String>) -> PinnedRequest {
        PinnedRequest {
            url: "https://hub.example/api/whoami".into(),
            method: "POST".into(),
            headers: vec![],
            body_b64,
            sha256: "0".repeat(64),
        }
    }

    #[test]
    fn request_body_decodes_from_b64() {
        let req = request_with(Some(STANDARD.encode(b"{\"a\":1}")));
        assert_eq!(decode_body(&req).unwrap().unwrap(), b"{\"a\":1}");
    }

    #[test]
    fn a_bodyless_request_decodes_to_nothing() {
        assert!(decode_body(&request_with(None)).unwrap().is_none());
    }

    #[test]
    fn a_body_that_is_not_base64_is_refused_rather_than_guessed_at() {
        let err = decode_body(&request_with(Some("not base64!!".into()))).unwrap_err();
        assert!(err.contains("could not be read"), "{err}");
    }

    #[test]
    fn unknown_stream_cancel_is_a_noop() {
        let streams = SseStreams::default();
        cancel_stream(&streams, 7); // must not panic
    }

    #[test]
    fn a_refused_pin_is_reported_in_words_a_user_can_act_on() {
        let said = transport_message(
            "error sending request: invalid peer certificate: Other(General(\"pinned fingerprint mismatch\"))",
        );
        assert!(said.contains("different certificate"), "{said}");
        // Everything else keeps the transport's own words — they are the only
        // description of what actually went wrong.
        assert_eq!(
            transport_message("connection refused"),
            "connection refused"
        );
    }

    /// The console tells the three messages apart by which field is present,
    /// so the wire shape is part of the contract.
    #[test]
    fn stream_events_serialize_as_bare_fields() {
        let open = serde_json::to_string(&SseEvent::Open { status: 200 }).unwrap();
        let chunk = serde_json::to_string(&SseEvent::Chunk {
            chunk_b64: "aGk=".into(),
        })
        .unwrap();
        let done = serde_json::to_string(&SseEvent::Done { done: true }).unwrap();
        assert_eq!(open, r#"{"status":200}"#);
        assert_eq!(chunk, r#"{"chunk_b64":"aGk="}"#);
        assert_eq!(done, r#"{"done":true}"#);
    }
}
