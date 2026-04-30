//! 反思验收清洗、Manager 输出类型、失败聚合与测试。

use super::super::task::{ExecutionStrategy, SubGoal, TaskResult, TaskStatus};

/// 反思产出的 `acceptance` 与 `GoalVerifier::run_verify_command`（无 shell 的 argv）对齐：去路径穿越、
/// 去 shell 元字符；`expect_file_exists` 规范为可拼进工作区根的相对路径。
pub fn sanitize_reflection_acceptance(
    acc: Option<super::super::task::GoalAcceptance>,
    _workspace_root: &std::path::Path,
) -> (Option<super::super::task::GoalAcceptance>, bool) {
    let mut dropped_cmd = false;
    let Some(mut a) = acc else {
        return (None, false);
    };

    let mut paths = Vec::new();
    for p in std::mem::take(&mut a.expect_file_exists) {
        if p.trim().is_empty() {
            continue;
        }
        if is_unsafe_path_segment(&p) {
            log::debug!(
                target: "crabmate",
                "[HIERARCHICAL] Manager: skipping unsafe expect_file_exists: {}",
                truncate_for_log(&p, 80)
            );
            continue;
        }
        // 规范化为相对、POSIX 式斜杠，便于与 GoalVerifier 拼接
        let cleaned = p.trim().trim_start_matches('/').to_string();
        if !cleaned.is_empty() {
            paths.push(cleaned);
        }
    }
    a.expect_file_exists = paths;

    if let Some(ref cmd) = a.expect_command_success
        && is_unsafe_verify_command(cmd)
    {
        log::warn!(
            target: "crabmate",
            "[HIERARCHICAL] Manager: expect_command_success rejected, clearing: {}",
            truncate_for_log(cmd, 200)
        );
        a.expect_command_success = None;
        dropped_cmd = true;
    }

    if a.expect_file_exists.is_empty()
        && a.expect_output_contains.is_empty()
        && a.expect_stdout_contains.is_none()
        && a.expect_stderr_contains.is_none()
        && a.expect_json_path_equals.is_none()
        && a.expect_http_status.is_none()
        && a.expect_command_success.is_none()
        && a.expect_exit_code.is_none()
    {
        (None, dropped_cmd)
    } else {
        (Some(a), dropped_cmd)
    }
}

pub fn is_unsafe_path_segment(s: &str) -> bool {
    use std::path::{Component, Path};

    if s.starts_with('/') {
        return true;
    }
    if s.contains("..") {
        return true;
    }
    let p = Path::new(s);
    for c in p.components() {
        if c == Component::ParentDir {
            return true;
        }
        if let Component::RootDir = c {
            return true;
        }
    }
    // 疑似 `C:...` / `C:\` 等 Windows 前缀（本验收为工作区相对路径）
    s.len() >= 2 && s.as_bytes()[0].is_ascii_alphabetic() && s.as_bytes()[1] == b':'
}

/// 与 `run_verify_command`（`split_whitespace` + 无 shell）一致；禁止 shell 注入与 `..` 实参
pub fn is_unsafe_verify_command(cmd: &str) -> bool {
    if cmd.is_empty() {
        return true;
    }
    let t = cmd.trim();
    for ch in [
        ';', '&', '|', '>', '<', '\n', '\r', '\t', '`', '$', '(', ')', '*', '?', '[', ']',
    ] {
        if t.contains(ch) {
            return true;
        }
    }
    let parts: Vec<&str> = t.split_whitespace().collect();
    if parts.len() >= 2
        && parts[1] == "-c"
        && matches!(
            parts[0].to_lowercase().as_str(),
            "sh" | "bash" | "dash" | "zsh" | "cmd" | "ksh" | "fish" | "pwsh"
        )
    {
        return true;
    }
    for w in t.split_whitespace() {
        if w.contains("..") {
            return true;
        }
        if w == "/" || w.starts_with('/') {
            return true;
        }
        if w == "\\" {
            return true;
        }
        if w.contains(':')
            && w.len() == 2
            && let Some(first) = w.chars().next()
            && w.ends_with(':')
            && first.is_ascii_alphabetic()
        {
            return true;
        }
    }
    // 过长的单条可能是不慎粘贴的 shell 串
    t.chars().count() > 512
}

