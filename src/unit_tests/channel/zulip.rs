use super::*;

#[test]
fn append_base64_tail_handles_empty_tail() {
    let mut result = Vec::new();
    append_base64_tail(&mut result, b"");

    assert_eq!(String::from_utf8(result).unwrap(), "");
}

#[test]
fn append_base64_tail_handles_single_byte_padding() {
    let mut result = Vec::new();
    append_base64_tail(&mut result, b"f");

    assert_eq!(String::from_utf8(result).unwrap(), "Zg==");
}

#[test]
fn append_base64_tail_handles_two_byte_padding() {
    let mut result = Vec::new();
    append_base64_tail(&mut result, b"fo");

    assert_eq!(String::from_utf8(result).unwrap(), "Zm8=");
}

#[test]
fn build_agent_sets_bounded_timeouts() {
    // A stalled connection must not hang the single-threaded sync daemon;
    // the agent must carry a global (and connect) timeout, not the ureq
    // default of `None`.
    let agent = build_agent();
    let timeouts = agent.config().timeouts();
    assert_eq!(timeouts.global, Some(HTTP_GLOBAL_TIMEOUT));
    assert_eq!(timeouts.connect, Some(HTTP_CONNECT_TIMEOUT));
}

// --- Attachment localization ---

const SITE: &str = "https://chat.example.com";

fn client_for_test() -> ZulipClient {
    let dir = tempfile::tempdir().unwrap();
    let rc = dir.path().join("zuliprc");
    std::fs::write(
        &rc,
        "[api]\nemail=bot@example.com\nkey=secret\nsite=https://chat.example.com\n",
    )
    .unwrap();
    ZulipClient::from_zuliprc(&rc).unwrap()
}

#[test]
fn from_zuliprc_parses_credentials() {
    let client = client_for_test();
    assert_eq!(client.credentials().email, "bot@example.com");
    assert_eq!(client.credentials().api_key, "secret");
    assert_eq!(client.credentials().site, "https://chat.example.com");
}

#[test]
fn extract_upload_links_finds_relative_target() {
    let body = "look at this\n[plant.jpg](/user_uploads/2/ab/xyz/plant.jpg)";
    let links = extract_upload_links(body, SITE);
    assert_eq!(links.len(), 1);
    assert_eq!(
        &body[links[0].span.clone()],
        "/user_uploads/2/ab/xyz/plant.jpg"
    );
    assert_eq!(links[0].server_path, "/user_uploads/2/ab/xyz/plant.jpg");
    assert_eq!(links[0].filename, "plant.jpg");
}

#[test]
fn extract_upload_links_finds_absolute_target_on_site() {
    let body = "[f.png](https://chat.example.com/user_uploads/1/aa/bb/f.png)";
    let links = extract_upload_links(body, SITE);
    assert_eq!(links.len(), 1);
    assert_eq!(
        &body[links[0].span.clone()],
        "https://chat.example.com/user_uploads/1/aa/bb/f.png"
    );
    assert_eq!(links[0].server_path, "/user_uploads/1/aa/bb/f.png");
}

#[test]
fn extract_upload_links_handles_balanced_parens_in_destination() {
    let body = "[shot](/user_uploads/1/x/screen(1).png)";
    let links = extract_upload_links(body, SITE);
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].server_path, "/user_uploads/1/x/screen(1).png");
    assert_eq!(links[0].filename, "screen_1_.png");
}

#[test]
fn extract_upload_links_excludes_title_from_span() {
    let body = "[a](/user_uploads/1/x/f.png \"my photo\")";
    let links = extract_upload_links(body, SITE);
    assert_eq!(links.len(), 1);
    assert_eq!(&body[links[0].span.clone()], "/user_uploads/1/x/f.png");
    assert_eq!(links[0].server_path, "/user_uploads/1/x/f.png");
}

