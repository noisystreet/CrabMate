//! 飞书 **`chat_id` → CrabMate `POST /workspace`** 路径展开。

/// 将模板中的 **`{chat_id}`** 替换为飞书会话 id；并 **trim**。模板须由运维配置在 **`workspace_allowed_roots`** 允许范围内。
pub fn expand_workspace_root_template(template: &str, chat_id: &str) -> String {
    template.replace("{chat_id}", chat_id).trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_chat_id() {
        assert_eq!(
            expand_workspace_root_template("/data/ws/{chat_id}/repo", "oc_abc"),
            "/data/ws/oc_abc/repo"
        );
    }
}
