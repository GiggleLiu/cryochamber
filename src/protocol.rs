// src/protocol.rs
use anyhow::Result;
use std::path::Path;

/// Protocol content written to the agent's working directory as CLAUDE.md or AGENTS.md.
/// Source: templates/protocol.md
pub const PROTOCOL_CONTENT: &str = include_str!("../templates/protocol.md");

/// Template plan written by `cryo init` if no plan.md exists.
/// Source: templates/plan.md
pub const TEMPLATE_PLAN: &str = include_str!("../templates/plan.md");

/// Config template written by `cryo init`.
/// Source: templates/cryo.toml
pub const CONFIG_TEMPLATE: &str = include_str!("../templates/cryo.toml");

/// README template written by `cryo init`.
/// Source: templates/README.md
pub const README_TEMPLATE: &str = include_str!("../templates/README.md");

/// NOTES.md template written by `cryo init`.
/// Source: templates/notes.md
pub const NOTES_TEMPLATE: &str = include_str!("../templates/notes.md");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolFile {
    Claude,
    Agents,
}

impl ProtocolFile {
    pub fn filename(self) -> &'static str {
        match self {
            Self::Claude => "CLAUDE.md",
            Self::Agents => "AGENTS.md",
        }
    }

    fn matches_executable(self, executable: &str) -> bool {
        let executable = executable.to_ascii_lowercase();
        match self {
            Self::Claude => executable.contains("claude"),
            Self::Agents => true,
        }
    }
}

const PROTOCOL_FILE_PRIORITY: &[ProtocolFile] = &[ProtocolFile::Claude, ProtocolFile::Agents];

fn agent_executable_name(agent_cmd: &str) -> String {
    agent_cmd
        .split_whitespace()
        .next()
        .unwrap_or("")
        .rsplit('/')
        .next()
        .unwrap_or("")
        .to_lowercase()
}

pub fn protocol_file_for_agent(agent_cmd: &str) -> ProtocolFile {
    let executable = agent_executable_name(agent_cmd);
    PROTOCOL_FILE_PRIORITY
        .iter()
        .copied()
        .find(|file| file.matches_executable(&executable))
        .unwrap_or(ProtocolFile::Agents)
}

/// Determine the protocol filename based on the agent command.
/// Returns `"CLAUDE.md"` if the executable name contains "claude", otherwise `"AGENTS.md"`.
/// Only inspects the first token (executable), so flags like `--model claude-3.7` are ignored.
pub fn protocol_filename(agent_cmd: &str) -> &'static str {
    protocol_file_for_agent(agent_cmd).filename()
}

/// Write the protocol file to the given directory.
/// Skips writing if the file already exists (no-clobber). Returns true if written.
pub fn write_protocol_file(dir: &Path, filename: &str) -> Result<bool> {
    let path = dir.join(filename);
    write_file_if_missing(&path, PROTOCOL_CONTENT)
}

/// Check if a protocol file (CLAUDE.md or AGENTS.md) exists in the directory.
/// Returns the filename if found.
pub fn find_protocol_file(dir: &Path) -> Option<&'static str> {
    PROTOCOL_FILE_PRIORITY
        .iter()
        .map(|file| file.filename())
        .find(|filename| dir.join(filename).exists())
}

/// Write a template plan.md if none exists. Returns true if written.
pub fn write_template_plan(dir: &Path) -> Result<bool> {
    let path = dir.join("plan.md");
    write_file_if_missing(&path, TEMPLATE_PLAN)
}

/// Write cryo.toml config file if none exists. Returns true if written.
/// Substitutes `{{agent}}` with the given agent command.
pub fn write_config_file(dir: &Path, agent_cmd: &str) -> Result<bool> {
    let path = dir.join("cryo.toml");
    let content = CONFIG_TEMPLATE.replace("{{agent}}", agent_cmd);
    write_file_if_missing(&path, &content)
}

/// Write README.md if none exists. Returns true if written.
/// Substitutes `{{project_name}}` with the directory name.
pub fn write_readme(dir: &Path) -> Result<bool> {
    let path = dir.join("README.md");
    let project_name = dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("cryochamber-project");
    let content = README_TEMPLATE.replace("{{project_name}}", project_name);
    write_file_if_missing(&path, &content)
}

/// Write NOTES.md if none exists. Returns true if written.
pub fn write_notes_file(dir: &Path) -> Result<bool> {
    let path = dir.join("NOTES.md");
    write_file_if_missing(&path, NOTES_TEMPLATE)
}

fn write_file_if_missing(path: &Path, content: &str) -> Result<bool> {
    if path.exists() {
        return Ok(false);
    }
    std::fs::write(path, content)?;
    Ok(true)
}

#[cfg(test)]
#[path = "unit_tests/protocol.rs"]
mod tests;