#[test]
fn extract_upload_links_accepts_site_with_trailing_slash() {
    let body = "[f.png](https://chat.example.com/user_uploads/1/aa/bb/f.png)";
    let links = extract_upload_links(body, "https://chat.example.com/");
    assert_eq!(links.len(), 1);
}

#[test]
fn extract_upload_links_ignores_non_upload_links() {
    let body = "see [docs](https://example.org/page) and [x](/api/v1/thing)";
    assert!(extract_upload_links(body, SITE).is_empty());
    // Same host, but not an upload path.
    let body = "[page](https://chat.example.com/api/v1/thing)";
    assert!(extract_upload_links(body, SITE).is_empty());
    assert!(extract_upload_links("no links at all", SITE).is_empty());
}

#[test]
fn extract_upload_links_ignores_uploads_on_other_hosts() {
    let body = "[f](https://evil.example.net/user_uploads/1/aa/bb/f.png)";
    assert!(extract_upload_links(body, SITE).is_empty());
}

#[test]
fn extract_upload_links_returns_every_occurrence_in_order() {
    let body = "[a](/user_uploads/1/x/a.png) [b](/user_uploads/1/x/b.png) [a again](/user_uploads/1/x/a.png)";
    let links = extract_upload_links(body, SITE);
    assert_eq!(links.len(), 3);
    assert_eq!(links[0].filename, "a.png");
    assert_eq!(links[1].filename, "b.png");
    assert_eq!(links[2].filename, "a.png");
}

#[test]
fn localize_upload_links_downloads_duplicate_target_once() {
    let dir = tempfile::tempdir().unwrap();
    let body = "[a](/user_uploads/1/x/a.png) and [a again](/user_uploads/1/x/a.png)";
    let mut fetch_calls = 0;
    let (new_body, warnings) = localize_upload_links(body, SITE, "5", dir.path(), |_| {
        fetch_calls += 1;
        Ok(vec![1])
    });
    assert_eq!(fetch_calls, 1, "same server path must be fetched once");
    assert!(warnings.is_empty());
    assert_eq!(
        new_body,
        "[a](messages/attachments/5-0_a.png) and [a again](messages/attachments/5-0_a.png)"
    );
}

#[test]
fn localize_upload_links_rewrites_only_link_destinations() {
    let dir = tempfile::tempdir().unwrap();
    // The same text outside a markdown destination must stay untouched.
    let body = "the url (/user_uploads/1/x/a.png) in prose, [a](/user_uploads/1/x/a.png)";
    let (new_body, warnings) = localize_upload_links(body, SITE, "5", dir.path(), |_| Ok(vec![1]));
    assert!(warnings.is_empty());
    assert_eq!(
        new_body,
        "the url (/user_uploads/1/x/a.png) in prose, [a](messages/attachments/5-0_a.png)"
    );
}

#[test]
fn extract_upload_links_survives_unclosed_link() {
    let body = "broken [a](/user_uploads/1/x/a.png then nothing";
    assert!(extract_upload_links(body, SITE).is_empty());
}

#[test]
fn extract_upload_links_sanitizes_hostile_filenames() {
    let body = "[weird](/user_uploads/1/x/na%20me$.jpg)";
    let links = extract_upload_links(body, SITE);
    assert_eq!(links[0].filename, "na_20me_.jpg");

    // Trailing slash yields an empty segment; dots-only names are unusable.
    let body = "[empty](/user_uploads/1/x/)";
    let links = extract_upload_links(body, SITE);
    assert_eq!(links[0].filename, "file");

    let body = "[dots](/user_uploads/1/x/...)";
    let links = extract_upload_links(body, SITE);
    assert_eq!(links[0].filename, "file");
}

