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

// --- Outbound attachment upload ---

#[test]
fn markdown_links_detects_image_syntax_bang() {
    let body = "text [a](/one) then ![b](/two)";
    let links = markdown_links(body);
    assert_eq!(links.len(), 2);
    assert_eq!(&body[links[0].span.clone()], "/one");
    assert_eq!(links[0].bang_at, None);
    assert_eq!(&body[links[1].span.clone()], "/two");
    assert_eq!(body.as_bytes()[links[1].bang_at.unwrap()], b'!');
}

#[test]
fn resolve_local_attachment_accepts_chamber_files_only() {
    let dir = tempfile::tempdir().unwrap();
    let attach = dir.path().join("messages/attachments");
    std::fs::create_dir_all(&attach).unwrap();
    std::fs::write(attach.join("qubit.png"), b"png").unwrap();

    assert!(resolve_local_attachment(dir.path(), "messages/attachments/qubit.png").is_some());
    // Not files / not local.
    assert!(resolve_local_attachment(dir.path(), "messages/attachments").is_none());
    assert!(resolve_local_attachment(dir.path(), "missing.png").is_none());
    assert!(resolve_local_attachment(dir.path(), "https://example.com/a.png").is_none());
    assert!(resolve_local_attachment(dir.path(), "/user_uploads/1/x/a.png").is_none());
    assert!(resolve_local_attachment(dir.path(), "").is_none());
}

#[test]
fn resolve_local_attachment_never_exposes_credentials_or_escapes_chamber() {
    let dir = tempfile::tempdir().unwrap();
    let cryo = dir.path().join(".cryo");
    std::fs::create_dir_all(&cryo).unwrap();
    std::fs::write(cryo.join("zuliprc"), b"[api]\nkey=secret\n").unwrap();
    let outside = dir.path().join("outside.txt");
    std::fs::write(&outside, b"nope").unwrap();
    let chamber = dir.path().join("chamber");
    std::fs::create_dir_all(&chamber).unwrap();

    // The bot's API key must never be uploadable.
    assert!(resolve_local_attachment(dir.path(), ".cryo/zuliprc").is_none());
    // Traversal out of the chamber is refused.
    assert!(resolve_local_attachment(&chamber, "../outside.txt").is_none());
}

#[test]
fn externalize_local_links_uploads_and_strips_image_bang() {
    let dir = tempfile::tempdir().unwrap();
    let attach = dir.path().join("messages/attachments");
    std::fs::create_dir_all(&attach).unwrap();
    std::fs::write(attach.join("qubit.png"), b"png").unwrap();

    let body = "画好了！\n\n![qubit](messages/attachments/qubit.png)\n\n源码在后面。";
    let mut uploads = 0;
    let (new_body, warnings) = externalize_local_links(body, dir.path(), SITE, |path| {
        uploads += 1;
        assert!(path.ends_with("qubit.png"));
        Ok("/user_uploads/2/b2/abc/qubit.png".to_string())
    });

    assert_eq!(uploads, 1);
    assert!(warnings.is_empty());
    // The `!` is gone (Zulip < 12 renders image syntax literally) and the
    // destination is absolute (relative URLs get no inline preview).
    assert_eq!(
        new_body,
        "画好了！\n\n[qubit](https://chat.example.com/user_uploads/2/b2/abc/qubit.png)\n\n源码在后面。"
    );
}

#[test]
fn externalize_local_links_strips_bang_on_preuploaded_paths() {
    // Reproduces the field bug: the agent uploaded the file itself and wrote
    // image syntax, which Zulip 11.4 rendered as literal text.
    let dir = tempfile::tempdir().unwrap();
    let body = "![qubit](/user_uploads/2/b2/Y5q/qubit.png)";
    let (new_body, warnings) = externalize_local_links(body, dir.path(), SITE, |_| {
        unreachable!("nothing to upload")
    });

    assert!(warnings.is_empty());
    assert_eq!(
        new_body,
        "[qubit](https://chat.example.com/user_uploads/2/b2/Y5q/qubit.png)"
    );
}

#[test]
fn externalize_local_links_uploads_each_file_once() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.png"), b"png").unwrap();

    let body = "[one](a.png) and again [two](a.png)";
    let mut uploads = 0;
    let (new_body, warnings) = externalize_local_links(body, dir.path(), SITE, |_| {
        uploads += 1;
        Ok("/user_uploads/1/x/a.png".to_string())
    });

    assert_eq!(uploads, 1, "same file must upload once");
    assert!(warnings.is_empty());
    assert_eq!(
        new_body,
        "[one](https://chat.example.com/user_uploads/1/x/a.png) and again [two](https://chat.example.com/user_uploads/1/x/a.png)"
    );
}

