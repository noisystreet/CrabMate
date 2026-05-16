//! DSML 物化相关单元测试（从主 `tests.rs` 拆出）：`r#"` 内若含 `# …` 等片段，部分静态分析器会误解析后续 `#[test]`，故单独成文件。

use super::materialize_deepseek_dsml_tool_calls_in_message;
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
    // 与部分模型在规划轮输出的全角 `｜` DSML 一致（分阶段路径须先物化再执行工具）。
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
