use super::{
    dedupe_plain_assistant_preamble, materialize_deepseek_dsml_tool_calls_in_message,
    naturalize_assistant_plan_prose_tail, naturalize_plan_step_description,
    strip_deepseek_dsml_for_display,
};
use crate::types::Message;
use serde_json::Value;

#[test]
fn naturalize_step_extracts_json_description() {
    let raw = r#"{"id":"a","description":"读取配置并汇总"}"#;
    assert_eq!(naturalize_plan_step_description(raw), "读取配置并汇总");
}
#[test]
fn naturalize_step_flattens_markdown_list() {
    let s = "- 查日志\n- 修配置";
    assert_eq!(naturalize_plan_step_description(s), "查日志；修配置");
}
#[test]
fn strips_tool_calls_dsml_double_fullwidth_pipe() {
    let s = "说明。\n<｜｜DSML｜｜tool_calls>\n<｜｜DSML｜｜invoke name=\"run_command\">\n</｜｜DSML｜｜invoke>\n</｜｜DSML｜｜tool_calls>\n尾部";
    let t = strip_deepseek_dsml_for_display(s);
    assert!(!t.contains("DSML"));
    assert!(t.contains("说明"));
    assert!(t.contains("尾部"));
}

#[test]
fn strips_nested_dsml_fullwidth() {
    let s = "前言<｜DSML｜function_calls><｜DSML｜invoke name=\"f\"><｜DSML｜parameter name=\"x\" string=\"true\">v</｜DSML｜parameter></｜DSML｜invoke></｜DSML｜function_calls>后记";
    let t = strip_deepseek_dsml_for_display(s);
    assert!(!t.contains("DSML"));
    assert!(t.contains("前言"));
    assert!(t.contains("后记"));
}
#[test]
fn strips_ascii_pipe_variant() {
    let s = include_str!("testdata/strips_ascii_pipe_variant_input.txt").trim_end();
    let t = strip_deepseek_dsml_for_display(s);
    assert!(!t.contains("DSML"));
    assert!(t.contains('a'));
    assert!(t.contains('b'));
}
#[test]
fn noop_without_dsml() {
    let s = "普通中文与 English\n第二行";
    assert_eq!(strip_deepseek_dsml_for_display(s), s);
}
#[test]
fn naturalize_plan_prose_dedupes_adjacent_identical_lines() {
    let line = "我将帮您编写 Hello World，并先规划任务步骤：";
    let raw = format!("{line}\n{line}");
    assert_eq!(naturalize_assistant_plan_prose_tail(&raw), line);
}
#[test]
fn naturalize_plan_prose_dedupes_fullwidth_colon_variant() {
    let a = "我将帮您编写步骤：";
    let b = "我将帮您编写步骤:"; // ASCII colon
    let raw = format!("{a}\n{b}");
    assert_eq!(naturalize_assistant_plan_prose_tail(&raw), a);
}
#[test]
fn naturalize_plan_prose_dedupes_terminal_punctuation_variant() {
    let a = "我将先拆解任务步骤：";
    let b = "我将先拆解任务步骤。";
    let raw = format!("{a}\n{b}");
    assert_eq!(naturalize_assistant_plan_prose_tail(&raw), a);
}
#[test]
fn dedupe_plain_preamble_collapses_space_joined_duplicate() {
    let line = "我将帮您编写 Hello World 并规划步骤。";
    let raw = format!("{line} {line}");
    assert_eq!(dedupe_plain_assistant_preamble(&raw), line);
}

