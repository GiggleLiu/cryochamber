use cryochamber::channel::github::{
    build_create_discussion_mutation, build_fetch_comments_query, build_post_comment_mutation,
    fetch_comments, parse_create_discussion_response, parse_discussion_comments,
};
use std::ffi::OsString;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::sync::Mutex;

static ENV_LOCK: Mutex<()> = Mutex::new(());

struct EnvGuard {
    key: &'static str,
    original: Option<OsString>,
}

impl EnvGuard {
    fn set_path_prefix(prefix: &std::path::Path) -> Self {
        let key = "PATH";
        let original = std::env::var_os(key);
        let mut paths = vec![prefix.to_path_buf()];
        if let Some(current) = original.as_ref() {
            paths.extend(std::env::split_paths(current));
        }
        std::env::set_var(key, std::env::join_paths(paths).unwrap());
        Self { key, original }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match self.original.as_ref() {
            Some(value) => std::env::set_var(self.key, value),
            None => std::env::remove_var(self.key),
        }
    }
}

fn make_gh_stub(dir: &std::path::Path, stdout: &str) {
    let path = dir.join("gh");
    let mut file = std::fs::File::create(&path).unwrap();
    writeln!(file, "#!/bin/sh").unwrap();
    writeln!(file, "cat <<'JSON'").unwrap();
    writeln!(file, "{stdout}").unwrap();
    writeln!(file, "JSON").unwrap();
    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&path, perms).unwrap();
}

#[test]
fn test_build_fetch_comments_query() {
    let query = build_fetch_comments_query("owner", "repo", 42, None);
    assert!(query.contains("owner"));
    assert!(query.contains("repo"));
    assert!(query.contains("42"));
    // No cursor — should not contain "after"
    assert!(!query.contains("after:"));
}

#[test]
fn test_build_fetch_comments_query_with_cursor() {
    let query = build_fetch_comments_query("owner", "repo", 42, Some("abc123"));
    assert!(query.contains("after:"));
    assert!(query.contains("abc123"));
}

#[test]
fn test_parse_discussion_comments() {
    let json = serde_json::json!({
        "data": {
            "repository": {
                "discussion": {
                    "comments": {
                        "nodes": [
                            {
                                "id": "DC_1",
                                "body": "Please update the config",
                                "author": { "login": "alice" },
                                "createdAt": "2026-02-23T10:30:00Z"
                            }
                        ],
                        "pageInfo": {
                            "endCursor": "cursor_abc",
                            "hasNextPage": false
                        }
                    }
                }
            }
        }
    });
    let (messages, cursor, has_next) = parse_discussion_comments(&json).unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].from, "alice");
    assert!(messages[0].body.contains("update the config"));
    assert_eq!(
        messages[0].metadata.get("github_comment_id"),
        Some(&"DC_1".to_string())
    );
    assert_eq!(cursor, "cursor_abc");
    assert!(!has_next);
}

#[test]
fn test_parse_discussion_comments_empty() {
    let json = serde_json::json!({
        "data": {
            "repository": {
                "discussion": {
                    "comments": {
                        "nodes": [],
                        "pageInfo": {
                            "endCursor": null,
                            "hasNextPage": false
                        }
                    }
                }
            }
        }
    });
    let (messages, cursor, _) = parse_discussion_comments(&json).unwrap();
    assert!(messages.is_empty());
    assert!(cursor.is_empty());
}

#[test]
fn test_github_fetch_comments_returns_messages_and_cursor_without_work_dir() {
    let _guard = ENV_LOCK.lock().unwrap();
    let bin = tempfile::tempdir().unwrap();
    make_gh_stub(
        bin.path(),
        r#"{
  "data": {
    "repository": {
      "discussion": {
        "comments": {
          "nodes": [
            {
              "id": "DC_1",
              "body": "Please update the config",
              "author": { "login": "alice" },
              "createdAt": "2026-02-23T10:30:00Z"
            },
            {
              "id": "DC_2",
              "body": "Bot echo",
              "author": { "login": "mybot" },
              "createdAt": "2026-02-23T10:31:00Z"
            }
          ],
          "pageInfo": {
            "endCursor": "cursor_abc",
            "hasNextPage": false
          }
        }
      }
    }
  }
}"#,
    );
    let _path = EnvGuard::set_path_prefix(bin.path());

    let result = fetch_comments("owner", "repo", 42, None, Some("mybot")).unwrap();

    assert_eq!(result.cursor, Some("cursor_abc".to_string()));
    assert_eq!(result.messages.len(), 1);
    assert_eq!(result.messages[0].from, "alice");
    assert_eq!(result.messages[0].body, "Please update the config");
}

#[test]
fn test_github_channel_does_not_write_local_inbox_files() {
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/channel/github.rs"),
    )
    .unwrap();
    assert!(
        !source.contains("crate::message::ensure_dirs")
            && !source.contains("crate::message::write_message"),
        "channel::github must stay transport-only; inbox persistence belongs in cryo_gh"
    );
}

#[test]
fn test_build_post_comment_mutation() {
    let mutation = build_post_comment_mutation("D_kwDOtest", "Hello from cryo");
    assert!(mutation.contains("D_kwDOtest"));
    assert!(mutation.contains("Hello from cryo"));
    assert!(mutation.contains("addDiscussionComment"));
}

#[test]
fn test_build_post_comment_escapes_special_chars() {
    let mutation = build_post_comment_mutation("D_test", "Line 1\nLine 2 with \"quotes\"");
    assert!(mutation.contains("\\n"));
    assert!(mutation.contains("\\\"quotes\\\""));
}

#[test]
fn test_build_post_comment_escapes_crlf() {
    let mutation = build_post_comment_mutation("D_test", "Line 1\r\nLine 2\ttab");
    assert!(mutation.contains("\\r"));
    assert!(mutation.contains("\\n"));
    assert!(mutation.contains("\\t"));
}

#[test]
fn test_build_create_discussion_mutation() {
    let mutation =
        build_create_discussion_mutation("R_abc", "DC_xyz", "My Title", "Line 1\nLine 2");
    assert!(mutation.contains("R_abc"));
    assert!(mutation.contains("DC_xyz"));
    assert!(mutation.contains("My Title"));
    assert!(mutation.contains("\\n")); // newline escaped
    assert!(mutation.contains("createDiscussion"));
}

#[test]
fn test_build_create_discussion_mutation_escapes_title() {
    let mutation =
        build_create_discussion_mutation("R_abc", "DC_xyz", "Title with \"quotes\"", "body");
    assert!(mutation.contains("\\\"quotes\\\""));
}

#[test]
fn test_parse_create_discussion_response() {
    let json = serde_json::json!({
        "data": {
            "createDiscussion": {
                "discussion": {
                    "id": "D_kwDOtest",
                    "number": 42
                }
            }
        }
    });
    let (node_id, number) = parse_create_discussion_response(&json).unwrap();
    assert_eq!(node_id, "D_kwDOtest");
    assert_eq!(number, 42);
}
