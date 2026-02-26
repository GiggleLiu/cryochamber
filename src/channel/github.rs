use anyhow::{Context, Result};
use chrono::NaiveDateTime;
use std::collections::BTreeMap;
use std::process::Command;

use crate::message::Message;

/// Get the login of the currently authenticated `gh` user.
pub fn whoami() -> Result<String> {
    let output = Command::new("gh")
        .args(["api", "user", "-q", ".login"])
        .output()
        .context("Failed to run `gh`. Is it installed and authenticated?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh api user failed: {stderr}");
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Call `gh api graphql` with a query string. Returns parsed JSON.
pub fn gh_graphql(query_body: &str) -> Result<serde_json::Value> {
    let output = Command::new("gh")
        .args(["api", "graphql", "-f", &format!("query={query_body}")])
        .output()
        .context("Failed to run `gh`. Is it installed and authenticated?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh api graphql failed: {stderr}");
    }

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("Failed to parse gh output as JSON")?;
    Ok(json)
}

// --- Helpers ---

/// Escape a string for embedding in a GraphQL JSON string literal.
fn escape_graphql(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

// --- Query Builders ---

pub fn build_fetch_comments_query(
    owner: &str,
    repo: &str,
    discussion_number: u64,
    after_cursor: Option<&str>,
) -> String {
    let owner = escape_graphql(owner);
    let repo = escape_graphql(repo);
    let after = match after_cursor {
        Some(c) => format!(", after: \"{}\"", escape_graphql(c)),
        None => String::new(),
    };
    format!(
        r#"{{ repository(owner: "{owner}", name: "{repo}") {{ discussion(number: {discussion_number}) {{ comments(first: 100{after}) {{ nodes {{ id body author {{ login }} createdAt }} pageInfo {{ endCursor hasNextPage }} }} }} }} }}"#
    )
}

pub fn build_post_comment_mutation(discussion_node_id: &str, body: &str) -> String {
    let escaped = escape_graphql(body);
    format!(
        r#"mutation {{ addDiscussionComment(input: {{discussionId: "{discussion_node_id}", body: "{escaped}"}}) {{ comment {{ id }} }} }}"#
    )
}

pub fn build_create_discussion_mutation(
    repo_node_id: &str,
    category_id: &str,
    title: &str,
    body: &str,
) -> String {
    let escaped_body = escape_graphql(body);
    let escaped_title = escape_graphql(title);
    format!(
        r#"mutation {{ createDiscussion(input: {{repositoryId: "{repo_node_id}", categoryId: "{category_id}", title: "{escaped_title}", body: "{escaped_body}"}}) {{ discussion {{ id number }} }} }}"#
    )
}

// --- Response Parsers ---

pub fn parse_discussion_comments(json: &serde_json::Value) -> Result<(Vec<Message>, String, bool)> {
    let comments = &json["data"]["repository"]["discussion"]["comments"];
    let nodes = comments["nodes"]
        .as_array()
        .context("Missing comments.nodes")?;
    let page_info = &comments["pageInfo"];

    let end_cursor = page_info["endCursor"].as_str().unwrap_or("").to_string();
    let has_next = page_info["hasNextPage"].as_bool().unwrap_or(false);

    let mut messages = Vec::new();
    for node in nodes {
        let comment_id = node["id"].as_str().unwrap_or("").to_string();
        let author = node["author"]["login"]
            .as_str()
            .unwrap_or("unknown")
            .to_string();
        let body = node["body"].as_str().unwrap_or("").to_string();
        let created_at = node["createdAt"].as_str().unwrap_or("");

        let timestamp = NaiveDateTime::parse_from_str(created_at, "%Y-%m-%dT%H:%M:%SZ")
            .or_else(|_| NaiveDateTime::parse_from_str(created_at, "%Y-%m-%dT%H:%M:%S%.fZ"))
            .unwrap_or_else(|_| chrono::Local::now().naive_local());

        let mut metadata = BTreeMap::from([("source".to_string(), "github".to_string())]);
        if !comment_id.is_empty() {
            metadata.insert("github_comment_id".to_string(), comment_id);
        }

        messages.push(Message {
            from: author,
            subject: String::new(),
            body,
            timestamp,
            metadata,
        });
    }

    Ok((messages, end_cursor, has_next))
}

pub fn parse_create_discussion_response(json: &serde_json::Value) -> Result<(String, u64)> {
    let discussion = &json["data"]["createDiscussion"]["discussion"];
    let id = discussion["id"]
        .as_str()
        .context("Missing discussion.id")?
        .to_string();
    let number = discussion["number"]
        .as_u64()
        .context("Missing discussion.number")?;
    Ok((id, number))
}

/// Enable GitHub Discussions on a repository via `gh repo edit`.
fn enable_discussions(owner: &str, repo: &str) -> Result<()> {
    let status = Command::new("gh")
        .args([
            "repo",
            "edit",
            &format!("{owner}/{repo}"),
            "--enable-discussions",
        ])
        .status()
        .context("Failed to run `gh repo edit`")?;
    if !status.success() {
        anyhow::bail!(
            "Failed to enable Discussions on {owner}/{repo}. Check repository permissions."
        );
    }
    Ok(())
}

/// Query repository node ID and discussion categories.
fn query_repo_and_categories(owner: &str, repo: &str) -> Result<(String, Vec<serde_json::Value>)> {
    let repo_query = format!(
        r#"{{ repository(owner: "{owner}", name: "{repo}") {{ id discussionCategories(first: 25) {{ nodes {{ id name }} }} }} }}"#
    );
    let repo_json = gh_graphql(&repo_query)?;

    let repo_node_id = repo_json["data"]["repository"]["id"]
        .as_str()
        .context("Failed to get repository node ID")?
        .to_string();

    let categories = repo_json["data"]["repository"]["discussionCategories"]["nodes"]
        .as_array()
        .context("Failed to get discussion categories")?
        .clone();

    Ok((repo_node_id, categories))
}

/// Create a new GitHub Discussion. Returns (node_id, number).
/// Automatically enables Discussions if not already enabled.
pub fn create_discussion(
    owner: &str,
    repo: &str,
    title: &str,
    body: &str,
) -> Result<(String, u64)> {
    let (mut repo_node_id, mut categories) = query_repo_and_categories(owner, repo)?;

    // If no categories, enable Discussions and retry after a brief delay for API propagation
    if categories.is_empty() {
        eprintln!("No discussion categories found. Enabling Discussions on {owner}/{repo}...");
        enable_discussions(owner, repo)?;
        std::thread::sleep(std::time::Duration::from_secs(2));
        let result = query_repo_and_categories(owner, repo)?;
        repo_node_id = result.0;
        categories = result.1;
    }

    let category_id = categories
        .iter()
        .find(|c| c["name"].as_str() == Some("General"))
        .or_else(|| categories.first())
        .and_then(|c| c["id"].as_str())
        .context("No discussion categories found even after enabling Discussions.")?;

    let mutation = build_create_discussion_mutation(&repo_node_id, category_id, title, body);
    let result = gh_graphql(&mutation)?;
    parse_create_discussion_response(&result)
}

/// Fetch new Discussion comments since cursor. Writes them as inbox files.
/// Comments authored by `skip_author` (if provided) are silently dropped
/// to prevent the bot from ingesting its own posts.
/// Returns the new cursor.
pub fn pull_comments(
    owner: &str,
    repo: &str,
    discussion_number: u64,
    last_cursor: Option<&str>,
    skip_author: Option<&str>,
    work_dir: &std::path::Path,
) -> Result<Option<String>> {
    crate::message::ensure_dirs(work_dir)?;
    let mut cursor = last_cursor.map(|s| s.to_string());

    loop {
        let query = build_fetch_comments_query(owner, repo, discussion_number, cursor.as_deref());
        let json = gh_graphql(&query)?;
        let (messages, new_cursor, has_next) = parse_discussion_comments(&json)?;

        for msg in &messages {
            if let Some(skip) = skip_author {
                if msg.from == skip {
                    continue;
                }
            }
            crate::message::write_message(work_dir, "inbox", msg)?;
        }

        if !new_cursor.is_empty() {
            cursor = Some(new_cursor);
        }

        if !has_next {
            break;
        }
    }

    Ok(cursor)
}

/// Post a comment to a Discussion.
pub fn post_comment(discussion_node_id: &str, body: &str) -> Result<()> {
    let mutation = build_post_comment_mutation(discussion_node_id, body);
    gh_graphql(&mutation)?;
    Ok(())
}
