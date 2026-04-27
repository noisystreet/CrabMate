//! 分层 Manager：首轮结构化 JSON 解析失败时，**最多一次**无工具低温补调用以修复 JSON。
//!
//! 供任务分解输出与验证失败反思两条路径共用（`ManagerAgent`）。

use crate::config::AgentConfig;
use crate::llm::{
    CompleteChatRetryingParams, complete_chat_retrying,
    no_tools_chat_request_for_hierarchical_manager,
};
use crate::types::{LlmSeedOverride, Message, message_content_as_str};

/// 从响应中提取最外层 JSON 对象切片（跳过前文噪音）。
///
/// 须在 **双引号字符串外**统计 `{}`，否则子目标 `description` 等字段中的 `{` / `}` 会破坏朴素括号计数，
/// 导致永远找不到匹配的闭合括号（表现为 `Failed to extract JSON`）。
pub(crate) fn extract_json(content: &str) -> Option<&str> {
    let start = content.find('{')?;
    let mut depth = 0u32;
    let mut in_string = false;
    let mut escape = false;
    for (rel_byte, c) in content[start..].char_indices() {
        if in_string {
            if escape {
                escape = false;
                continue;
            }
            match c {
                '\\' => escape = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '{' => depth = depth.saturating_add(1),
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    let end = start + rel_byte + c.len_utf8();
                    return Some(&content[start..end]);
                }
            }
            _ => {}
        }
    }
    None
}

/// 为 JSON 修复补调用提取“最像 JSON”的候选片段，尽量降低噪声。
pub(crate) fn extract_json_candidate_for_repair(content: &str) -> String {
    if let Some(s) = extract_json(content) {
        return s.to_string();
    }
    if let Some(start) = content.find('{') {
        return truncate_to_char_boundary(&content[start..], 12_000).to_string();
    }
    truncate_to_char_boundary(content, 12_000).to_string()
}

#[derive(Debug)]
pub(crate) struct ExtractJsonDiagnostic {
    pub depth: u32,
    pub in_string: bool,
    pub tail: String,
}

/// JSON 提取失败时的快速诊断：
/// - `depth > 0`：大概率为输出被截断（缺失闭合 `}`）；
/// - `in_string = true`：字符串可能未闭合；
/// - `tail`：末尾片段，便于定位中断点。
pub(crate) fn extract_json_diagnostic(content: &str) -> ExtractJsonDiagnostic {
    let start = content.find('{').unwrap_or(0);
    let mut depth = 0u32;
    let mut in_string = false;
    let mut escape = false;
    for c in content[start..].chars() {
        if in_string {
            if escape {
                escape = false;
                continue;
            }
            match c {
                '\\' => escape = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '{' => depth = depth.saturating_add(1),
            '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    let tail: String = content
        .chars()
        .rev()
        .take(200)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    ExtractJsonDiagnostic {
        depth,
        in_string,
        tail,
    }
}

/// 在 utf-8 下按**字符**数截断到上限（用于打包进二轮 JSON 修复的 user 消息，避免超上下文）。
pub(crate) fn truncate_to_char_boundary(s: &str, max_chars: usize) -> &str {
    if s.chars().count() <= max_chars {
        return s;
    }
    let mut n = 0;
    for (i, c) in s.char_indices() {
        n += 1;
        if n == max_chars {
            let end = i + c.len_utf8();
            return &s[..end];
        }
    }
    s
}

/// 二次调用：两段 user（JSON 片段 + 修复说明），无工具、`force_json_mode` 后 `complete_chat_retrying`。
pub(crate) async fn one_shot_json_repair_llm_response(
    params: &CompleteChatRetryingParams<'_>,
    cfg: &AgentConfig,
    repair_temperature: Option<f32>,
    seed_override: LlmSeedOverride,
    force_json_mode: fn(&mut crate::types::ChatRequest),
    json_fragment: String,
    repair_user_prompt: String,
) -> Result<String, String> {
    let messages = vec![
        Message::user_only(json_fragment),
        Message::user_only(repair_user_prompt),
    ];
    let mut request = no_tools_chat_request_for_hierarchical_manager(
        cfg,
        &messages,
        repair_temperature,
        None,
        seed_override,
    );
    force_json_mode(&mut request);
    let response = complete_chat_retrying(params, &request)
        .await
        .map_err(|e| e.to_string())?;
    Ok(message_content_as_str(&response.0.content)
        .unwrap_or_default()
        .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_json() {
        let content = "好的，我来分解。\n{\n  \"sub_goals\": []\n}\n完成";
        let json = extract_json(content).unwrap();
        assert!(json.contains("sub_goals"));
    }

    #[test]
    fn test_extract_json_diagnostic_reports_unclosed_object() {
        let d = extract_json_diagnostic(r#"{"sub_goals":[{"goal_id":"g1""#);
        assert!(d.depth > 0);
        assert!(!d.tail.is_empty());
    }

    #[test]
    fn extract_json_candidate_prefers_json_slice() {
        let raw = "前文噪音\n{\"sub_goals\":[]}\n后文";
        let out = extract_json_candidate_for_repair(raw);
        assert_eq!(out, "{\"sub_goals\":[]}");
    }

    #[test]
    fn test_extract_json_braces_inside_string_values() {
        let content = r#"{"sub_goals":[{"goal_id":"g1","description":"查看 {src} 与 } 符号","priority":1,"depends_on":[],"required_tools":[]}],"execution_strategy":"sequential"}"#;
        let json = extract_json(content).unwrap();
        assert!(json.contains("sub_goals"));
        assert!(json.contains("{src}"));
        let v: serde_json::Value = serde_json::from_str(json).expect("valid JSON");
        assert!(v.get("sub_goals").is_some());
    }
}