#[test]
fn materialize_dsml_populates_tool_calls_and_strips_markup() {
    let dsml = r#"将更新文件。
<|DSML|function_calls>
<|DSML|invoke name="modify_file">
<|DSML|parameter name="path">1.md</|DSML|parameter>
<|DSML|parameter name="content"># 标题</|DSML|parameter>
</|DSML|invoke>
</|DSML|function_calls>"#;
    let mut msg = Message {
        role: "assistant".to_string(),
        content: Some(dsml.into()),
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: None,
        name: None,
        tool_call_id: None,
    };
    materialize_deepseek_dsml_tool_calls_in_message(&mut msg, true);
    let tcs = msg.tool_calls.as_ref().expect("tool_calls");
    assert_eq!(tcs.len(), 1);
    assert_eq!(tcs[0].function.name, "modify_file");
    let v: Value = serde_json::from_str(&tcs[0].function.arguments).unwrap();
    assert_eq!(v.get("path").and_then(|x| x.as_str()), Some("1.md"));
    assert_eq!(v.get("content").and_then(|x| x.as_str()), Some("# 标题"));
    let prose = crate::types::message_content_as_str(&msg.content).unwrap_or("");
    assert!(prose.contains("将更新"));
    assert!(!prose.contains("DSML"));
}
#[test]
fn materialize_dsml_skipped_when_disabled() {
    let dsml = r#"<|DSML|invoke name="read_file">
<|DSML|parameter name="path">x.txt</|DSML|parameter>
</|DSML|invoke>"#;
    let mut msg = Message {
        role: "assistant".to_string(),
        content: Some(dsml.into()),
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: None,
        name: None,
        tool_call_id: None,
    };
    materialize_deepseek_dsml_tool_calls_in_message(&mut msg, false);
    assert!(msg.tool_calls.is_none());
    assert!(
        crate::types::message_content_as_str(&msg.content)
            .unwrap_or("")
            .contains("DSML")
    );
}
#[test]
fn materialize_dsml_spaced_tags_and_multiline_parameter() {
    let dsml = r#"将写入。
< | DSML | invoke name="modify_file" >
<|DSML|parameter name="path">note.md</|DSML|parameter>
<|DSML|parameter name="content"># 标题
第二行</|DSML|parameter>
</|DSML|invoke>"#;
    let mut msg = Message {
        role: "assistant".to_string(),
        content: Some(dsml.into()),
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: None,
        name: None,
        tool_call_id: None,
    };
    materialize_deepseek_dsml_tool_calls_in_message(&mut msg, true);
    let tcs = msg.tool_calls.as_ref().expect("tool_calls");
    assert_eq!(tcs.len(), 1);
    assert_eq!(tcs[0].function.name, "modify_file");
    let v: Value = serde_json::from_str(&tcs[0].function.arguments).unwrap();
    assert_eq!(v.get("path").and_then(|x| x.as_str()), Some("note.md"));
    assert!(
        v.get("content")
            .and_then(|x| x.as_str())
            .is_some_and(|s| s.contains("第二行"))
    );
}
#[test]
fn materialize_dsml_single_quoted_names() {
    let dsml = r#"<|DSML|invoke name='read_file'>
<|DSML|parameter name='path'>x.txt</|DSML|parameter>
</|DSML|invoke>"#;
    let mut msg = Message {
        role: "assistant".to_string(),
        content: Some(dsml.into()),
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: None,
        name: None,
        tool_call_id: None,
    };
    materialize_deepseek_dsml_tool_calls_in_message(&mut msg, true);
    let tcs = msg.tool_calls.as_ref().expect("tool_calls");
    assert_eq!(tcs[0].function.name, "read_file");
}
#[test]
fn materialize_dsml_json_array_parameter_for_run_command_args() {
    let dsml = r#"让我用 cat。
<|DSML|function_calls>
<|DSML|invoke name="run_command">
<|DSML|parameter name="command" string="true">cat</|DSML|parameter>
<|DSML|parameter name="args" string="true">["1.md"]</|DSML|parameter>
</|DSML|invoke>
</|DSML|function_calls>"#;
    let mut msg = Message {
        role: "assistant".to_string(),
        content: Some(dsml.into()),
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: None,
        name: None,
        tool_call_id: None,
    };
    materialize_deepseek_dsml_tool_calls_in_message(&mut msg, true);
    let tcs = msg.tool_calls.as_ref().expect("tool_calls");
    assert_eq!(tcs[0].function.name, "run_command");
    let v: Value = serde_json::from_str(&tcs[0].function.arguments).unwrap();
    assert_eq!(v.get("command").and_then(|x| x.as_str()), Some("cat"));
    let args = v
        .get("args")
        .and_then(|x| x.as_array())
        .expect("args array");
    assert_eq!(args.len(), 1);
    assert_eq!(args[0].as_str(), Some("1.md"));
}
#[test]
fn materialize_dsml_from_reasoning_when_content_empty() {
    let dsml = r#"<|DSML|invoke name="read_file">
<|DSML|parameter name="path">z.txt</|DSML|parameter>
</|DSML|invoke>"#;
    let mut msg = Message {
        role: "assistant".to_string(),
        content: None,
        reasoning_content: Some(dsml.to_string()),
        reasoning_details: None,
        tool_calls: None,
        name: None,
        tool_call_id: None,
    };
    materialize_deepseek_dsml_tool_calls_in_message(&mut msg, true);
    let tcs = msg.tool_calls.as_ref().expect("tool_calls");
    assert_eq!(tcs[0].function.name, "read_file");
    assert!(
        msg.reasoning_content
            .as_deref()
            .unwrap_or("")
            .trim()
            .is_empty()
    );
}
