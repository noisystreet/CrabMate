//! 分阶段规划：首轮 JSON 解析成功后的**可选**第二轮无工具优化（合并探查步、提示单轮内批量并行工具）。

use std::collections::BTreeSet;

use crate::agent::plan_artifact::{self, AgentReplyPlanV1, PlanStepV1};
use crate::config::AgentConfig;
use crate::config::StagedPlanBaselineMode;
use crate::types::{
    Message, Tool, is_first_turn_workspace_context_injection,
    is_message_excluded_from_llm_context_except_memory, is_server_injected_user_message,
};

/// 步骤优化轮注入的 user 正文标记（取消/失败时弹出临时 user）。
pub(crate) const STAGED_PLAN_OPTIMIZER_COACH_MARK: &str = "### 分阶段规划 · 步骤优化（服务端注入）";

/// 本会话 `tools_defs` 中，满足「同轮可多调用并行 `spawn_blocking`」的工具名（逗号分隔，已排序去重）。
pub(crate) fn parallel_batchable_tool_names_csv_from_defs(
    tools: &[Tool],
    handler_lookup: &crate::tool_registry::HandlerLookupTable,
    cfg: &AgentConfig,
) -> String {
    let mut names = BTreeSet::new();
    for t in tools {
        let n = t.function.name.as_str();
        if crate::tool_registry::tool_ok_for_parallel_readonly_batch_piece(handler_lookup, cfg, n) {
            names.insert(n.to_string());
        }
    }
    names.into_iter().collect::<Vec<_>>().join(", ")
}

#[inline]
fn user_task_line_visible(m: &Message, skip_workspace_bootstrap: bool) -> Option<&str> {
    if m.role != "user" {
        return None;
    }
    if is_message_excluded_from_llm_context_except_memory(m)
        || is_server_injected_user_message(m)
        || (skip_workspace_bootstrap && is_first_turn_workspace_context_injection(m))
    {
        return None;
    }
    let t = crate::types::message_content_as_str(&m.content)?.trim();
    (!t.is_empty()).then_some(t)
}

/// 从会话缓冲**从旧到新**扫描，取第一条计入模型的真实 user 正文（通常为本轮用户初始诉求）。
///
/// 与 [`staged_plan_trigger_user_content`] 相对：后者取**最新**一条 user；滚动重规划后缓冲末尾常为分步注入或教练句，
/// 不宜作为「不变层」锚点。跳过首轮工作区画像等 `user.name` 注入。
pub(crate) fn staged_plan_turn_anchor_user_content(messages: &[Message]) -> Option<&str> {
    messages
        .iter()
        .find_map(|m| user_task_line_visible(m, true))
}

/// 在规划轮 assistant 尚未入史时，从 `messages` 末尾回溯，取**触发本轮分阶段规划**的用户正文（跳过注入类 user）。
pub(crate) fn staged_plan_trigger_user_content(messages: &[Message]) -> Option<&str> {
    messages
        .iter()
        .rev()
        .find_map(|m| user_task_line_visible(m, false))
}

/// 无工具规划轮 system 末尾追加：不变层用户原文 + 少量硬约束。
pub(crate) fn staged_rolling_immutable_plan_system_appendix(goal: &str) -> String {
    format!(
        "\n\n### 不变层（系统持有·本轮用户原文）\n{}\n\n\
         ### 不变层约束（硬；每次滚动重规划须自检）\n\
         - 上文用户原文为本轮**终极目标**；仅步骤、顺序与实现细节可调，**不得**用过程中涌现的子话题、中间小结或「扩展分析」替代或架空该目标。\n\
         - 若新信息与总目标冲突或无法同时满足：优先在规划中请求澄清或收敛到总目标，**勿擅自改题**。\n\
         - 遵守工作区与工具安全边界；不得索取或输出密钥、token。\n",
        goal.trim()
    )
}

/// 分步执行注入 user 开头：短锚定，避免步内工具链漂移。
pub(crate) fn staged_rolling_immutable_step_user_prefix(goal: &str) -> String {
    format!(
        "【不变层·本轮用户总目标】（本步工具与终答须对齐，勿偏题）\n{}\n\n",
        goal.trim()
    )
}

const STAGED_BASELINE_PLAN_MD_MAX_CHARS: usize = 6000;

/// 首轮定稿计划的紧凑 Markdown，供无工具规划 **system** 锚定（控制长度）。
pub(crate) fn format_baseline_plan_v1_compact_md(plan: &AgentReplyPlanV1) -> String {
    let mut s = String::from("### 首轮定稿计划（蓝图快照）\n");
    for (i, st) in plan.steps.iter().enumerate() {
        let desc_short: String = st.description.trim().chars().take(220).collect();
        s.push_str(&format!("{}. `{}` — {}\n", i + 1, st.id.trim(), desc_short));
    }
    if s.len() > STAGED_BASELINE_PLAN_MD_MAX_CHARS {
        s.truncate(STAGED_BASELINE_PLAN_MD_MAX_CHARS);
        s.push_str("\n…（截断）\n");
    }
    s
}

