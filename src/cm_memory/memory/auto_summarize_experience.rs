//! 回合结束后启发式检测「由失败到成功」的构建/验证类工具调用，自动写入长期记忆（`source_role=auto_summarize_experience`）。

use crate::agent::agent_turn::policy::task_level_evidence::command_line_looks_like_verify;
use crate::cm_names as tool_names;
use crate::cm_tools::tool_result::{
    ToolStatus, normalize_tool_message_content, tool_output_status,
};
use crate::cm_types::{Message, message_content_as_str};

/// 检测本回合是否值得自动沉淀；返回经验正文与标签。
pub fn draft_auto_experience_from_turn(messages: &[Message]) -> Option<(String, Vec<String>)> {
    let turn_start = last_non_injection_user_index(messages)?;
    let turn = &messages[turn_start..];
    let (user_text, assistant_text) = last_user_assistant_final_pair(turn)?;
    if !turn_has_user_task_context(user_text) {
        return None;
    }
    let recovery = detect_build_recovery_in_slice(turn)?;
    let experience = format_auto_experience(user_text, assistant_text, &recovery)?;
    if experience.chars().count() < 20 {
        return None;
    }
    Some((experience, tags_for_recovery(&recovery)))
}

fn last_non_injection_user_index(messages: &[Message]) -> Option<usize> {
    for (i, m) in messages.iter().enumerate().rev() {
        if m.role != "user" {
            continue;
        }
        if crate::cm_types::server_injected_user::is_runtime_refreshed_injection(m) {
            continue;
        }
        let c = message_content_as_str(&m.content)?.trim();
        if c.is_empty() {
            continue;
        }
        return Some(i);
    }
    None
}

fn last_user_assistant_final_pair(messages: &[Message]) -> Option<(&str, &str)> {
    crate::cm_memory::memory::long_term_memory::last_user_assistant_final_pair_for_turn(messages)
}

/// 本轮是否存在**可归因的任务上下文**。
///
/// 只做**形态**判断（真实 user 发言 + 非纯寒暄长度），**不再匹配构建/故障词表**：
/// 词表双向都会错——「这个 build 阶段没问题，帮我解释下 test 目录」会误命中，
/// 「帮我把编译不过的地方改对」会漏命中；而且函数名说「是否提出任务」，
/// 实现却只判「是否提到这些词」，名实不符本身就是误判源（#1438）。
///
/// 「这一轮是否真的发生了构建失败并修复」由 [`detect_build_recovery_in_slice`] 的
/// **结构化**判定承担（`tool_output_status` 的 Failed → Ok），不需要用户句来确认。
fn turn_has_user_task_context(user: &str) -> bool {
    const MIN_TASK_CHARS: usize = 8;
    user.chars().count() >= MIN_TASK_CHARS
}

struct BuildRecovery {
    tool_name: String,
}

fn detect_build_recovery_in_slice(turn: &[Message]) -> Option<BuildRecovery> {
    let mut saw_build_failure = false;
    let mut recovery_tool: Option<String> = None;
    for m in turn {
        if m.role != "tool" {
            continue;
        }
        let tool_name = m.name.as_deref().unwrap_or("").trim();
        if tool_name.is_empty() || !is_build_verify_tool(tool_name) {
            continue;
        }
        let content = message_content_as_str(&m.content).unwrap_or("").trim();
        if tool_name == tool_names::RUN_COMMAND && !run_command_is_build(content) {
            continue;
        }
        match tool_output_status(tool_name, content) {
            // 判不出来（无结构化信号、正文也不像失败）：既不算失败也不算恢复。
            // 旧实现此处判 `false` 并设 `saw_build_failure`，会凭空造出「失败→成功恢复」经验。
            ToolStatus::Unknown => continue,
            ToolStatus::Failed => {
                saw_build_failure = true;
                continue;
            }
            ToolStatus::Ok => {}
        }
        if saw_build_failure {
            recovery_tool = Some(tool_name.to_string());
            break;
        }
    }
    let tool_name = recovery_tool?;
    Some(BuildRecovery { tool_name })
}

