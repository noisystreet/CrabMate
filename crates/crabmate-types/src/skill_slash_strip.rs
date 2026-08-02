//! 显式 `/<skill-id>`：出站给模型时剥离前缀（会话存盘可保留原文供 UI）。

/// REPL / Web / TUI 内建斜杠命令词头（小写、无 `/`），不可当作 skill id。
/// 与 `crabmate_config::skills_slash::is_reserved_slash_head` / 前端展示侧对齐。
#[must_use]
pub fn is_reserved_slash_head(head: &str) -> bool {
    matches!(
        head.to_ascii_lowercase().as_str(),
        "?" | "agent"
            | "api-base"
            | "api-key"
            | "apikey"
            | "apibase"
            | "branch"
            | "cd"
            | "clear"
            | "config"
            | "conv"
            | "doctor"
            | "export"
            | "help"
            | "mcp"
            | "model"
            | "models"
            | "probe"
            | "save-session"
            | "skill"
            | "skills"
            | "tools"
            | "version"
            | "workspace"
    )
}

fn looks_like_skill_id(head: &str) -> bool {
    !head.is_empty()
        && head
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// 去掉显式 `/<skill-id> [任务]` 前缀，供供应商出站 / 模型上下文使用。
///
/// - **不**解析 skill 是否存在（历史消息里 skill 可能已删除，仍应剥离）。
/// - 保留词、非 skill 形 id、非 `/` 开头：原样返回。
/// - 仅 `/id` 无任务时返回默认任务句（与 config `default_skill_user_task` 一致）。
#[must_use]
pub fn strip_explicit_skill_slash_prefix_for_model(raw: &str) -> String {
    let trimmed = raw.trim();
    let Some(rest) = trimmed.strip_prefix('/') else {
        return raw.to_string();
    };
    let rest = rest.trim_start();
    if rest.is_empty() {
        return raw.to_string();
    }
    let mut parts = rest.splitn(2, char::is_whitespace);
    let head = parts.next().unwrap_or("").trim();
    let task = parts.next().unwrap_or("").trim();
    if head.is_empty() || is_reserved_slash_head(head) || !looks_like_skill_id(head) {
        return raw.to_string();
    }
    if task.is_empty() {
        "请按该技能执行。".to_string()
    } else {
        // 保留原文在 `/id` 之后的缩进/换行形态：用 trim 后的 task；
        // 若 raw 在 skill 行后还有展开附录，task 已含附录（splitn 2）。
        task.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_skill_with_task() {
        assert_eq!(
            strip_explicit_skill_slash_prefix_for_model("/rust-style 分析一下"),
            "分析一下"
        );
    }

    #[test]
    fn strips_skill_keeps_file_expand_appendix() {
        let raw = "/code-review 看这个\n\n---\n**工作区文件引用**\n";
        let out = strip_explicit_skill_slash_prefix_for_model(raw);
        assert!(out.starts_with("看这个"));
        assert!(out.contains("工作区文件引用"));
        assert!(!out.contains("/code-review"));
    }

    #[test]
    fn id_only_uses_default_task() {
        assert_eq!(
            strip_explicit_skill_slash_prefix_for_model("/code-review"),
            "请按该技能执行。"
        );
    }

    #[test]
    fn reserved_and_plain_unchanged() {
        assert_eq!(
            strip_explicit_skill_slash_prefix_for_model("/skills list"),
            "/skills list"
        );
        assert_eq!(
            strip_explicit_skill_slash_prefix_for_model("普通问题"),
            "普通问题"
        );
        assert_eq!(strip_explicit_skill_slash_prefix_for_model("/a/b"), "/a/b");
    }
}
