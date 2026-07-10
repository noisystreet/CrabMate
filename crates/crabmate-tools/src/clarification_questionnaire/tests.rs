use super::*;

#[test]
fn merge_prepends_block_after_text() {
    let m = serde_json::json!({"a": "x", "b": "y"});
    let obj = m.as_object().unwrap().clone();
    let out = merge_user_text_with_clarification_answers("hello".into(), Some(("q1".into(), obj)));
    assert!(out.starts_with("hello"));
    assert!(out.contains("questionnaire_id=q1"));
    assert!(out.contains("`a`: x"));
    assert!(out.contains("`b`: y"));
}
