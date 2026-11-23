use super::fence::strip_optional_json_fence_label;
use super::parse;
use super::types::AgentReplyPlanV1;

/// 首个 \`\`\` 代码围栏之前的正文（模型常在此写任务概括），与 JSON 块合读作「目标」。
fn prose_before_first_fence(content: &str) -> String {
    if !content.contains("```") {
        return String::new();
    }
    content.split("```").next().unwrap_or("").trim().to_string()
}

/// 将已解析的 v1 规划格式化为与终答展示相同的 Markdown 有序列表（`1. \`id\`: description`）；仅本模块的展示入口使用。
fn format_plan_steps_markdown(plan: &AgentReplyPlanV1) -> String {
    use std::fmt::Write;

    let mut out = String::new();
    let mut n = 1usize;
    for st in &plan.steps {
        let desc = crate::cm_agent::text_sanitize::naturalize_plan_step_description(&st.description);
        let id = st.id.trim();
        if id.is_empty() {
            continue;
        }
        let id_esc = id.replace('`', "'");
        let role = st
            .executor_kind
            .map(|k| format!(" [`executor_kind={}`]", k.as_snake_case_str()))
            .unwrap_or_default();
        let _ = writeln!(&mut out, "{}. `{}`: {}{}", n, id_esc, desc.trim(), role);
        n += 1;
    }
    out.trim_end().to_string()
}

/// 若 `content` 中含合法 `agent_reply_plan` v1，返回**简单 Markdown 有序列表**（可选围栏前自然语言段落；每步一行 `1. \`id\`: description`），不含原始 JSON。
/// 仅影响展示层；`Message.content` 仍为原文以便服务端继续解析。
pub fn format_agent_reply_plan_for_display(content: &str) -> Option<String> {
    let plan = parse::parse_agent_reply_plan_v1(content).ok()?;
    let raw_goal = prose_before_first_fence(content);
    let goal = crate::cm_agent::text_sanitize::naturalize_assistant_plan_prose_tail(&raw_goal);
    let goal_t = goal.trim().to_string();
    let steps = format_plan_steps_markdown(&plan);
    let mut out = String::new();
    if !goal_t.is_empty() {
        out.push_str(&goal_t);
        out.push_str("\n\n");
    }
    out.push_str(&steps);
    Some(out.trim_end().to_string())
}

fn fence_inner_should_hide_agent_reply_plan_json(inner: &str) -> bool {
    let raw = inner.trim();
    let body = strip_optional_json_fence_label(raw);
    if !body.starts_with('{') {
        return false;
    }
    if parse::parse_agent_reply_plan_v1(&body).is_ok() {
        return true;
    }
    let b = body.trim();
    // 流式输出时半截 JSON 往往已含 "agent_reply_plan" / "steps" 子串；若仅凭子串就隐去围栏，
    // 展示层会在 `assistant_markdown_source_for_display` 里得到空串，用户看不到围栏前的说明句。
    // 仅当围栏内已是**语法闭合**的 JSON 值、却仍不能通过 v1 规划校验时，再按形状隐去原文。
    if !b.contains("\"agent_reply_plan\"") || !b.contains("\"steps\"") {
        return false;
    }
    serde_json::from_str::<serde_json::Value>(b).is_ok()
}

/// 从展示用正文中移除含 `agent_reply_plan` 的 Markdown 代码围栏块（``` … ```），
/// 不修改 `Message.content`；日志仍可用 `debug!` 打印原文。
///
/// **未闭合且围栏内尚为空**（流式刚打出起始 ` ``` `、尚未写入语言行或正文）：不伪造收尾围栏。否则 `inner == ""` 时会拼出连续六个反引号（` ``` `` + `` + ``` `）。
pub fn strip_agent_reply_plan_fence_blocks_for_display(content: &str) -> String {
    let parts: Vec<&str> = content.split("```").collect();
    let unclosed_trailing_fence = parts.len().is_multiple_of(2);
    let mut out = String::new();
    let mut i = 0usize;
    while i < parts.len() {
        out.push_str(parts[i]);
        i += 1;
        if i >= parts.len() {
            break;
        }
        let inner = parts[i];
        i += 1;
        if fence_inner_should_hide_agent_reply_plan_json(inner) {
            continue;
        }
        if unclosed_trailing_fence && i >= parts.len() && inner.trim().is_empty() {
            break;
        }
        out.push_str("```");
        out.push_str(inner);
        out.push_str("```");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 首步描述里的「以下是任务拆解」**不再**被搬到气泡顶部（启发式已删除，#1478）。
    /// 用例存在的意义是防后人"顺手"把它加回来。
    #[test]
    fn breakdown_sentence_in_first_step_is_not_lifted_to_goal() {
        let step_json = r#"{"type":"agent_reply_plan","version":1,"steps":[{"id":"s1","description":"以下是任务拆解。创建 hello.cpp"}]}"#;
        let content = format!("让我先规划一下任务步骤：\n```json\n{step_json}\n```\n");
        let out = format_agent_reply_plan_for_display(&content).expect("plan display");
        // 引导句可以留在步骤行里，但不允许出现在步骤列表**之前**（那才是"上浮到气泡顶部"）。
        let head = out.split("1. `s1`").next().unwrap_or("");
        assert!(!head.contains("以下是任务拆解"), "引导句不应上浮：{out}");
        assert!(out.contains("让我先规划"), "{out}");
        assert!(out.contains("s1"), "步骤列表仍要保留：{out}");
    }
}
