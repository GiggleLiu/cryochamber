//! `/api/chambers` routes: list + refresh.

use std::collections::HashMap;
use std::sync::Arc;

use axum::{extract::State, http::StatusCode, response::Json, Extension};
use serde::Deserialize;
use serde_json::Value;

use crate::hub::state::AppState;
use crate::hub::tokens::Role;

/// Validate a user-supplied chamber name. Returns `Ok(())` if safe to use as
/// a directory name under the configured chamber root, otherwise a one-line reason string
/// suitable for the `error` field of a 400 response.
pub fn validate_chamber_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("name is empty".to_string());
    }
    if name.len() > 64 {
        return Err("name too long (max 64 chars)".to_string());
    }
    if name.starts_with('.') {
        return Err("name contains illegal characters".to_string());
    }
    if name == ".." {
        return Err("name contains illegal characters".to_string());
    }
    for c in name.chars() {
        if c == '/' || c == '\\' || c.is_whitespace() || c.is_control() {
            return Err("name contains illegal characters".to_string());
        }
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct NewChamberPayload {
    pub name: String,
    pub api_key_provider: Option<String>,
    pub api_key: Option<String>,
    pub model: Option<String>,
    /// Start the chamber after scaffolding. Optional on the wire so existing
    /// API clients retain the original create-only behavior.
    #[serde(default)]
    pub start: bool,
}

/// List chambers. An invite sees only the chambers its token is scoped to —
/// the sidebar must not reveal that other chambers exist. Owner and open
/// (no-token) mode see the whole index.
pub async fn get_chambers(
    State(app): State<Arc<AppState>>,
    role: Option<Extension<Role>>,
) -> Json<Value> {
    // Snapshot the index under a short-lived reader, then run blocking
    // per-chamber I/O (state/todos/inbox reads + libc::kill probes) off the
    // async worker. Only reacquire the writer to swap the populated snapshot
    // back in. This avoids holding the std RwLock writer across filesystem
    // walks, which would otherwise stall every concurrent route that calls
    // `app.resolve(..)` (they all go through `chambers.read()`).
    let app_task = app.clone();
    let value = tokio::task::spawn_blocking(move || {
        let mut snapshot = app_task
            .chambers
            .read()
            .map(|g| g.clone())
            .unwrap_or_default();
        crate::hub::discovery::populate_runtime(&mut snapshot);
        let value = serde_json::to_value(snapshot.values().collect::<Vec<_>>())
            .unwrap_or(Value::Array(vec![]));
        if let Ok(mut idx) = app_task.chambers.write() {
            *idx = snapshot;
        }
        value
    })
    .await
    .unwrap_or(Value::Array(vec![]));
    let value = match role {
        Some(Extension(Role::Invite { chambers, .. })) => match value {
            Value::Array(items) => Value::Array(
                items
                    .into_iter()
                    .filter(|c| {
                        c["id"]
                            .as_str()
                            .map(|id| crate::hub::auth::scope_covers(&chambers, id))
                            .unwrap_or(false)
                    })
                    .collect(),
            ),
            other => other,
        },
        _ => value,
    };
    Json(value)
}

pub async fn post_refresh(State(app): State<Arc<AppState>>) -> Json<Value> {
    app.refresh();
    let idx = app.chambers.read().unwrap();
    let list: Vec<&crate::hub::discovery::ChamberEntry> = idx.values().collect();
    Json(serde_json::to_value(&list).unwrap_or(Value::Array(vec![])))
}

pub async fn post_new(
    State(app): State<Arc<AppState>>,
    Json(payload): Json<NewChamberPayload>,
) -> (StatusCode, Json<Value>) {
    if let Err(e) = validate_chamber_name(&payload.name) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e })),
        );
    }
    let provider_config = match ProviderConfigInput::from_payload(&payload) {
        Ok(provider_config) => provider_config,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": e })),
            )
        }
    };

    let default_agent = app
        .default_agent
        .read()
        .map(|agent| agent.clone())
        .unwrap_or_else(|_| crate::config::default_agent());
    if provider_config.as_ref().is_some_and(|p| p.model.is_some()) {
        if let Some(error) = model_refusal(&default_agent) {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": error })),
            );
        }
    }
    if payload.start {
        if let Err(error) = crate::hub::lifecycle::validate_agent_command(&default_agent) {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": format!("cannot start chamber: {error}")
                })),
            );
        }
    }

    let workspace = app.workspace_dir.clone();
    let target = workspace.join(&payload.name);

    if !path_under(&workspace, &target) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "name resolves outside the chamber root"
            })),
        );
    }

    if let Err(e) = std::fs::create_dir_all(&workspace) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": format!("failed to create chamber root: {e}")
            })),
        );
    }

    match std::fs::create_dir(&target) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "chamber already exists" })),
            );
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": format!("failed to create directory: {e}")
                })),
            );
        }
    }

    if let Err(e) = crate::protocol::scaffold_chamber(&target, &default_agent) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": format!("scaffold failed: {e}")
            })),
        );
    }
    if let Some(provider_config) = provider_config {
        if let Err(e) = write_provider_config(&target, &provider_config) {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": format!("config failed: {e}")
                })),
            );
        }
    }
    if app.discovery_options.include_registry {
        if let Err(e) = crate::registry::remember_chamber(&target) {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": format!("registry update failed: {e}")
                })),
            );
        }
    }

    let start_error = if payload.start {
        let target = target.clone();
        match tokio::task::spawn_blocking(move || crate::hub::lifecycle::start_chamber(&target))
            .await
        {
            Ok(Ok(())) => None,
            Ok(Err(error)) => Some(error.to_string()),
            Err(error) => Some(format!("start task failed: {error}")),
        }
    } else {
        None
    };

    app.refresh();
    let id = app
        .chambers
        .read()
        .ok()
        .and_then(|idx| {
            idx.iter()
                .find(|(_, c)| c.name == payload.name)
                .map(|(id, _)| id.clone())
        })
        .unwrap_or_default();

    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "id": id,
            "started": payload.start && start_error.is_none(),
            "start_error": start_error,
        })),
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ProviderConfigInput {
    provider: String,
    api_key: String,
    model: Option<String>,
}