fn is_build_verify_tool(name: &str) -> bool {
    matches!(
        name,
        tool_names::RUN_COMMAND
            | tool_names::CARGO_CHECK
            | tool_names::CARGO_TEST
            | tool_names::CARGO_CLIPPY
    )
}

/// `run_command` 这条工具消息是否在跑构建 / 测试：**只看结构化 `invocation`**。
///
/// 与 `evidence.rs` 同口径（同一个 `command_line_looks_like_verify`，见 #1422 A5）。
/// 不再回落到正文关键词（#1440 K7）：本函数处理的切片已被裁到**本轮**
/// （`last_non_injection_user_index`），而本轮 `run_command` 结果恒含 `命令：` 行、
/// `crabmate_tool` 信封恒开——正文兜底不可达，留着只会把 `make sure` 当成构建。
fn run_command_is_build(content: &str) -> bool {
    run_command_invocation(content).is_some_and(|inv| command_line_looks_like_verify(&inv))
}

/// 取 `crabmate_tool.structured_payload.invocation`（完整命令行）。
///
/// 空串等同「没有 invocation」：与 `evidence.rs` 同口径，
/// 否则「字段存在但解析为空」的消息会走「结构化判定为假」而**不**回落到正文。
fn run_command_invocation(raw: &str) -> Option<String> {
    let env = normalize_tool_message_content(raw)?;
    env.structured_payload
        .as_ref()?
        .get("invocation")?
        .as_str()
        .map(str::to_string)
        .filter(|s| !s.trim().is_empty())
}

fn format_auto_experience(user: &str, assistant: &str, recovery: &BuildRecovery) -> Option<String> {
    let user_line = one_line_excerpt(user, 120);
    let asst_line = one_line_excerpt(assistant, 200);
    let text = format!(
        "【经验·自动】问题：{user_line}。经工具 {} 验证已由失败转为成功。处理要点：{asst_line}",
        recovery.tool_name
    );
    Some(text)
}

fn one_line_excerpt(s: &str, max_chars: usize) -> String {
    let flat: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max_chars {
        return flat;
    }
    flat.chars().take(max_chars).collect::<String>() + "…"
}