/// 滚动重规划 / 补丁规划轮：在 **system** 末尾附加冻结蓝图与自检约束（[`StagedPlanBaselineMode::ImmutableGoalOnly`] 为空串）。
pub(crate) fn staged_baseline_plan_planner_system_appendix(
    baseline: &AgentReplyPlanV1,
    mode: StagedPlanBaselineMode,
) -> String {
    match mode {
        StagedPlanBaselineMode::ImmutableGoalOnly => String::new(),
        StagedPlanBaselineMode::GoalPlusBaselinePlan
        | StagedPlanBaselineMode::StrictBaselineSteps => {
            let md = format_baseline_plan_v1_compact_md(baseline);
            let strict_note = if mode == StagedPlanBaselineMode::StrictBaselineSteps {
                "\n### 严格模式（`strict_baseline_steps`）\n\
                 - **`patch_planner` 合并结果**：在「尚未被补丁替换的前缀」上，每一步的 `id` 必须与上述蓝图中**同一下标**的 `id` 完全一致。\n"
            } else {
                ""
            };
            format!(
                "\n\n### 蓝图锚点（服务端冻结·须在后续规划中自检）\n\
                 下文为首轮进入分步执行前定稿的 `agent_reply_plan` v1 摘要。**不得**用新话题替代用户不变层总目标；若须调整步骤，请在正文中**简要说明**相对该蓝图保留、合并或替换哪些意图。\n\n\
                 {md}\
                 {strict_note}\
                 ### 硬约束\n\
                 - 仍须遵守上文「不变层」用户总目标与工具安全边界。\n\
                 - 若新信息与蓝图或总目标冲突：优先请求澄清或收敛，**勿擅自改题**。\n"
            )
        }
    }
}

/// 启发式：是否像闲聊/极短输入，适合跳过逻辑多规划员（ensemble）以省 API。
pub(crate) fn staged_plan_user_prompt_looks_like_casual_or_trivial(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() {
        return true;
    }
    let n_chars = t.chars().count();
    if n_chars <= 12 {
        return true;
    }
    let lower = t.to_lowercase();
    const CASUAL: &[&str] = &[
        "谢谢",
        "感谢",
        "多谢",
        "好的",
        "好",
        "嗯",
        "嗯嗯",
        "ok",
        "okay",
        "hi",
        "hello",
        "hey",
        "哈哈",
        "呵呵",
        "在吗",
        "在么",
        "早上好",
        "下午好",
        "晚上好",
        "再见",
        "拜拜",
    ];
    CASUAL.iter().any(|p| {
        lower == *p
            || lower.starts_with(&format!("{p}，"))
            || lower.starts_with(&format!("{p},"))
            || lower.starts_with(&format!("{p}。"))
            || lower.starts_with(&format!("{p}！"))
            || lower.starts_with(&format!("{p}!"))
    })
}

/// 生成「规划优化轮」注入的 user 正文（中文）。
pub(crate) fn staged_plan_optimizer_user_body(
    plan: &AgentReplyPlanV1,
    parallel_tool_names_csv: &str,
) -> String {
    let steps_md = plan_artifact::format_plan_steps_markdown(plan);
    let batch_tools_line = if parallel_tool_names_csv.trim().is_empty() {
        "- 不仅限于 `read_file`：凡可在**同一助手轮**内并行发起的**只读**内建工具，应优先放在**同一步**中批量使用。当前会话工具列表中**暂无**经服务端判定为「可同轮并行批处理」的内建名；仍请尽量合并无依赖的只读探查步。"
            .to_string()
    } else {
        format!(
            "- 不仅限于 `read_file`：凡可在**同一助手轮**内并行发起的只读内建工具，应优先放在**同一步**中批量使用。本进程当前会话下可同轮并行批处理的内建工具名包括：{}。",
            parallel_tool_names_csv
        )
    };
    format!(
        "{}\n\
         下面是你刚输出的可执行规划（Markdown 列表，**勿**改动其中 `id`，除非合并/拆分时为新步分配新 id）。\n\
         请在**不削弱任务覆盖面**的前提下优化 `steps`：\n\
         - 将**连续、彼此无数据依赖**且主要为**只读探查**的子目标合并为更少的大步（一步内可安排**多次**工具调用）。\n\
         {batch_tools_line}\n\
         - **不要**把「必须先读后写」或「必须先分析再改」的强依赖硬塞进同一步；写盘、`run_command`、网络变更类步骤保持独立或放在只读探查之后。\n\
         - 若各步带有 `executor_kind`（`review_readonly` / `patch_write` / `test_runner`），优化后**应保留**其分步工具边界，除非合并步在语义上仍满足同一角色。\n\
         - 若原规划已足够紧凑，可原样返回（`steps` 与下列列表等价即可）。\n\n\
         **输出要求**：仅输出一段可解析的 `agent_reply_plan` v1 JSON（可用 ```json 围栏），`type`/`version`/`steps` 与既有 schema 一致；`no_task` 须为 false 且 `steps` 非空。\n\n\
         Schema：{}\n\
         示例：\n```json\n{}\n```\n\n\
         ---\n\
         当前规划步骤：\n\
         {}",
        STAGED_PLAN_OPTIMIZER_COACH_MARK,
        plan_artifact::PLAN_V1_SCHEMA_RULES,
        plan_artifact::PLAN_V1_EXAMPLE_JSON,
        steps_md,
    )
}

