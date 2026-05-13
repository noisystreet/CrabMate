//! DeepSeek DSML 变体归一化（双竖线 `<｜｜DSML｜｜` 等）：从 `text_sanitize` 拆出以降低物理行数棘轮；供物化与展示剥离调用。
pub(crate) fn normalize_deepseek_dsml_vendor_variants(s: &str) -> String {
    s.replace("<｜｜DSML｜｜", "<｜DSML｜")
        .replace("</｜｜DSML｜｜", "</｜DSML｜")
        .replace("<||DSML||", "<|DSML|")
        .replace("</||DSML||", "</|DSML|")
}

#[cfg(test)]
mod tests {
    use crate::text_sanitize::materialize_deepseek_dsml_tool_calls_in_message;
    use crate::types::Message;
    use serde_json::Value;

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
}
