//! 用户气泡：将 `/<skill-id> [任务]` 展示为内联「使用 xxx skill」样式（存盘仍保留原文）。

/// 与后端 `crabmate_config::skills_slash::is_reserved_slash_head` 对齐（展示侧勿把内建命令当成 skill）。
fn is_reserved_slash_head(head: &str) -> bool {
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

/// 解析用户消息中的显式 skill 斜杠：`/<id> [任务]`。
///
/// 仅当 id 非空、非保留词、且像 skill id（字母数字 / `_` / `-`）时命中。
#[must_use]
pub fn parse_user_skill_slash(raw: &str) -> Option<(String, String)> {
    let s = raw.trim();
    let rest = s.strip_prefix('/')?;
    if rest.is_empty() {
        return None;
    }
    let mut parts = rest.splitn(2, char::is_whitespace);
    let head = parts.next()?.trim();
    if head.is_empty() || is_reserved_slash_head(head) {
        return None;
    }
    if !head
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return None;
    }
    let task = parts.next().unwrap_or("").trim().to_string();
    Some((head.to_string(), task))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_skill_with_task() {
        let (id, task) = parse_user_skill_slash("/rust-style 分析一下").unwrap();
        assert_eq!(id, "rust-style");
        assert_eq!(task, "分析一下");
    }

    #[test]
    fn parse_skill_id_only() {
        let (id, task) = parse_user_skill_slash("/code-review").unwrap();
        assert_eq!(id, "code-review");
        assert!(task.is_empty());
    }

    #[test]
    fn reserved_not_skill() {
        assert!(parse_user_skill_slash("/skills list").is_none());
        assert!(parse_user_skill_slash("/help").is_none());
        assert!(parse_user_skill_slash("/workspace").is_none());
    }

    #[test]
    fn plain_text_not_skill() {
        assert!(parse_user_skill_slash("rust-style 分析").is_none());
        assert!(parse_user_skill_slash("/a/b").is_none());
    }
}