/// 若优化轮解析成功且 `steps` 非空，返回新步骤；否则返回 `None`（调用方沿用首轮规划）。
pub(crate) fn try_parse_optimizer_reply(content: &str) -> Option<Vec<PlanStepV1>> {
    let p = plan_artifact::parse_agent_reply_plan_v1(content).ok()?;
    if p.no_task || p.steps.is_empty() {
        return None;
    }
    Some(p.steps)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FunctionDef, Tool};

    #[test]
    fn parallel_csv_from_defs_filters_by_registry() {
        let tools = vec![
            Tool {
                typ: "function".to_string(),
                function: FunctionDef {
                    name: "read_file".to_string(),
                    description: String::new(),
                    parameters: serde_json::json!({}),
                },
            },
            Tool {
                typ: "function".to_string(),
                function: FunctionDef {
                    name: "create_file".to_string(),
                    description: String::new(),
                    parameters: serde_json::json!({}),
                },
            },
        ];
        let cfg = crate::config::load_config(None).expect("embed default");
        let hl = crate::tool_registry::HandlerLookupTable::default_dispatch();
        let csv = parallel_batchable_tool_names_csv_from_defs(&tools, &hl, &cfg);
        assert!(csv.contains("read_file"));
        assert!(!csv.contains("create_file"));
    }

    #[test]
    fn try_parse_optimizer_rejects_no_task() {
        let body = r#"{"type":"agent_reply_plan","version":1,"no_task":true,"steps":[]}"#;
        assert!(try_parse_optimizer_reply(body).is_none());
    }

    #[test]
    fn casual_or_trivial_user_prompt_detection() {
        assert!(staged_plan_user_prompt_looks_like_casual_or_trivial(
            "谢谢！"
        ));
        assert!(staged_plan_user_prompt_looks_like_casual_or_trivial("ok"));
        assert!(staged_plan_user_prompt_looks_like_casual_or_trivial(
            "  hi  "
        ));
        assert!(staged_plan_user_prompt_looks_like_casual_or_trivial(
            "好的，知道了"
        ));
        assert!(!staged_plan_user_prompt_looks_like_casual_or_trivial(
            "请把 src/foo.rs 里的 bar 函数改成返回 Result"
        ));
    }

    #[test]
    fn trigger_user_skips_injections() {
        use crate::types::Message;
        let msgs = vec![
            Message::user_only("plain ask"),
            Message {
                role: "user".into(),
                content: Some("memo".into()),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: Some(crate::types::CRABMATE_LONG_TERM_MEMORY_NAME.into()),
                tool_call_id: None,
            },
        ];
        assert_eq!(staged_plan_trigger_user_content(&msgs), Some("plain ask"));
    }

    #[test]
    fn anchor_user_prefers_chronological_first_not_latest() {
        use crate::types::Message;
        let msgs = vec![
            Message::user_only("总目标：修 A"),
            Message::assistant_only("ok"),
            Message::user_only("### 分步 1/2\n子步"),
        ];
        assert_eq!(
            staged_plan_turn_anchor_user_content(&msgs),
            Some("总目标：修 A")
        );
        assert_eq!(
            staged_plan_trigger_user_content(&msgs),
            Some("### 分步 1/2\n子步")
        );
    }

    #[test]
    fn anchor_skips_first_turn_workspace_injection() {
        use crate::types::{CRABMATE_FIRST_TURN_WORKSPACE_CONTEXT_NAME, Message, MessageContent};
        let msgs = vec![
            Message {
                role: "user".into(),
                content: Some(MessageContent::Text("bootstrap".into())),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: Some(CRABMATE_FIRST_TURN_WORKSPACE_CONTEXT_NAME.into()),
                tool_call_id: None,
            },
            Message::user_only("真实问题"),
        ];
        assert_eq!(
            staged_plan_turn_anchor_user_content(&msgs),
            Some("真实问题")
        );
    }
}
