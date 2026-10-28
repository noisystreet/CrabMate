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

/// 迁 `Typed` 后仍要守的两条契约：
/// 1. 成功正文首行必须是 `退出码：0` —— `clarification_sse_bridge` 靠它判断
///    「这是一份登记成功的问卷」，换掉会让 Web SSE 控制面静默失效；
/// 2. 失败是显式 `invalid_args`，不再靠「`退出码：1` + 启发式」让人去猜。
#[test]
fn present_runner_keeps_exit_zero_header_and_reports_invalid_args() {
    let ok = run_present_clarification_questionnaire(
        r#"{"questionnaire_id":"q1","intro":"需要确认","questions":[{"id":"a","label":"A?"}]}"#,
    )
    .expect("合法问卷应成功");
    assert!(ok.starts_with("退出码：0"), "{ok}");
    assert!(ok.contains("`q1`"), "{ok}");

    // 缺必填字段（`{}`）：以前要靠「退出码：1」兜底才被判失败。
    let e = run_present_clarification_questionnaire("{}").expect_err("缺字段应失败");
    assert_eq!(e.code, "invalid_args");
    assert!(e.message.contains("错误："), "{}", e.message);

    // 语义校验（intro 全空白）同样归 invalid_args。
    let e = run_present_clarification_questionnaire(
        r#"{"questionnaire_id":"q1","intro":"   ","questions":[{"id":"a","label":"A?"}]}"#,
    )
    .expect_err("空 intro 应失败");
    assert_eq!(e.code, "invalid_args");
}
