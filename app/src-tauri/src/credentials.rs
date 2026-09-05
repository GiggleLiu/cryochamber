//! The fixed hub-token record lives in OS protected storage. Never fall back
//! to plaintext if a keychain is locked or a device key is unavailable.

#[cfg(target_os = "android")]
use tauri::Manager;

#[cfg(target_os = "android")]
struct Credentials(tauri::plugin::PluginHandle<tauri::Wry>);

pub fn init() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri::plugin::Builder::new("credentials")
        .setup(|_app, _api| {
            #[cfg(target_os = "android")]
            _app.manage(Credentials(_api.register_android_plugin(
                "com.cryochamber.console",
                "CredentialsPlugin",
            )?));
            Ok(())
        })
        .build()
}

#[tauri::command]
pub async fn load_credentials(_app: tauri::AppHandle) -> Result<Option<String>, String> {
    #[cfg(target_os = "macos")]
    {
        tauri::async_runtime::spawn_blocking(|| {
            match security_framework::passwords::get_generic_password(
                "com.cryochamber.console",
                "hub-tokens",
            ) {
                Ok(bytes) => String::from_utf8(bytes)
                    .map(Some)
                    .map_err(|_| "Invalid credential encoding".into()),
                Err(error) if error.code() == -25300 => Ok(None),
                Err(error) => Err(format!("Cannot read hub tokens from Keychain: {error}")),
            }
        })
        .await
        .map_err(|e| e.to_string())?
    }
    #[cfg(target_os = "android")]
    {
        #[derive(serde::Deserialize)]
        struct Response {
            value: Option<String>,
        }
        _app.state::<Credentials>()
            .0
            .run_mobile_plugin::<Response>("load", ())
            .map(|response| response.value)
            .map_err(|e| e.to_string())
    }
    #[cfg(not(any(target_os = "macos", target_os = "android")))]
    Err("Native token storage is supported on macOS and Android. Use the browser Console on this platform.".into())
}

#[tauri::command]
pub async fn save_credentials(_app: tauri::AppHandle, value: String) -> Result<(), String> {
    if value.len() > 1_048_576 {
        return Err("Hub token storage exceeds 1 MiB".into());
    }
    #[cfg(target_os = "macos")]
    {
        tauri::async_runtime::spawn_blocking(move || {
            security_framework::passwords::set_generic_password(
                "com.cryochamber.console",
                "hub-tokens",
                value.as_bytes(),
            )
            .map_err(|error| format!("Cannot save hub tokens in Keychain: {error}"))
        })
        .await
        .map_err(|e| e.to_string())?
    }
    #[cfg(target_os = "android")]
    {
        _app.state::<Credentials>()
            .0
            .run_mobile_plugin::<serde_json::Value>("save", serde_json::json!({ "value": value }))
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    #[cfg(not(any(target_os = "macos", target_os = "android")))]
    Err("Native token storage is supported on macOS and Android. Use the browser Console on this platform.".into())
}