/// Manager 输出
#[derive(Debug, Clone)]
pub struct ManagerOutput {
    /// 分解的子目标列表
    pub sub_goals: Vec<SubGoal>,
    /// 执行策略
    pub execution_strategy: ExecutionStrategy,
    /// 给用户的结果摘要
    pub summary: String,
}

/// 失败处理决策
#[derive(Debug, Clone)]
pub enum FailureDecision {
    Continue,
    Retry { goal_id: String },
    Skip { goal_id: String, reason: String },
    Replan { reason: String },
    Abort { reason: String },
}

/// 处理失败
pub fn handle_failure(
    results: &[TaskResult],
    max_failures: usize,
) -> (Vec<&TaskResult>, Vec<&TaskResult>, FailureDecision) {
    let completed: Vec<&TaskResult> = results
        .iter()
        .filter(|r| matches!(r.status, TaskStatus::Completed))
        .collect();

    let failed: Vec<&TaskResult> = results
        .iter()
        .filter(|r| matches!(r.status, TaskStatus::Failed { .. }))
        .collect();

    let decision = if failed.is_empty() {
        FailureDecision::Continue
    } else if failed.len() > max_failures {
        FailureDecision::Abort {
            reason: format!("{} failures exceeded threshold", failed.len()),
        }
    } else {
        FailureDecision::Continue
    };

    (completed, failed, decision)
}

/// 尝试修复常见的 LLM JSON 格式错误
pub fn fix_common_json_errors(content: &str) -> String {
    let mut fixed = content.to_string();

    // 修复 Perl/Ruby 风格的 => 语法为标准的 JSON :
    // 但需要注意不要破坏 URL 中的 => 或合法的字符串内容
    // 使用正则替换：在非字符串上下文中将 => 替换为 :
    let re = regex::Regex::new(r"(\w+)\s*=>\s*").unwrap();
    fixed = re.replace_all(&fixed, r#"$1": "#).to_string();

    // 修复单引号为双引号（如果整个 JSON 使用单引号）
    // 注意：这可能会破坏包含单引号的字符串，需要谨慎
    // 只在检测到 JSON 以单引号开始时替换
    if fixed.trim().starts_with("'") {
        fixed = fixed.replace("'", "\"");
    }

    fixed
}

/// 截断任务字符串用于日志（按字符边界截断，支持中文）
pub fn truncate_task(task: &str) -> String {
    truncate_for_log(task, 100)
}

/// 按字符边界截断字符串（支持中文），用于日志输出
pub fn truncate_for_log(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let truncated: String = s.chars().take(max_chars - 3).collect();
    format!("{}...", truncated)
}

#[cfg(test)]
mod tests {
    use super::super::super::task::GoalAcceptance;
    use super::*;

    #[test]
    fn test_sanitize_reflection_acceptance_paths_and_cmd() {
        let acc = Some(GoalAcceptance {
            expect_file_exists: vec!["/etc/passwd".to_string(), "ok/rel".to_string()],
            expect_command_success: Some("rm -rf /".to_string()),
            expect_output_contains: vec![],
            expect_exit_code: None,
            ..Default::default()
        });
        let (out, dropped) = sanitize_reflection_acceptance(acc, std::path::Path::new("/tmp/ws"));
        assert!(dropped);
        let g = out.expect("some");
        assert_eq!(g.expect_file_exists, vec!["ok/rel".to_string()]);
        assert!(g.expect_command_success.is_none());
    }

    #[test]
    fn test_is_unsafe_verify_command_allows_simple_argv() {
        assert!(!is_unsafe_verify_command("test -f build/foo"));
        assert!(is_unsafe_verify_command("sh -c echo"));
    }
}