fn tags_for_recovery(recovery: &BuildRecovery) -> Vec<String> {
    let mut tags = vec!["auto".to_string(), "build-recovery".to_string()];
    let n = recovery.tool_name.as_str();
    // 工具名是**结构化字段**，用精确匹配而非子串：`contains("test")` 会把任何含 `test`
    // 的工具名都算成测试工具（#1438）。
    if matches!(
        n,
        tool_names::RUN_COMMAND
            | tool_names::CARGO_CHECK
            | tool_names::CARGO_TEST
            | tool_names::CARGO_CLIPPY
    ) {
        tags.push("rust".to_string());
    }
    if matches!(n, tool_names::CARGO_TEST | tool_names::PYTEST_RUN) {
        tags.push("test".to_string());
    }
    tags
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cm_types::Message;

    fn user_msg(text: &str) -> Message {
        Message::user_only(text)
    }

    fn tool_msg(name: &str, body: &str) -> Message {
        Message::tool_result_named("tc1".to_string(), name.to_string(), body.to_string())
    }

    fn asst_final(text: &str) -> Message {
        Message::assistant_only(text)
    }

    /// `run_command` 的**真实信封**形状（含 `structured_payload.invocation`）。
    ///
    /// #1440 K7 之后正文兜底已删除，凡是要被测成「构建」的 `run_command` 消息
    /// 都必须带这个块——否则就是「无结构化信号」，一律不算构建。
    fn run_command_msg(invocation: &str, output: &str, ok: bool, exit_code: i64) -> Message {
        tool_msg(
            "run_command",
            &serde_json::json!({
                "crabmate_tool": {
                    "name": "run_command",
                    "summary": invocation,
                    "output": output,
                    "ok": ok,
                    "exit_code": exit_code,
                    "structured_payload": {
                        "kind": "crabmate_structured_payload",
                        "tool": "run_command",
                        "version": 1,
                        "schema": "run_command_exit_v1",
                        "invocation": invocation,
                        "ok": ok,
                        "exit_code": exit_code,
                    }
                }
            })
            .to_string(),
        )
    }

    /// fixture 用**真实**形状：`cargo_*` 工具输出首行是 `{title} (exit=N):`
    /// （见 `cm_tools::tool_result` 的 `subprocess_exit_v1` 用例），不是 `退出码：N`。
    /// 早期这里写的是 `cargo check 完成\n退出码：0`——那种形状现实里不存在，
    /// 于是「这条到底算成功还是判不出来」从来没被真正问过。
    #[test]
    fn detects_cargo_check_recovery() {
        let messages = vec![
            user_msg("请修复 cargo check 编译错误"),
            tool_msg(
                "cargo_check",
                "cargo check (exit=101):\nstderr: error[E0425]",
            ),
            tool_msg("cargo_check", "cargo check (exit=0):\n(check output)"),
            asst_final("已修复未使用的导入，cargo check 通过。"),
        ];
        let draft = draft_auto_experience_from_turn(&messages);
        assert!(draft.is_some());
        let (exp, tags) = draft.unwrap();
        assert!(exp.contains("cargo_check"));
        assert!(exp.contains("编译错误") || exp.contains("修复"));
        assert!(tags.iter().any(|t| t == "rust"));
    }

    #[test]
    fn skips_without_prior_failure() {
        let messages = vec![
            user_msg("运行 cargo test 确认一下"),
            tool_msg("cargo_test", "cargo test (exit=0):\n全部通过"),
            asst_final("测试已通过。"),
        ];
        assert!(draft_auto_experience_from_turn(&messages).is_none());
    }

    #[test]
    fn run_command_build_recovery() {
        let messages = vec![
            user_msg("fix the build failure please"),
            run_command_msg("cargo build", "error: could not compile", false, 101),
            run_command_msg("cargo build", "Finished", true, 0),
            asst_final("Fixed the type mismatch in lib.rs."),
        ];
        assert!(draft_auto_experience_from_turn(&messages).is_some());
    }

    /// 用户句**不含**任何构建/故障词时也要能沉淀——旧关键词表会在这里拒绝。
    ///
    /// 区分度：旧实现返回 `None`（`KEYS` 全不命中），新实现返回 `Some`。
    /// 「这一轮真的失败后修好了」由 `cargo_check` 的 exit=101 → exit=0 结构化事实证明。
    #[test]
    fn detects_recovery_without_build_words_in_user_message() {
        let messages = vec![
            user_msg("帮我把这个整理一下可以吗"),
            tool_msg(
                "cargo_check",
                "cargo check (exit=101):\nstderr: error[E0425]",
            ),
            tool_msg("cargo_check", "cargo check (exit=0):\n(check output)"),
            asst_final("已修好那个未使用的导入。"),
        ];
        assert!(
            draft_auto_experience_from_turn(&messages).is_some(),
            "不应再依赖用户句关键词"
        );
    }

    /// `run_command` 是否算构建，以结构化 `invocation` 为准，不再扫输出正文。
    ///
    /// 区分度：旧实现扫正文，会把 `grep -rn build src` 判成构建、把 invocation 为
    /// `cargo build` 但正文不含构建词的正常输出漏掉。
    #[test]
    fn run_command_build_judged_by_invocation_not_output_text() {
        let build_inv = r#"{"crabmate_tool":{"v":1,"name":"run_command","summary":"cargo build","ok":true,"exit_code":0,"output":"done","structured_payload":{"invocation":"cargo build"}}}"#;
        assert!(
            run_command_is_build(build_inv),
            "invocation 是构建命令即应判构建（与正文无关）"
        );

        let grep_inv = r#"{"crabmate_tool":{"v":1,"name":"run_command","summary":"grep -rn build src","ok":true,"exit_code":0,"output":"build target 文档","structured_payload":{"invocation":"grep -rn build src"}}}"#;
        assert!(
            !run_command_is_build(grep_inv),
            "正文提到 build 但 invocation 不是构建，不应命中"
        );

        // 无信封（无 `structured_payload.invocation`）：一律不算构建，不再从正文反推（#1440 K7）。
        assert!(!run_command_is_build("命令：cargo build\n退出码：101"));
    }
}
