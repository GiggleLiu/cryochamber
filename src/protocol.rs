// src/protocol.rs
use anyhow::Result;
use std::path::Path;

/// Protocol content written to the agent's working directory as CLAUDE.md or AGENTS.md.
/// Source: templates/protocol.md
pub const PROTOCOL_CONTENT: &str = include_str!("../templates/protocol.md");

/// Template plan written by `cryo init` if no plan.md exists.
/// Source: templates/plan.md
pub const TEMPLATE_PLAN: &str = include_str!("../templates/plan.md");

/// Makefile written to the agent's working directory.
/// Source: templates/Makefile
pub const MAKEFILE_CONTENT: &str = include_str!("../templates/Makefile");

/// Config template written by `cryo init`.
/// Source: templates/cryo.toml
pub const CONFIG_TEMPLATE: &str = include_str!("../templates/cryo.toml");

/// Determine the protocol filename based on the agent command.
/// Returns `"CLAUDE.md"` if the executable name contains "claude", otherwise `"AGENTS.md"`.
/// Only inspects the first token (executable), so flags like `--model claude-3.7` are ignored.
pub fn protocol_filename(agent_cmd: &str) -> &'static str {
    let executable = agent_cmd
        .split_whitespace()
        .next()
        .unwrap_or("")
        .rsplit('/')
        .next()
        .unwrap_or("");
    if executable.to_lowercase().contains("claude") {
        "CLAUDE.md"
    } else {
        "AGENTS.md"
    }
}

/// Write the protocol file to the given directory.
/// Skips writing if the file already exists (no-clobber). Returns true if written.
pub fn write_protocol_file(dir: &Path, filename: &str) -> Result<bool> {
    let path = dir.join(filename);
    if path.exists() {
        return Ok(false);
    }
    std::fs::write(path, PROTOCOL_CONTENT)?;
    Ok(true)
}

/// Check if a protocol file (CLAUDE.md or AGENTS.md) exists in the directory.
/// Returns the filename if found.
pub fn find_protocol_file(dir: &Path) -> Option<&'static str> {
    if dir.join("CLAUDE.md").exists() {
        Some("CLAUDE.md")
    } else if dir.join("AGENTS.md").exists() {
        Some("AGENTS.md")
    } else {
        None
    }
}

/// Write a template plan.md if none exists. Returns true if written.
pub fn write_template_plan(dir: &Path) -> Result<bool> {
    let path = dir.join("plan.md");
    if path.exists() {
        return Ok(false);
    }
    std::fs::write(path, TEMPLATE_PLAN)?;
    Ok(true)
}

/// Write the agent Makefile if none exists. Returns true if written.
pub fn write_makefile(dir: &Path) -> Result<bool> {
    let path = dir.join("Makefile");
    if path.exists() {
        return Ok(false);
    }
    std::fs::write(path, MAKEFILE_CONTENT)?;
    Ok(true)
}

/// Write cryo.toml config file if none exists. Returns true if written.
/// Substitutes `{{agent}}` with the given agent command.
pub fn write_config_file(dir: &Path, agent_cmd: &str) -> Result<bool> {
    let path = dir.join("cryo.toml");
    if path.exists() {
        return Ok(false);
    }
    let content = CONFIG_TEMPLATE.replace("{{agent}}", agent_cmd);
    std::fs::write(path, content)?;
    Ok(true)
}
