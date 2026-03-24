use super::*;

#[test]
fn normalize_conversation_id_accepts_expected_charset() {
    let got =
        normalize_client_conversation_id(Some("conv_abc-123:xyz.test")).expect("id should parse");
    assert_eq!(got.as_deref(), Some("conv_abc-123:xyz.test"));
}

#[test]
fn normalize_conversation_id_rejects_invalid_chars() {
    let err = normalize_client_conversation_id(Some("abc/def")).expect_err("should reject /");
    assert!(err.contains("仅允许"));
}
