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
