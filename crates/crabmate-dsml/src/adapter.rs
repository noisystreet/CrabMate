//! 将 DSML 解析结果写入 [`crabmate_types::Message`]（执行轮物化入口）。

use log::debug;

use crabmate_types::{FunctionCall, Message, MessageContent, ToolCall};
use serde_json::Value;

use crate::parser::{parse_combined_assistant_text, text_looks_like_dsml};
use crate::strip::strip_deepseek_dsml_for_display;
use crate::types::DsmlMaterializePolicy;

/// 流式聚合等场景下 API 可能留下 **`function.name` 全空** 的占位 `tool_calls`。
fn has_usable_native_tool_calls(tcs: &[ToolCall]) -> bool {
    tcs.iter().any(|tc| !tc.function.name.trim().is_empty())
}

fn json_value_looks_like_tool_args(v: &Value) -> bool {
    match v {
        Value::Array(a) => {
            !a.is_empty()
                && a.iter()
                    .all(|x| x.is_string() || x.is_number() || x.is_boolean())
        }
        Value::Object(o) => o.keys().any(|k| {
            matches!(
                k.as_str(),
                "command" | "args" | "path" | "content" | "cmd" | "pattern" | "file" | "files"
            )
        }),
        _ => false,
    }
}

fn looks_like_tool_argument_residue(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() {
        return true;
    }
    serde_json::from_str::<Value>(t).is_ok_and(|v| json_value_looks_like_tool_args(&v))
}

/// 单一适配入口：解析策略 + 写回助手消息。
#[derive(Debug, Clone, Copy, Default)]
pub struct DsmlToolCallAdapter;

impl DsmlToolCallAdapter {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// 与历史 **`materialize_deepseek_dsml_tool_calls_in_message(msg, enabled)`** 行为对齐。
    pub fn apply_to_assistant_message(&self, msg: &mut Message, policy: DsmlMaterializePolicy) {
        if matches!(policy, DsmlMaterializePolicy::Off) {
            return;
        }
        if msg
            .tool_calls
            .as_ref()
            .is_some_and(|c| !c.is_empty() && has_usable_native_tool_calls(c))
        {
            return;
        }
        let c = crabmate_types::message_content_as_str(&msg.content).unwrap_or("");
        let r = msg.reasoning_content.as_deref().unwrap_or("");
        if c.is_empty() && r.is_empty() {
            return;
        }
        if !text_looks_like_dsml(c, r) {
            return;
        }
        let outcome = parse_combined_assistant_text(c, r);
        if outcome.invokes.is_empty() {
            return;
        }
        let out_calls: Vec<ToolCall> = outcome
            .invokes
            .into_iter()
            .enumerate()
            .map(|(i, inv)| ToolCall {
                id: format!("dsml_{i}"),
                typ: "function".to_string(),
                function: FunctionCall {
                    name: inv.name,
                    arguments: inv.arguments_json,
                },
            })
            .collect();
        debug!(
            target: "crabmate",
            "从助手正文 DeepSeek DSML 解析出 {} 个 tool_calls（API 未提供 tool_calls）",
            out_calls.len()
        );
        msg.tool_calls = Some(out_calls);
        strip_dsml_from_assistant_message_fields(msg);
    }
}

/// 部分 DeepSeek 兼容端在 **`tool_calls` 为空** 时，仍把调用写在正文 **DSML** 里。
pub fn materialize_deepseek_dsml_tool_calls_in_message(msg: &mut Message, enabled: bool) {
    DsmlToolCallAdapter::new()
        .apply_to_assistant_message(msg, DsmlMaterializePolicy::from_enabled(enabled));
}

pub fn strip_dsml_from_assistant_message_fields(msg: &mut Message) {
    fn trim_stripped_field(s: &mut Option<String>) {
        let Some(t) = s.as_deref() else {
            return;
        };
        let stripped = strip_deepseek_dsml_for_display(t);
        let u = stripped.trim();
        if looks_like_tool_argument_residue(u) {
            *s = None;
        } else {
            *s = Some(u.to_string());
        }
    }
    if let Some(MessageContent::Text(ref mut s)) = msg.content {
        let stripped = strip_deepseek_dsml_for_display(s);
        let u = stripped.trim();
        if looks_like_tool_argument_residue(u) {
            msg.content = None;
        } else {
            *s = u.to_string();
        }
    }
    trim_stripped_field(&mut msg.reasoning_content);
}