impl ProviderConfigInput {
    fn from_payload(payload: &NewChamberPayload) -> Result<Option<Self>, String> {
        let provider = trim_option(&payload.api_key_provider);
        let api_key = trim_option(&payload.api_key);
        let model = trim_option(&payload.model);
        if provider.is_none() && api_key.is_none() && model.is_none() {
            return Ok(None);
        }
        let provider = provider.ok_or_else(|| "api key provider is empty".to_string())?;
        if !is_valid_provider_id(&provider) {
            return Err("api key provider contains illegal characters".to_string());
        }
        let api_key = api_key.ok_or_else(|| "api key is empty".to_string())?;
        Ok(Some(Self {
            provider: provider.to_ascii_lowercase(),
            api_key,
            model,
        }))
    }
}

fn trim_option(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn is_valid_provider_id(value: &str) -> bool {
    value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn api_key_env_for_provider(provider: &str) -> String {
    match provider {
        "anthropic" => "ANTHROPIC_API_KEY".to_string(),
        "openai" => "OPENAI_API_KEY".to_string(),
        "openrouter" => "OPENROUTER_API_KEY".to_string(),
        "google" | "gemini" => "GOOGLE_GENERATIVE_AI_API_KEY".to_string(),
        "xai" => "XAI_API_KEY".to_string(),
        "deepseek" => "DEEPSEEK_API_KEY".to_string(),
        "alibaba" | "qwen" => "DASHSCOPE_API_KEY".to_string(),
        "kimi" | "moonshot" => "MOONSHOT_API_KEY".to_string(),
        "glm" | "zhipu" | "zhipuai" => "ZHIPUAI_API_KEY".to_string(),
        "minmax" | "minimax" => "MINIMAX_API_KEY".to_string(),
        "groq" => "GROQ_API_KEY".to_string(),
        "mistral" => "MISTRAL_API_KEY".to_string(),
        "cohere" => "COHERE_API_KEY".to_string(),
        "together" => "TOGETHER_API_KEY".to_string(),
        "perplexity" => "PERPLEXITY_API_KEY".to_string(),
        "fireworks" => "FIREWORKS_API_KEY".to_string(),
        _ => {
            let key = provider
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() {
                        c.to_ascii_uppercase()
                    } else {
                        '_'
                    }
                })
                .collect::<String>();
            format!("{key}_API_KEY")
        }
    }
}

