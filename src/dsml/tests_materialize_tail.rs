//! DSML 物化相关单元测试（从 `text_sanitize` 迁入）。

use crate::dsml::materialize_deepseek_dsml_tool_calls_in_message;
use crate::types::{FunctionCall, Message, ToolCall};
use serde_json::Value;

#[test]
fn materialize_dsml_replaces_nameless_native_tool_call_placeholders() {
    let dsml = r#"说明文字。
<|DSML|function_calls>
<|DSML|invoke name="modify_file">
<|DSML|parameter name="path" string="true">1.md</|DSML|parameter>
<|DSML|parameter name="content" string="true"># Hi

Line2</|DSML|parameter>
</|DSML|invoke>
</|DSML|function_calls>"#;
    let mut msg = Message {
        role: "assistant".to_string(),
        content: Some(dsml.into()),
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: Some(vec![ToolCall {
            id: "stream_slot_0".to_string(),
            typ: "function".to_string(),
            function: FunctionCall {
                name: String::new(),
                arguments: String::new(),
            },
        }]),
        name: None,
        tool_call_id: None,
    };
    materialize_deepseek_dsml_tool_calls_in_message(&mut msg, true);
    let tcs = msg.tool_calls.as_ref().expect("tool_calls");
    assert_eq!(tcs.len(), 1);
    assert_eq!(tcs[0].function.name, "modify_file");
    let v: Value = serde_json::from_str(&tcs[0].function.arguments).unwrap();
    assert_eq!(v.get("path").and_then(|x| x.as_str()), Some("1.md"));
    assert!(
        v.get("content")
            .and_then(|x| x.as_str())
            .is_some_and(|s| s.contains("Line2"))
    );
}
#[test]
fn materialize_dsml_fullwidth_brackets_create_file_like_cli() {
    let dsml = "我们只需要创建 1.md。<｜DSML｜function_calls>\n\
<｜DSML｜invoke name=\"create_file\">\n\
<｜DSML｜parameter name=\"path\" string=\"true\">1.md</｜DSML｜parameter>\n\
<｜DSML｜parameter name=\"content\" string=\"true\"></｜DSML｜parameter>\n\
</｜DSML｜invoke>\n\
</｜DSML｜function_calls>";
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
    assert_eq!(tcs[0].function.name, "create_file");
    let v: Value = serde_json::from_str(&tcs[0].function.arguments).unwrap();
    assert_eq!(v.get("path").and_then(|x| x.as_str()), Some("1.md"));
    assert_eq!(v.get("content").and_then(|x| x.as_str()), Some(""));
    assert!(
        !crate::types::message_content_as_str(&msg.content)
            .unwrap_or("")
            .contains("DSML")
    );
}
#[test]
fn materialize_dsml_skipped_when_native_tool_call_has_name() {
    let dsml = r#"<|DSML|invoke name="modify_file">
<|DSML|parameter name="path">x.md</|DSML|parameter>
</|DSML|invoke>"#;
    let mut msg = Message {
        role: "assistant".to_string(),
        content: Some(dsml.into()),
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: Some(vec![ToolCall {
            id: "real".to_string(),
            typ: "function".to_string(),
            function: FunctionCall {
                name: "read_file".to_string(),
                arguments: "{}".to_string(),
            },
        }]),
        name: None,
        tool_call_id: None,
    };
    materialize_deepseek_dsml_tool_calls_in_message(&mut msg, true);
    let tcs = msg.tool_calls.as_ref().expect("tool_calls");
    assert_eq!(tcs[0].function.name, "read_file");
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

#[test]
fn materialize_dsml_double_fullwidth_pipe_tags_populate_tool_calls() {
    let dsml = "将执行。\n<｜｜DSML｜｜tool_calls>\n<｜｜DSML｜｜invoke name=\"run_command\">\n<｜｜DSML｜｜parameter name=\"command\">git log -1</｜｜DSML｜｜parameter>\n</｜｜DSML｜｜invoke>\n</｜｜DSML｜｜tool_calls>";
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
    assert_eq!(tcs[0].function.name, "run_command");
    let v: Value = serde_json::from_str(&tcs[0].function.arguments).unwrap();
    assert_eq!(
        v.get("command").and_then(|x| x.as_str()),
        Some("git log -1")
    );
    let prose = crate::types::message_content_as_str(&msg.content).unwrap_or("");
    assert!(prose.contains("将执行"));
    assert!(!prose.contains("DSML"));
}