#[test]
fn localize_upload_links_downloads_and_rewrites() {
    let dir = tempfile::tempdir().unwrap();
    let body = "what is this?\n[plant.jpg](/user_uploads/2/ab/xyz/plant.jpg)";
    let (new_body, warnings) = localize_upload_links(body, SITE, "42", dir.path(), |path| {
        assert_eq!(path, "/user_uploads/2/ab/xyz/plant.jpg");
        Ok(vec![1, 2, 3])
    });
    assert!(warnings.is_empty());
    assert_eq!(
        new_body,
        "what is this?\n[plant.jpg](messages/attachments/42-0_plant.jpg)"
    );
    let saved = dir.path().join("messages/attachments/42-0_plant.jpg");
    assert_eq!(std::fs::read(saved).unwrap(), vec![1, 2, 3]);
}

#[test]
fn localize_upload_links_leaves_link_on_fetch_failure() {
    let dir = tempfile::tempdir().unwrap();
    let body = "[plant.jpg](/user_uploads/2/ab/xyz/plant.jpg)";
    let (new_body, warnings) =
        localize_upload_links(body, SITE, "42", dir.path(), |_| anyhow::bail!("boom"));
    assert_eq!(new_body, body);
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("/user_uploads/2/ab/xyz/plant.jpg"));
    assert!(warnings[0].contains("boom"));
    assert!(!dir.path().join("messages/attachments").exists());
}

#[test]
fn localize_upload_links_skips_existing_files() {
    let dir = tempfile::tempdir().unwrap();
    let attach = dir.path().join("messages/attachments");
    std::fs::create_dir_all(&attach).unwrap();
    std::fs::write(attach.join("42-0_plant.jpg"), b"old").unwrap();

    let body = "[plant.jpg](/user_uploads/2/ab/xyz/plant.jpg)";
    let mut fetch_calls = 0;
    let (new_body, warnings) = localize_upload_links(body, SITE, "42", dir.path(), |_| {
        fetch_calls += 1;
        Ok(vec![9])
    });
    assert_eq!(fetch_calls, 0, "existing file must not be re-downloaded");
    assert!(warnings.is_empty());
    assert_eq!(new_body, "[plant.jpg](messages/attachments/42-0_plant.jpg)");
    assert_eq!(
        std::fs::read(attach.join("42-0_plant.jpg")).unwrap(),
        b"old"
    );
}

#[test]
fn localize_upload_links_gives_same_filename_distinct_indices() {
    let dir = tempfile::tempdir().unwrap();
    let body = "[a](/user_uploads/1/x/image.png) [b](/user_uploads/2/y/image.png)";
    let (new_body, warnings) = localize_upload_links(body, SITE, "7", dir.path(), |path| {
        Ok(path.as_bytes().to_vec())
    });
    assert!(warnings.is_empty());
    assert_eq!(
        new_body,
        "[a](messages/attachments/7-0_image.png) [b](messages/attachments/7-1_image.png)"
    );
    assert_eq!(
        std::fs::read(dir.path().join("messages/attachments/7-0_image.png")).unwrap(),
        b"/user_uploads/1/x/image.png"
    );
}

#[test]
fn localize_upload_links_without_links_touches_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (new_body, warnings) =
        localize_upload_links("plain text", SITE, "1", dir.path(), |_| unreachable!());
    assert_eq!(new_body, "plain text");
    assert!(warnings.is_empty());
    assert!(!dir.path().join("messages").exists());
}

#[test]
fn download_upload_rejects_non_upload_paths() {
    let client = client_for_test();
    let err = client.download_upload("/etc/passwd").unwrap_err();
    assert!(err.to_string().contains("not a user_uploads path"));
}

#[test]
fn download_upload_rejects_unsafe_paths() {
    // The request carries the bot's API key; a crafted link must not steer
    // it outside /user_uploads/.
    let client = client_for_test();
    for path in [
        "/user_uploads/../api/v1/users/me",
        "/user_uploads/./x/f.png",
        "/user_uploads/1/x/f.png?evil=1",
        "/user_uploads/1/x/f.png#frag",
    ] {
        let err = client.download_upload(path).unwrap_err();
        assert!(
            err.to_string().contains("unsafe user_uploads path"),
            "path {path} must be rejected"
        );
    }
}
