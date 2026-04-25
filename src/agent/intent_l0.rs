//! L0 预处理与特征：多轮续接合并、文本启发式、近期 **tool** 条成败（不记录参数正文）。
//!
//! 与 `docs/design/intent_recognition_enhancement.md` 的 L0 节对齐（轻量实现，无独立 ML）。

const SHORT_UTTERANCE_MAX_CHARS: usize = 40;
const MERGED_MAX_CHARS: usize = 2000;

/// 可观测的 L0 特征，供 L1 提级与 `IntentMergeMeta`。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct IntentL0Snapshot {
    /// 出现路径/工作区/文件或 `@` 引用等信号。
    pub has_file_path_like: bool,
    /// 出现报错、trace、panic 等信号。
    pub has_error_signal: bool,
    /// 近邻（当前 user 前）存在 `role: tool` 且非成功态。
    pub has_recent_tool_failure: bool,
    /// 是否偏短、可能指代不足（启发式，非严格「字数」产品定义）。
    pub is_short: bool,
    /// 出现 `git` / 分支 / 提交 等（大小写不敏感，git 为 ASCII）。
    pub has_git_keyword: bool,
    /// 出现 `cargo` / `npm` / `pnpm` 等包管理器命令痕迹。
    pub has_command_cargo: bool,
}

/// 在「续接短句 + 前序在澄清/确认」时，将最近用户句与当前句拼成**路由用**文本，降低指代失败。
/// 返回 `(路由文本, 是否发生了续接合并)`；不修改原始 `current_task`。
pub fn effective_intent_routing_text(
    current_task: &str,
    in_clarification_flow: bool,
    recent_user_messages: &[String],
) -> (String, bool) {
    let t = current_task.trim();
    if t.is_empty() {
        return (String::new(), false);
    }
    if !in_clarification_flow {
        return (t.to_string(), false);
    }
    if t.chars().count() > SHORT_UTTERANCE_MAX_CHARS {
        return (t.to_string(), false);
    }
    if has_substantive_execute_leverage(t) {
        return (t.to_string(), false);
    }
    if recent_user_messages.is_empty() {
        return (t.to_string(), false);
    }
    let prior: String = recent_user_messages
        .iter()
        .take(2)
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if prior.is_empty() {
        return (t.to_string(), false);
    }
    let mut merged = format!("[前序用户]\n{prior}\n[当前续接]\n{t}");
    if merged.chars().count() > MERGED_MAX_CHARS {
        let tail: String = merged
            .chars()
            .rev()
            .take(MERGED_MAX_CHARS)
            .collect::<String>();
        merged = tail.chars().rev().collect();
    }
    (merged, true)
}

/// 有路径、扩展名、明确改动动词等，视为无需与前句拼接。
fn has_substantive_execute_leverage(s: &str) -> bool {
    let n = s.to_lowercase();
    n.contains('/')
        || n.contains('@')
        || n.contains(".rs")
        || n.contains(".ts")
        || n.contains(".md")
        || n.contains("改")
        || n.contains("修")
        || n.contains("实现")
        || n.contains("删除")
        || n.contains("重构")
        || n.contains("commit")
        || n.contains("cargo")
        || n.contains("test")
        || n.contains("npm")
        || n.contains("pnpm")
}

/// 为合并后的 `routing` 文本计算 L0 信号（**不含** tool 位，等价于 `l0_snapshot_merged(routing, false)`）。
pub fn l0_snapshot_from_merged_routing(routing: &str) -> IntentL0Snapshot {
    l0_snapshot_merged(routing, false)
}

/// 对合并/当前路由正文的**文本**子串特征（不含 tool 位）。
fn text_flags_from_routing(routing: &str) -> IntentL0Snapshot {
    let n = routing.to_lowercase();
    let has_file_path_like = n.contains('/')
        || n.contains('@')
        || n.contains("src/")
        || n.contains(".rs")
        || n.contains(".ts")
        || n.contains(".md")
        || n.contains("目录")
        || n.contains("文件");
    let has_error_signal = n.contains("error")
        || n.contains("panic")
        || n.contains("traceback")
        || n.contains("stack")
        || n.contains("失败")
        || n.contains("异常")
        || n.contains("报了")
        || n.contains("bug");
    let is_short = routing.chars().count() <= SHORT_UTTERANCE_MAX_CHARS && !n.contains("前序用户");
    let has_git_keyword = n.contains("git")
        || n.contains("pr")
        || n.contains("rebase")
        || n.contains("cherry")
        || n.contains("commit")
        || n.contains("分支")
        || n.contains("合并");
    let has_command_cargo = n.contains("cargo")
        || n.contains("npm")
        || n.contains("pnpm")
        || n.contains("pytest")
        || n.contains("cmake");
    IntentL0Snapshot {
        has_file_path_like,
        has_error_signal,
        has_recent_tool_failure: false,
        is_short,
        has_git_keyword,
        has_command_cargo,
    }
}

/// 合并**路由文本**子串 + **近期 tool 是否失败**（不解析完整 JSON 正文，依赖 normalize）。
pub fn l0_snapshot_merged(merged_routing: &str, has_recent_tool_failure: bool) -> IntentL0Snapshot {
    let mut s = text_flags_from_routing(merged_routing);
    s.has_recent_tool_failure = has_recent_tool_failure;
    if has_recent_tool_failure {
        s.has_error_signal = true;
    }
    s
}

/// 自当前 user 条**之前**、自尾向前，若存在 `ok: false` 的 `crabmate_tool` 则视为需跟进。最多扫 `max_tail` 条 `Message`。
pub fn messages_have_recent_tool_failure(
    messages: &[crate::types::Message],
    max_tail: usize,
) -> bool {
    use crate::tool_result::normalize_tool_message_content;
    for m in messages.iter().rev().take(max_tail) {
        if m.role != "tool" {
            continue;
        }
        let Some(t) = crate::types::message_content_as_str(&m.content) else {
            continue;
        };
        if let Some(env) = normalize_tool_message_content(t) {
            if !env.ok {
                return true;
            }
        } else {
            // 无信封时的保守启发式（不依赖具体供应商）
            let low = t.to_lowercase();
            if low.contains("\"ok\":false")
                || (low.contains("ok") && (low.contains("error_code") || low.contains("失败")))
            {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    #[test]
    fn merged_continuation_includes_prior_in_clarification() {
        let (s, merged) = super::effective_intent_routing_text(
            "好的，就这样做",
            true,
            &["在 src/foo.rs 里加日志".to_string()],
        );
        assert!(merged);
        assert!(s.contains("src/foo"));
        assert!(s.contains("好的，就这样做"));
    }

    #[test]
    fn no_merge_without_clarification_flag() {
        let (s, merged) = super::effective_intent_routing_text("短", false, &["上文".to_string()]);
        assert!(!merged);
        assert_eq!(s, "短");
    }

    #[test]
    fn l0_detects_error_signal() {
        let s = super::l0_snapshot_from_merged_routing("cargo test 报 error: 找不到模块");
        assert!(s.has_error_signal);
    }

    #[test]
    fn l0_merged_sets_tool_flag() {
        let s = super::l0_snapshot_merged("继续改", true);
        assert!(s.has_recent_tool_failure);
        assert!(s.has_error_signal);
    }
}
