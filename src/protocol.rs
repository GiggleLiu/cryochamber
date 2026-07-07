// src/protocol.rs
use anyhow::Result;
use std::path::Path;

/// Protocol content embedded into every runtime agent prompt.
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

/// `.gitignore` written by `cryo init` if none exists. The critical entry is
/// `.cryo/`, which holds sync credentials (`.cryo/zuliprc`) and the IPC socket
/// and must never be committed; the rest are per-chamber runtime artifacts.
pub const GITIGNORE_CONTENT: &str = "\
# Cryochamber runtime state and credentials (never commit)
.cryo/
timer.json
todo.json
messages/
*.log
";

/// Outcome of `scaffold_chamber`. Each `*_created` field is true if the file
/// was newly created, false if it already existed and was kept untouched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScaffoldReport {
    pub cryo_toml_created: bool,
    pub plan_created: bool,
    pub readme_created: bool,
    pub notes_created: bool,
    pub gitignore_created: bool,
}

/// Scaffold a fresh chamber under `dir` for the given `agent_cmd`. Creates
/// `cryo.toml`, `plan.md`, `README.md`, `NOTES.md`, `.gitignore`, and ensures
/// the `messages/` directory tree.
/// Each file is no-clobber: existing files are kept and reported as
/// `*_created: false`. Used by both `cryo init` and the hub's
/// `POST /api/chambers/new` route so the two paths stay in lockstep.
pub fn scaffold_chamber(dir: &Path, agent_cmd: &str) -> Result<ScaffoldReport> {
    let cryo_toml_created = write_config_file(dir, agent_cmd)?;
    let plan_created = write_template_plan(dir)?;
    let readme_created = write_readme(dir)?;
    let notes_created = write_notes_file(dir)?;
    let gitignore_created = write_gitignore(dir)?;
    crate::channel::store::MessageStore::new(dir.to_path_buf()).ensure_dirs()?;
    Ok(ScaffoldReport {
        cryo_toml_created,
        plan_created,
        readme_created,
        notes_created,
        gitignore_created,
    })
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

/// Write `.gitignore` if none exists. Returns true if written. Keeps the
/// credential/socket dir `.cryo/` out of git along with other runtime state.
/// No-clobber: a hand-authored `.gitignore` is kept untouched (`cryo-zulip
/// init` separately ensures `.cryo/` is present in that case).
pub fn write_gitignore(dir: &Path) -> Result<bool> {
    let path = dir.join(".gitignore");
    write_file_if_missing(&path, GITIGNORE_CONTENT)
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