/// The bare executable name of an agent command (`"pi --provider x"` -> `"pi"`).
fn agent_executable(agent_cmd: &str) -> anyhow::Result<String> {
    let program = crate::agent::agent_program(agent_cmd)?;
    Ok(program.rsplit('/').next().unwrap_or(&program).to_string())
}

/// Why a chosen model cannot reach this runner, or `None` if it can.
///
/// An API key is universal — every runner reads it from the environment — but a
/// *model* only reaches the runner through a runner-specific flag or env var,
/// and Cryohub knows those for `opencode` and `pi` alone. Writing the model
/// nowhere and creating the chamber anyway would hand the owner a chamber that
/// silently ignores the model they picked, so `post_new` refuses instead.
fn model_refusal(agent_cmd: &str) -> Option<String> {
    match agent_executable(agent_cmd) {
        Ok(exe) if exe == "opencode" || exe == "pi" => None,
        Ok(_) => Some(format!(
            "the host agent `{agent_cmd}` takes its model from its own command, not from Cryohub: \
             leave Model blank and add the model flag to `agent` in the chamber's cryo.toml"
        )),
        Err(error) => Some(format!(
            "the host agent `{agent_cmd}` is not a command Cryohub can parse ({error:#}): fix \
             `default_agent` in cryohub.toml before creating a chamber with a model"
        )),
    }
}

fn write_provider_config(
    target: &std::path::Path,
    provider: &ProviderConfigInput,
) -> anyhow::Result<()> {
    let config_path = crate::config::config_path(target);
    let mut config = crate::config::load_config(&config_path)?.unwrap_or_default();
    let mut env = HashMap::new();
    match agent_executable(&config.agent)?.as_str() {
        "opencode" => {
            env.insert("OPENCODE_PROVIDER".to_string(), provider.provider.clone());
            if let Some(model) = &provider.model {
                env.insert("OPENCODE_MODEL".to_string(), model.clone());
            }
        }
        "pi" => {
            config.agent.push_str(" --provider ");
            config
                .agent
                .push_str(&shell_words::quote(&provider.provider));
            if let Some(model) = &provider.model {
                config.agent.push_str(" --model ");
                config.agent.push_str(&shell_words::quote(model));
            }
        }
        // Any other runner: the API key below is the whole wiring. `post_new`
        // refuses a model for these before it creates the directory, with a
        // message that says why; this arm makes the drop impossible rather
        // than merely unreached, since it reads the *scaffolded* agent while
        // the guard reads the host default.
        other => {
            if provider.model.is_some() {
                anyhow::bail!("`{other}` takes its model from its own command, not from Cryohub");
            }
        }
    }
    env.insert(
        api_key_env_for_provider(&provider.provider),
        provider.api_key.clone(),
    );
    config.provider = Some(crate::config::ProviderConfig {
        name: provider.provider.clone(),
        env,
    });
    config.providers.clear();
    crate::config::save_config(&config_path, &config)?;
    Ok(())
}

/// Lexical containment check: `target` must be under `parent`. Used as
/// belt-and-suspenders against `..` even though `validate_chamber_name`
/// already rejects path separators.
fn path_under(parent: &std::path::Path, target: &std::path::Path) -> bool {
    let p = parent
        .components()
        .filter(|c| !matches!(c, std::path::Component::CurDir))
        .collect::<Vec<_>>();
    let t = target
        .components()
        .filter(|c| !matches!(c, std::path::Component::CurDir))
        .collect::<Vec<_>>();
    if t.len() <= p.len() {
        return false;
    }
    p.iter().zip(t.iter()).all(|(a, b)| a == b)
        && !t
            .iter()
            .any(|c| matches!(c, std::path::Component::ParentDir))
}

#[cfg(test)]
#[path = "../../unit_tests/hub/routes/chambers.rs"]
mod tests;
