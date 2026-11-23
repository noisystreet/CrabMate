use super::fence::strip_optional_json_fence_label;
use super::parse;
use super::types::AgentReplyPlanV1;

/// 「以下是…拆解」类引导语的**最长**字符数：再长更像正文，拼进气泡会把主内容淹掉。
const TASK_BREAKDOWN_LEAD_MAX_CHARS: usize = 72;
/// 截断后短于该长度的片段没有信息量（如只剩「以下是」），不当作引导语。
const TASK_BREAKDOWN_LEAD_MIN_CHARS: usize = 4;
/// 拼进气泡的引导语字符上限。
///
/// 只对 `contains("以下是任务拆解")` 这条**无长度上限**的分支生效：走「以下 + 拆解」分支时
/// 已在 `TASK_BREAKDOWN_LEAD_MAX_CHARS`(72) 处被挡下，够不到这里。
const TASK_BREAKDOWN_LEAD_CAP_CHARS: usize = 80;

/// 启发式：同时出现「以下」与「拆解」才算任务拆解引导语。
///
/// 判的是模型自由文本，没有结构化信号可替代，属**有意保留**的启发式——与 `context/agent_errors.rs`
/// 的样板一致：已知误判按启发式接受并钉住用例，而不是硬造一个更脆的判据（#1478）。
fn looks_like_breakdown_lead(t: &str) -> bool {
    t.contains("以下") && t.contains("拆解")
}

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

/// 模型常把「以下是任务拆解」写在首步 `description`，围栏前只有开场白；隐藏 JSON 时该句会随步骤列表一并从主气泡消失。若围栏前 goal 未同时含「以下」与「拆解」，则从首步描述里取**一行**简短引导拼到 goal 前。
fn augment_agent_reply_plan_goal_for_display(goal: &str, plan: &AgentReplyPlanV1) -> String {
    let goal = goal.trim();
    let lead = breakdown_lead_line_from_first_step_description(plan);
    let Some(lead) = lead else {
        return goal.to_string();
    };
    if looks_like_breakdown_lead(goal) {
        return goal.to_string();
    }
    if goal.is_empty() {
        return lead;
    }
    if goal.contains(&lead) {
        return goal.to_string();
    }
    format!("{lead}\n\n{goal}")
}

fn breakdown_lead_line_from_first_step_description(plan: &AgentReplyPlanV1) -> Option<String> {
    let st = plan.steps.first()?;
    let desc = crate::cm_agent::text_sanitize::naturalize_plan_step_description(&st.description);
    let desc = desc.trim();
    if desc.is_empty() {
        return None;
    }
    for line in desc.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if let Some(frag) = extract_task_breakdown_lead_fragment(t) {
            return Some(frag);
        }
    }
    None
}

/// 从首步描述的一行里取出「以下是…拆解」类短引导，在句末标点或冒号处截断，避免把整步描述拼进主气泡。
fn extract_task_breakdown_lead_fragment(line: &str) -> Option<String> {
    let t = line.trim();
    if t.is_empty() {
        return None;
    }
    let candidate = t.contains("以下是任务拆解")
        || (looks_like_breakdown_lead(t) && t.chars().count() <= TASK_BREAKDOWN_LEAD_MAX_CHARS);
    if !candidate {
        return None;
    }
    let start = t.find("以下")?;
    let tail = t[start..].trim_start();
    let clipped = clip_task_breakdown_lead_at_punct(tail);
    let clipped = clipped.trim();
    if clipped.chars().count() < TASK_BREAKDOWN_LEAD_MIN_CHARS {
        return None;
    }
    Some(cap_display_fragment(clipped, TASK_BREAKDOWN_LEAD_CAP_CHARS))
}

fn clip_task_breakdown_lead_at_punct(s: &str) -> &str {
    for (i, c) in s.char_indices() {
        if matches!(c, '。' | '！' | '？') {
            return &s[..i + c.len_utf8()];
        }
    }
    for (i, c) in s.char_indices() {
        if c == '：' {
            return &s[..i + c.len_utf8()];
        }
    }
    s
}

fn cap_display_fragment(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max_chars).collect();
        out.push('…');
        out
    }
}

/// 若 `content` 中含合法 `agent_reply_plan` v1，返回**简单 Markdown 有序列表**（可选围栏前自然语言段落；每步一行 `1. \`id\`: description`），不含原始 JSON。
/// 仅影响展示层；`Message.content` 仍为原文以便服务端继续解析。
pub fn format_agent_reply_plan_for_display(content: &str) -> Option<String> {
    let plan = parse::parse_agent_reply_plan_v1(content).ok()?;
    let raw_goal = prose_before_first_fence(content);
    let goal = crate::cm_agent::text_sanitize::naturalize_assistant_plan_prose_tail(&raw_goal);
    let goal_t = augment_agent_reply_plan_goal_for_display(goal.trim(), &plan);
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

    /// 短引导语被采纳，并在冒号处截断（不把整步描述拼进气泡）。
    #[test]
    fn short_breakdown_lead_is_adopted() {
        let frag = extract_task_breakdown_lead_fragment("以下是任务拆解：1) 读代码 2) 改实现");
        assert_eq!(frag.as_deref(), Some("以下是任务拆解："));
    }

    /// 超过 `TASK_BREAKDOWN_LEAD_MAX_CHARS` 的长句更像正文，不当作引导语。
    #[test]
    fn overlong_sentence_is_not_a_lead() {
        let line = format!("以下拆解{}", "很长的正文说明".repeat(20));
        assert!(line.chars().count() > TASK_BREAKDOWN_LEAD_MAX_CHARS);
        assert_eq!(extract_task_breakdown_lead_fragment(&line), None);
    }

    /// 判据是「以下」与「拆解」**两词与**：缺任一都不算引导语。
    #[test]
    fn needs_both_keywords() {
        assert_eq!(
            extract_task_breakdown_lead_fragment("以下是执行计划："),
            None
        );
        assert_eq!(
            extract_task_breakdown_lead_fragment("我拆解一下这个问题"),
            None
        );
    }

    /// `以下是任务拆解` 分支没有长度上限，靠 `TASK_BREAKDOWN_LEAD_CAP_CHARS` 兜住（72 分支够不到它）。
    #[test]
    fn canonical_lead_over_cap_is_truncated_with_ellipsis() {
        let line = format!("以下是任务拆解{}", "很长的正文说明".repeat(30));
        assert!(line.chars().count() > TASK_BREAKDOWN_LEAD_MAX_CHARS + TASK_BREAKDOWN_LEAD_CAP_CHARS);
        let frag = extract_task_breakdown_lead_fragment(&line).expect("无标点的长句仍应取到引导语");
        assert_eq!(
            frag.chars().count(),
            TASK_BREAKDOWN_LEAD_CAP_CHARS + 1,
            "上限 + 省略号：{frag}"
        );
        assert!(frag.ends_with('…'), "截断必须带省略号：{frag}");
    }
}