#[test]
fn externalize_local_links_keeps_message_when_upload_fails() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.png"), b"png").unwrap();

    let body = "see ![a](a.png)";
    let (new_body, warnings) =
        externalize_local_links(body, dir.path(), SITE, |_| anyhow::bail!("network down"));

    // Body untouched: the operator still gets the text, just no inline image.
    assert_eq!(new_body, body);
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("a.png"));
    assert!(warnings[0].contains("network down"));
}

#[test]
fn externalize_local_links_leaves_plain_messages_alone() {
    let dir = tempfile::tempdir().unwrap();
    let body = "no links here, and a [doc](https://example.com/page) too";
    let (new_body, warnings) =
        externalize_local_links(body, dir.path(), SITE, |_| unreachable!("nothing local"));
    assert_eq!(new_body, body);
    assert!(warnings.is_empty());
}

#[test]
fn multipart_boundary_avoids_colliding_with_payload() {
    let plain = multipart_boundary(b"some png bytes");
    assert!(!plain.is_empty());
    // A payload that literally contains the marker forces a longer boundary.
    let collide = multipart_boundary(plain.as_bytes());
    assert!(collide.len() > plain.len());
    assert!(!collide.contains("XX") || collide.starts_with(&plain));
}

#[test]
fn parse_upload_response_accepts_url_and_legacy_uri() {
    let new = serde_json::json!({"result": "success", "url": "/user_uploads/1/x/a.png"});
    assert_eq!(
        parse_upload_response(&new).unwrap(),
        "/user_uploads/1/x/a.png"
    );
    let old = serde_json::json!({"result": "success", "uri": "/user_uploads/2/y/b.png"});
    assert_eq!(
        parse_upload_response(&old).unwrap(),
        "/user_uploads/2/y/b.png"
    );
    let bad = serde_json::json!({"result": "success"});
    assert!(parse_upload_response(&bad).is_err());
}

#[test]
fn externalize_local_links_leaves_already_absolute_uploads_but_strips_bang() {
    // Exactly the field bug: agent uploaded the file itself and wrote image
    // syntax with an absolute URL. Only the `!` should change.
    let dir = tempfile::tempdir().unwrap();
    let body = "![qubit](https://chat.example.com/user_uploads/2/b2/Y5q/qubit.png)";
    let (new_body, warnings) = externalize_local_links(body, dir.path(), SITE, |_| {
        unreachable!("nothing to upload")
    });
    assert!(warnings.is_empty());
    assert_eq!(
        new_body,
        "[qubit](https://chat.example.com/user_uploads/2/b2/Y5q/qubit.png)"
    );
}

#[test]
fn externalize_local_links_ignores_other_links_on_same_site() {
    let dir = tempfile::tempdir().unwrap();
    let body = "see [help](https://chat.example.com/help/topic)";
    let (new_body, warnings) = externalize_local_links(body, dir.path(), SITE, |_| {
        unreachable!("nothing to upload")
    });
    assert_eq!(new_body, body);
    assert!(warnings.is_empty());
}

#[test]
fn externalize_local_links_is_idempotent_and_utf8_safe() {
    let dir = tempfile::tempdir().unwrap();
    let attach = dir.path().join("messages/attachments");
    std::fs::create_dir_all(&attach).unwrap();
    std::fs::write(attach.join("t.png"), b"png").unwrap();
    // Multi-byte text on both sides of the link exercises span arithmetic.
    let body = "画好了！这是图：![图](messages/attachments/t.png)，源码在后面。";
    let (once, w1) = externalize_local_links(body, dir.path(), SITE, |_| {
        Ok("/user_uploads/1/x/t.png".to_string())
    });
    assert!(w1.is_empty());
    let (twice, w2) = externalize_local_links(&once, dir.path(), SITE, |_| {
        panic!("second pass must not upload again")
    });
    assert!(w2.is_empty());
    assert_eq!(once, twice, "rewriting must be idempotent");
    assert!(once.contains("画好了！这是图："));
    assert!(once.contains("，源码在后面。"));
    assert!(!once.contains("!["));
}

#[test]
fn resolve_local_attachment_refuses_symlink_escape() {
    // A symlink inside the chamber pointing at the credentials file (or
    // anywhere outside) must not become uploadable.
    let dir = tempfile::tempdir().unwrap();
    let chamber = dir.path().join("chamber");
    std::fs::create_dir_all(chamber.join(".cryo")).unwrap();
    std::fs::write(chamber.join(".cryo/zuliprc"), b"[api]\nkey=secret\n").unwrap();
    let secret = dir.path().join("outside-secret.txt");
    std::fs::write(&secret, b"top secret").unwrap();

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&secret, chamber.join("link-out.txt")).unwrap();
        std::os::unix::fs::symlink(chamber.join(".cryo/zuliprc"), chamber.join("link-rc")).unwrap();
        assert!(
            resolve_local_attachment(&chamber, "link-out.txt").is_none(),
            "symlink out of the chamber must be refused"
        );
        assert!(
            resolve_local_attachment(&chamber, "link-rc").is_none(),
            "symlink to the bot API key must be refused"
        );
    }
}
