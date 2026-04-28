//! 日志与轨迹用字符串截断、思维链标签剥离。

/// 子目标内工具轨迹用截断：运行类保留更多字符，供 GoalVerifier 识别 stdout
pub(crate) fn truncate_for_subgoal_trace(output: &str, tool_name: &str) -> String {
    const DEFAULT_MAX: usize = 200;
    const RUN_LIKE_MAX: usize = 8000;
    let max = if tool_name == "run_executable" || tool_name == "run_command" {
        RUN_LIKE_MAX
    } else {
        DEFAULT_MAX
    };
    if output.len() > max {
        let truncated = output
            .char_indices()
            .take(max.saturating_sub(3))
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(0);
        format!("{}...", &output[..truncated])
    } else {
        output.to_string()
    }
}

/// 截断输出用于日志（按字符边界截断，支持中文）
pub(crate) fn truncate_output(output: &str) -> String {
    const MAX_LEN: usize = 200;
    if output.len() > MAX_LEN {
        let truncated = output
            .char_indices()
            .take(MAX_LEN - 3)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(0);
        format!("{}...", &output[..truncated])
    } else {
        output.to_string()
    }
}

/// 剥离思维链标签
pub(crate) fn strip_thinking_tags(text: &str) -> String {
    let mut result = text.to_string();
    while let Some(start) = result.find("<think>") {
        if let Some(end) = result[start..].find("</think>") {
            let close_tag = "</think>";
            result = format!(
                "{}{}",
                &result[..start],
                &result[start + end + close_tag.len()..]
            );
        } else {
            break;
        }
    }
    result.trim().to_string()
}

/// 截断目标描述用于日志（按字符边界截断，支持中文）
pub(crate) fn truncate_goal(desc: &str) -> String {
    const MAX_LEN: usize = 80;
    if desc.len() > MAX_LEN {
        let truncated = desc
            .char_indices()
            .take(MAX_LEN - 3)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(0);
        format!("{}...", &desc[..truncated])
    } else {
        desc.to_string()
    }
}
