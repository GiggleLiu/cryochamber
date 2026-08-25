mod pinned;
mod probe;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        // The live event streams, so a cancel from the console can find the
        // stream it means.
        .manage(pinned::SseStreams::default())
        .invoke_handler(tauri::generate_handler![
            probe::probe_hub,
            pinned::pinned_fetch,
            pinned::pinned_sse,
            pinned::pinned_sse_cancel
        ])
        .run(tauri::generate_context!())
        .expect("error while running cryochamber app");
}
