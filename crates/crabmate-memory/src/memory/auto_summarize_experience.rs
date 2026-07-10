//! 回合结束后启发式检测「由失败到成功」的构建/验证类工具调用，自动写入长期记忆（`source_role=auto_summarize_experience`）。

use crabmate_tools::tool_result::tool_message_content_ok_for_model;
use crabmate_types::{Message, message_content_as_str};

/// 检测本回合是否值得自动沉淀；返回经验正文与标签。
pub fn draft_auto_experience_from_turn(messages: &[Message]) -> Option<(String, Vec<String>)> {
    let turn_start = last_non_injection_user_index(messages)?;
    let turn = &messages[turn_start..];
    let (user_text, assistant_text) = last_user_assistant_final_pair(turn)?;
    if !user_message_suggests_task(user_text) {
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
        if crabmate_types::is_long_term_memory_injection(m)
            || crabmate_types::is_workspace_changelist_injection(m)
        {
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
    crate::memory::long_term_memory::last_user_assistant_final_pair_for_turn(messages)
}

fn user_message_suggests_task(user: &str) -> bool {
    if user.chars().count() < 8 {
        return false;
    }
    let lower = user.to_ascii_lowercase();
    const KEYS: &[&str] = &[
        "修复", "编译", "构建", "测试", "错误", "失败", "fix", "error", "failed", "build", "test",
        "clippy", "cargo", "rust", "配置", "config", "debug", "bug", "报错",
    ];
    KEYS.iter().any(|k| lower.contains(k))
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
        if tool_name == "run_command" && !run_command_looks_like_build(content) {
            continue;
        }
        let ok = tool_message_content_ok_for_model(content, tool_name);
        if !ok {
            saw_build_failure = true;
            continue;
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
        "run_command"
            | "cargo_check"
            | "cargo_test"
            | "cargo_clippy"
            | "cargo_fmt_check"
            | "cargo_fix"
            | "cargo_nextest"
            | "format_check_file"
            | "cargo_audit"
    )
}

fn run_command_looks_like_build(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    lower.contains("cargo ")
        || lower.contains("rustc")
        || lower.contains("cmake")
        || lower.contains(" make ")
        || lower.contains("npm ")
        || lower.contains("ctest")
        || lower.contains("mvn ")
        || lower.contains("gradle")
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
    if n.contains("cargo") || n == "run_command" {
        tags.push("rust".to_string());
    }
    if n.contains("test") {
        tags.push("test".to_string());
    }
    tags
}

#[cfg(test)]
mod tests {
    use super::*;
    use crabmate_types::Message;

    fn user_msg(text: &str) -> Message {
        Message::user_only(text)
    }

    fn tool_msg(name: &str, body: &str) -> Message {
        Message {
            role: "tool".to_string(),
            content: Some(crabmate_types::MessageContent::Text(body.to_string())),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: Some(name.to_string()),
            tool_call_id: Some("tc1".to_string()),
        }
    }

    fn asst_final(text: &str) -> Message {
        Message::assistant_only(text)
    }

    #[test]
    fn detects_cargo_check_recovery() {
        let messages = vec![
            user_msg("请修复 cargo check 编译错误"),
            tool_msg(
                "cargo_check",
                "cargo check 失败\n退出码：101\nstderr: error[E0425]",
            ),
            tool_msg("cargo_check", "cargo check 完成\n退出码：0\n"),
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
            tool_msg("cargo_test", "退出码：0\n全部通过"),
            asst_final("测试已通过。"),
        ];
        assert!(draft_auto_experience_from_turn(&messages).is_none());
    }

    #[test]
    fn run_command_build_recovery() {
        let fail = "命令：cargo build\n退出码：101\nerror: could not compile";
        let ok = "命令：cargo build\n退出码：0\nFinished";
        let messages = vec![
            user_msg("fix the build failure please"),
            tool_msg("run_command", fail),
            tool_msg("run_command", ok),
            asst_final("Fixed the type mismatch in lib.rs."),
        ];
        assert!(draft_auto_experience_from_turn(&messages).is_some());
    }
}
