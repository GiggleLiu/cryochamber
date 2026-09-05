mod credentials;
mod pinned;
mod probe;

use std::sync::Mutex;

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "android"))]
use tauri::{Emitter, Manager};

#[derive(Default)]
struct OpenedUrls(Mutex<Vec<String>>);

#[tauri::command]
fn take_opened_urls(urls: tauri::State<'_, OpenedUrls>) -> Vec<String> {
    std::mem::take(&mut *urls.0.lock().unwrap_or_else(|e| e.into_inner()))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(credentials::init())
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        // The live event streams, so a cancel from the console can find the
        // stream it means.
        .manage(pinned::SseStreams::default())
        .manage(OpenedUrls::default())
        .invoke_handler(tauri::generate_handler![
            take_opened_urls,
            credentials::load_credentials,
            credentials::save_credentials,
            probe::probe_hub,
            pinned::pinned_fetch,
            pinned::pinned_sse,
            pinned::pinned_sse_cancel
        ])
        .build(tauri::generate_context!())
        .expect("error while building cryochamber app")
        .run(|_app, _event| {
            #[cfg(any(target_os = "macos", target_os = "ios", target_os = "android"))]
            if let tauri::RunEvent::Opened { urls } = _event {
                let urls = urls
                    .into_iter()
                    .map(|url| url.to_string())
                    .collect::<Vec<_>>();
                _app.state::<OpenedUrls>()
                    .0
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .extend(urls.clone());
                let _ = _app.emit("open-urls", urls);
            }
        });
}
