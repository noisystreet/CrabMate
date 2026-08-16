//! Markdown 导出：角色标题、标题行、按 role/body 组装；可选对完整 [`Message`] 重排。

use crate::cm_types::Message;

/// Markdown 导出语言（与 Web `Locale` / CLI 默认中文对齐）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportMdLocale {
    ZhHans,
    En,
}

pub fn export_md_title_full(l: ExportMdLocale) -> &'static str {
    match l {
        ExportMdLocale::ZhHans => "# CrabMate 聊天记录\n\n",
        ExportMdLocale::En => "# CrabMate chat export\n\n",
    }
}

pub fn export_md_title_selection(l: ExportMdLocale) -> &'static str {
    match l {
        ExportMdLocale::ZhHans => "# CrabMate 聊天记录（已选消息）\n\n",
        ExportMdLocale::En => "# CrabMate chat export (selected messages)\n\n",
    }
}

pub fn export_md_heading_user(l: ExportMdLocale) -> &'static str {
    match l {
        ExportMdLocale::ZhHans => "## 用户",
        ExportMdLocale::En => "## User",
    }
}

pub fn export_md_heading_assistant(l: ExportMdLocale) -> &'static str {
    match l {
        ExportMdLocale::ZhHans => "## 助手",
        ExportMdLocale::En => "## Assistant",
    }
}

pub fn export_md_heading_tool(l: ExportMdLocale) -> &'static str {
    match l {
        ExportMdLocale::ZhHans => "## 工具",
        ExportMdLocale::En => "## Tool",
    }
}

pub fn export_md_heading_other(l: ExportMdLocale) -> &'static str {
    match l {
        ExportMdLocale::ZhHans => "## 其它",
        ExportMdLocale::En => "## Other",
    }
}

pub fn export_md_heading_timeline(l: ExportMdLocale) -> &'static str {
    match l {
        ExportMdLocale::ZhHans => "## 时间线",
        ExportMdLocale::En => "## Timeline",
    }
}

fn heading_for_role(role: &str, loc: ExportMdLocale) -> &'static str {
    match role {
        "user" => export_md_heading_user(loc),
        "assistant" => export_md_heading_assistant(loc),
        "tool" => export_md_heading_tool(loc),
        "system" => export_md_heading_timeline(loc),
        _ => export_md_heading_other(loc),
    }
}

/// 追加单个角色分段（跳过调用方已过滤的 `system` 时仍可传入其它角色）。
pub fn append_markdown_role_section(md: &mut String, role: &str, body: &str, loc: ExportMdLocale) {
    if role == "system" {
        return;
    }
    md.push_str(heading_for_role(role, loc));
    md.push_str("\n\n");
    md.push_str(body);
    md.push_str("\n\n");
}

/// 由 `(role, body)` 迭代组装全文（含标题）；跳过 `system`。
pub fn markdown_from_role_bodies<'a, I>(title: &str, items: I, loc: ExportMdLocale) -> String
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    let mut md = String::from(title);
    for (role, body) in items {
        append_markdown_role_section(&mut md, role, body, loc);
    }
    md
}

/// 按对话流重排消息：将工具消息插入到对应的助手消息之后。
///
/// Agent 主循环的消息追加顺序是：
/// 1. 助手消息（带 tool_calls）追加到 messages
/// 2. 所有工具结果依次追加（tool, tool, tool...）
/// 3. 下一条助手消息追加
///
/// 这导致 messages 数组中工具消息聚集在前面，助手消息聚集在后面。
/// 本函数按对话流重排：将每个工具消息移动到其对应助手消息之后。
pub fn reorder_messages_for_conversation_flow(messages: Vec<Message>) -> Vec<Message> {
    let mut result: Vec<Message> = Vec::with_capacity(messages.len());
    let mut tool_calls_pending: Vec<Message> = Vec::new();

    for m in messages {
        match m.role.as_str() {
            "assistant" => {
                result.append(&mut tool_calls_pending);
                result.push(m);
            }
            "tool" => {
                tool_calls_pending.push(m);
            }
            "user" => {
                result.append(&mut tool_calls_pending);
                result.push(m);
            }
            _ => {
                result.append(&mut tool_calls_pending);
                result.push(m);
            }
        }
    }
    result.append(&mut tool_calls_pending);
    result
}

/// 完整 [`Message`] 列表 → Markdown：先重排，再跳过 `system`；正文由 `body_for` 提供。
pub fn messages_to_markdown_with_body<F>(
    messages: &[Message],
    loc: ExportMdLocale,
    mut body_for: F,
) -> String
where
    F: FnMut(&Message) -> String,
{
    let reordered = reorder_messages_for_conversation_flow(messages.to_vec());
    let mut md = String::from(export_md_title_full(loc));
    for m in &reordered {
        if m.role == "system" {
            continue;
        }
        let body = body_for(m);
        append_markdown_role_section(&mut md, m.role.as_str(), &body, loc);
    }
    md
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cm_types::Message;

    fn msg(role: &str, content: &str) -> Message {
        Message {
            role: role.to_string(),
            content: Some(content.into()),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        }
    }

    fn plain_body(m: &Message) -> String {
        crate::cm_types::message_content_as_str(&m.content)
            .unwrap_or("")
            .to_string()
    }

    #[test]
    fn markdown_skips_system_and_labels_roles() {
        let md = messages_to_markdown_with_body(
            &[
                msg("system", "sys"),
                msg("user", "hi"),
                msg("assistant", "hey"),
                msg("tool", "out"),
            ],
            ExportMdLocale::ZhHans,
            plain_body,
        );
        assert!(!md.contains("sys"));
        assert!(md.contains("## 用户"));
        assert!(md.contains("hi"));
        assert!(md.contains("## 助手"));
        assert!(md.contains("## 工具"));
        assert!(md.contains("out"));
    }

    #[test]
    fn markdown_reorders_tool_after_assistant() {
        let messages = vec![
            msg("assistant", "开始执行"),
            msg("tool", "解压缩结果"),
            msg("tool", "list_tree 结果"),
            msg("assistant", "已解压。看看目录结构..."),
        ];
        let md = messages_to_markdown_with_body(&messages, ExportMdLocale::ZhHans, plain_body);
        let assistant_pos = md.find("## 助手").unwrap();
        let tool_pos = md.find("## 工具").unwrap();
        let second_assistant_pos =
            md[assistant_pos + 10..].find("## 助手").unwrap() + assistant_pos + 10;
        assert!(
            assistant_pos < tool_pos && tool_pos < second_assistant_pos,
            "工具消息应该在两个助手消息之间"
        );
    }

    #[test]
    fn en_headings() {
        let md = markdown_from_role_bodies(
            export_md_title_full(ExportMdLocale::En),
            [("user", "hi"), ("assistant", "yo")],
            ExportMdLocale::En,
        );
        assert!(md.contains("# CrabMate chat export"));
        assert!(md.contains("## User"));
        assert!(md.contains("## Assistant"));
    }
}
