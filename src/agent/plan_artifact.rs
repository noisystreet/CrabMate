//! 最终回答中的结构化「规划」产物：从 assistant content 中解析 JSON，替代 `## 规划` 等子串匹配。

use serde::Deserialize;

/// 约定的规划 JSON：`type` + `version` + 非空 `steps`。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AgentReplyPlanV1 {
    #[serde(rename = "type")]
    pub plan_type: String,
    pub version: u32,
    pub steps: Vec<PlanStepV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PlanStepV1 {
    pub id: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanArtifactError {
    /// 未找到可解析且通过校验的 JSON 块
    NotFound,
    WrongType(String),
    WrongVersion(u32),
    EmptySteps,
    InvalidStep {
        index: usize,
        reason: &'static str,
    },
}

/// Plan v1 的 schema 规则描述（中文），供提示词引用。
pub const PLAN_V1_SCHEMA_RULES: &str = "\
- 顶层 \"type\" 为字符串 \"agent_reply_plan\"
- \"version\" 为数字 1
- \"steps\" 为非空数组；每项含非空字符串 \"id\" 与 \"description\"";

/// Plan v1 的 JSON 示例。
pub const PLAN_V1_EXAMPLE_JSON: &str = r#"{"type":"agent_reply_plan","version":1,"steps":[{"id":"layer-0","description":"先执行无依赖节点 …"}]}"#;

/// 从整段 assistant `content` 中提取并校验 v1 规划（支持 \`\`\`json 围栏，或整段即为单个 JSON 对象）。
pub fn parse_agent_reply_plan_v1(content: &str) -> Result<AgentReplyPlanV1, PlanArtifactError> {
    for slice in collect_json_candidates(content) {
        let Ok(plan) = serde_json::from_str::<AgentReplyPlanV1>(&slice) else {
            continue;
        };
        if validate_agent_reply_plan_v1(&plan).is_ok() {
            return Ok(plan);
        }
    }
    Err(PlanArtifactError::NotFound)
}

#[allow(dead_code)] // `per_coord::content_has_plan` 等封装使用
pub fn content_has_valid_agent_reply_plan_v1(content: &str) -> bool {
    parse_agent_reply_plan_v1(content).is_ok()
}

fn validate_agent_reply_plan_v1(p: &AgentReplyPlanV1) -> Result<(), PlanArtifactError> {
    if p.plan_type != "agent_reply_plan" {
        return Err(PlanArtifactError::WrongType(p.plan_type.clone()));
    }
    if p.version != 1 {
        return Err(PlanArtifactError::WrongVersion(p.version));
    }
    if p.steps.is_empty() {
        return Err(PlanArtifactError::EmptySteps);
    }
    for (i, s) in p.steps.iter().enumerate() {
        if s.id.trim().is_empty() {
            return Err(PlanArtifactError::InvalidStep {
                index: i,
                reason: "id 为空",
            });
        }
        if s.description.trim().is_empty() {
            return Err(PlanArtifactError::InvalidStep {
                index: i,
                reason: "description 为空",
            });
        }
    }
    Ok(())
}

/// 候选 JSON 字符串：每个 fenced \`\`\` 块（奇数段）去掉可选的 `json` 语言行后尝试；再尝试整段 trim 后以 `{` 开头的全文。
fn collect_json_candidates(content: &str) -> Vec<String> {
    let mut out = Vec::new();
    let parts: Vec<&str> = content.split("```").collect();
    for i in (1..parts.len()).step_by(2) {
        let raw = parts[i].trim();
        if raw.is_empty() {
            continue;
        }
        let body = strip_optional_json_fence_label(raw);
        if body.starts_with('{') {
            out.push(body);
        }
    }
    let all = content.trim();
    if all.starts_with('{') && !out.iter().any(|s| s.as_str() == all) {
        out.push(all.to_string());
    }
    out
}

/// 首个 \`\`\` 代码围栏之前的正文（模型常在此写任务概括），与 JSON 块合读作「目标」。
pub(crate) fn prose_before_first_fence(content: &str) -> String {
    if !content.contains("```") {
        return String::new();
    }
    content.split("```").next().unwrap_or("").trim().to_string()
}

/// 将已解析的 v1 规划格式化为与终答展示相同的 Markdown 有序列表（`1. \`id\`: description`），供 TUI/CLI 分步通知与气泡展示共用。
pub fn format_plan_steps_markdown(plan: &AgentReplyPlanV1) -> String {
    use std::fmt::Write;

    let mut out = String::new();
    let mut n = 1usize;
    for st in &plan.steps {
        let desc = crate::text_sanitize::naturalize_plan_step_description(&st.description);
        let id = st.id.trim();
        if id.is_empty() {
            continue;
        }
        let id_esc = id.replace('`', "'");
        let _ = writeln!(&mut out, "{}. `{}`: {}", n, id_esc, desc.trim());
        n += 1;
    }
    out.trim_end().to_string()
}

/// 分阶段规划**队列**区：每步前加 `[ ]` 未完成 / `[✓]` 已完成（`completed_count` 为已完成步数，与 `run_staged_plan_then_execute_steps` 中步下标一致）。
/// 仅展示 `description`（**不**输出 `step.id`，便于阅读；`Message` / `debug!` 等仍含 id）。
pub fn format_plan_steps_markdown_for_staged_queue(
    plan: &AgentReplyPlanV1,
    completed_count: usize,
) -> String {
    use std::fmt::Write;

    let mut out = String::new();
    let n = plan.steps.len();
    let done = completed_count.min(n);
    let mut line_no = 1usize;
    for (idx, st) in plan.steps.iter().enumerate() {
        let desc = crate::text_sanitize::naturalize_plan_step_description(&st.description);
        let id = st.id.trim();
        if id.is_empty() {
            continue;
        }
        let mark = if idx < done { "[✓]" } else { "[ ]" };
        let _ = writeln!(&mut out, "{mark} {}. {}", line_no, desc.trim());
        line_no += 1;
    }
    out.trim_end().to_string()
}

/// 模型常把「以下是任务拆解」写在首步 `description`，围栏前只有开场白；隐藏 JSON 时该句会随步骤列表一并从主气泡消失。若围栏前 goal 未同时含「以下」与「拆解」，则从首步描述里取**一行**简短引导拼到 goal 前。
pub(crate) fn augment_agent_reply_plan_goal_for_display(
    goal: &str,
    plan: &AgentReplyPlanV1,
) -> String {
    let goal = goal.trim();
    let lead = breakdown_lead_line_from_first_step_description(plan);
    let Some(lead) = lead else {
        return goal.to_string();
    };
    if goal.contains("以下") && goal.contains("拆解") {
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
    let desc = crate::text_sanitize::naturalize_plan_step_description(&st.description);
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
        || (t.contains("以下") && t.contains("拆解") && t.chars().count() <= 72);
    if !candidate {
        return None;
    }
    let start = t.find("以下")?;
    let tail = t[start..].trim_start();
    let clipped = clip_task_breakdown_lead_at_punct(tail);
    let clipped = clipped.trim();
    if clipped.chars().count() < 4 {
        return None;
    }
    Some(cap_display_fragment(clipped, 80))
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
    let plan = parse_agent_reply_plan_v1(content).ok()?;
    let raw_goal = prose_before_first_fence(content);
    let goal = crate::text_sanitize::naturalize_assistant_plan_prose_tail(&raw_goal);
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

fn strip_optional_json_fence_label(raw: &str) -> String {
    let mut lines = raw.lines();
    let Some(first) = lines.next() else {
        return raw.trim().to_string();
    };
    let first_t = first.trim();
    if first_t.eq_ignore_ascii_case("json") {
        lines.collect::<Vec<_>>().join("\n").trim().to_string()
    } else {
        raw.trim().to_string()
    }
}

fn fence_inner_should_hide_agent_reply_plan_json(inner: &str) -> bool {
    let raw = inner.trim();
    let body = strip_optional_json_fence_label(raw);
    if !body.starts_with('{') {
        return false;
    }
    if parse_agent_reply_plan_v1(&body).is_ok() {
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
pub fn strip_agent_reply_plan_fence_blocks_for_display(content: &str) -> String {
    let parts: Vec<&str> = content.split("```").collect();
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
        out.push_str("```");
        out.push_str(inner);
        out.push_str("```");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_json() -> String {
        r#"{"type":"agent_reply_plan","version":1,"steps":[{"id":"a","description":"do a"}]}"#
            .to_string()
    }

    #[test]
    fn strip_fence_removes_plan_json_keeps_prose() {
        let bad = r#"{"type":"agent_reply_plan","version":1,"steps":[]}"#;
        let content = format!("说明\n```json\n{bad}\n```\n");
        let s = strip_agent_reply_plan_fence_blocks_for_display(&content);
        assert!(s.contains("说明"));
        assert!(!s.contains("agent_reply_plan"));
    }

    #[test]
    fn strip_fence_keeps_streaming_incomplete_plan_inside_fence() {
        let partial =
            r#"{"type":"agent_reply_plan","version":1,"steps":[{"id":"a","description":"x""#;
        let content = format!("先说明一句。\n```json\n{partial}");
        let s = strip_agent_reply_plan_fence_blocks_for_display(&content);
        assert!(s.contains("先说明一句"));
        assert!(
            s.contains("agent_reply_plan"),
            "语法未闭合时不应剥掉围栏内流式正文"
        );
    }

    #[test]
    fn parses_fenced_json() {
        let content = format!("说明\n```json\n{}\n```\n", sample_json());
        let p = parse_agent_reply_plan_v1(&content).unwrap();
        assert_eq!(p.steps.len(), 1);
        assert_eq!(p.steps[0].id, "a");
    }

    #[test]
    fn augment_goal_prepends_breakdown_lead_when_only_in_first_step() {
        let step_json = r#"{"type":"agent_reply_plan","version":1,"steps":[{"id":"s1","description":"以下是任务拆解。创建 hello.cpp"}]}"#;
        let content = format!("让我先规划一下任务步骤：\n```json\n{step_json}\n```\n");
        let plan = parse_agent_reply_plan_v1(&content).unwrap();
        let raw = prose_before_first_fence(&content);
        let goal = crate::text_sanitize::naturalize_assistant_plan_prose_tail(&raw);
        let out = augment_agent_reply_plan_goal_for_display(goal.trim(), &plan);
        assert!(
            out.contains("以下是任务拆解"),
            "应拼回首步里的拆解引导句: {}",
            out
        );
        assert!(out.contains("让我先规划"), "{}", out);
    }

    #[test]
    fn parses_raw_json_only_message() {
        let p = parse_agent_reply_plan_v1(&sample_json()).unwrap();
        assert_eq!(p.plan_type, "agent_reply_plan");
    }

    #[test]
    fn rejects_legacy_heading() {
        let content = "## 规划\n- step one";
        assert!(parse_agent_reply_plan_v1(content).is_err());
    }

    #[test]
    fn rejects_wrong_type() {
        let s = r#"{"type":"other","version":1,"steps":[{"id":"x","description":"y"}]}"#;
        assert!(parse_agent_reply_plan_v1(s).is_err());
    }

    #[test]
    fn rejects_empty_steps() {
        let s = r#"{"type":"agent_reply_plan","version":1,"steps":[]}"#;
        assert!(parse_agent_reply_plan_v1(s).is_err());
    }

    #[test]
    fn rejects_bad_json_in_fence_then_accepts_second() {
        let content = r#"
```json
not json
```
```json
{"type":"agent_reply_plan","version":1,"steps":[{"id":"1","description":"ok"}]}
```
"#;
        assert!(parse_agent_reply_plan_v1(content).is_ok());
    }

    #[test]
    fn staged_queue_marks_done_steps() {
        let plan = parse_agent_reply_plan_v1(
            r#"{"type":"agent_reply_plan","version":1,"steps":[
                {"id":"a","description":"one"},
                {"id":"b","description":"two"}
            ]}"#,
        )
        .unwrap();
        let s0 = format_plan_steps_markdown_for_staged_queue(&plan, 0);
        assert!(s0.contains("[ ]"));
        assert!(!s0.contains("[✓]"));
        let s1 = format_plan_steps_markdown_for_staged_queue(&plan, 1);
        assert!(s1.lines().next().unwrap_or("").contains("[✓]"));
        assert!(s1.lines().nth(1).unwrap_or("").contains("[ ]"));
        let s2 = format_plan_steps_markdown_for_staged_queue(&plan, 2);
        assert_eq!(s2.matches("[✓]").count(), 2);
    }

    #[test]
    fn format_display_includes_goal_and_steps() {
        let content = "先调研再改代码。\n```json\n{\"type\":\"agent_reply_plan\",\"version\":1,\"steps\":[{\"id\":\"s1\",\"description\":\"读 README\"},{\"id\":\"s2\",\"description\":\"改 main\"}]}\n```\n";
        let s = format_agent_reply_plan_for_display(content).expect("formatted");
        assert!(s.contains("调研"));
        assert!(s.contains("1. `s1`: 读 README"));
        assert!(s.contains("2. `s2`: 改 main"));
        assert!(!s.contains("agent_reply_plan"));
    }

    #[test]
    fn format_display_raw_json_only_still_works() {
        let content =
            r#"{"type":"agent_reply_plan","version":1,"steps":[{"id":"a","description":"do"}]}"#;
        let s = format_agent_reply_plan_for_display(content).expect("formatted");
        assert_eq!(s, "1. `a`: do");
    }
}
