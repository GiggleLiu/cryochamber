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
