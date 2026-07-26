//! 会话导出落盘：复用 [`crabmate_chat_export`] 的 schema / Markdown；本模块仅负责写
//! `<workspace>/.crabmate/exports/`。Web / Tauri 见 `frontend/src/session_export.rs`。

use crabmate_types::Message;
use std::io;
use std::path::{Path, PathBuf};

pub use crabmate_chat_export::{
    CHAT_EXPORT_SCHEMA_ID, CHAT_EXPORT_SCHEMA_VERSION, CHAT_SESSION_FILE_VERSION, ChatSessionFile,
    ExportMdLocale, session_to_json_pretty,
};

/// 与 TUI `/export` / Web 导出一致：跳过 `system`；`tool` 与 `assistant`/`user` 分段输出。
/// CLI/TUI 默认中文标题；助手正文经 [`crate::message_display::assistant_raw_markdown_body_for_message`]。
fn messages_to_markdown(messages: &[Message]) -> String {
    crabmate_chat_export::messages_to_markdown_with_body(messages, ExportMdLocale::ZhHans, |m| {
        if m.role == "assistant" {
            crate::message_display::assistant_raw_markdown_body_for_message(m)
        } else {
            crabmate_types::message_content_as_str(&m.content)
                .unwrap_or("")
                .to_string()
        }
    })
}

/// `<workspace>/.crabmate/exports`
pub fn workspace_exports_dir(workspace: &Path) -> PathBuf {
    workspace.join(".crabmate").join("exports")
}

fn export_filename(prefix: &str, ext: &str) -> String {
    format!(
        "{}_{}.{}",
        prefix,
        chrono::Local::now().format("%Y%m%d_%H%M%S"),
        ext
    )
}

/// 写入 `exports/chat_export_YYYYMMDD_HHMMSS.json`。
pub fn write_json_export(workspace: &Path, messages: &[Message]) -> io::Result<PathBuf> {
    let dir = workspace_exports_dir(workspace);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(export_filename("chat_export", "json"));
    let body = ChatSessionFile::from_slice(messages);
    let json = session_to_json_pretty(&body).map_err(io::Error::other)?;
    std::fs::write(&path, json)?;
    Ok(path)
}

/// 写入 `exports/chat_export_YYYYMMDD_HHMMSS.md`。
pub fn write_markdown_export(workspace: &Path, messages: &[Message]) -> io::Result<PathBuf> {
    let dir = workspace_exports_dir(workspace);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(export_filename("chat_export", "md"));
    std::fs::write(&path, messages_to_markdown(messages))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crabmate_chat_export::messages_to_markdown_with_body;

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
        crabmate_types::message_content_as_str(&m.content)
            .unwrap_or("")
            .to_string()
    }

    fn md(messages: &[Message]) -> String {
        messages_to_markdown_with_body(messages, ExportMdLocale::ZhHans, plain_body)
    }

    #[test]
    fn markdown_skips_system_and_labels_roles() {
        let text = md(&[
            msg("system", "sys"),
            msg("user", "hi"),
            msg("assistant", "hey"),
            msg("tool", "out"),
        ]);
        assert!(!text.contains("sys"));
        assert!(text.contains("## 用户"));
        assert!(text.contains("hi"));
        assert!(text.contains("## 助手"));
        assert!(text.contains("## 工具"));
        assert!(text.contains("out"));
    }

    #[test]
    fn markdown_reorders_tool_after_assistant() {
        let messages = vec![
            msg("assistant", "意图分析：执行类"),
            msg("tool", "解压缩结果"),
            msg("tool", "list_tree 结果"),
            msg("assistant", "已解压。看看目录结构..."),
        ];
        let text = md(&messages);
        let assistant_pos = text.find("## 助手").unwrap();
        let tool_pos = text.find("## 工具").unwrap();
        let second_assistant_pos =
            text[assistant_pos + 10..].find("## 助手").unwrap() + assistant_pos + 10;
        assert!(
            assistant_pos < tool_pos && tool_pos < second_assistant_pos,
            "工具消息应该在两个助手消息之间"
        );
    }

    #[test]
    fn markdown_reorders_multiple_tools_with_assistant() {
        let messages = vec![
            msg("assistant", "第一轮：分析任务"),
            msg("tool", "解压缩"),
            msg("tool", "list_tree"),
            msg("tool", "read_file"),
            msg("assistant", "第二轮：执行编译"),
            msg("tool", "run_command"),
            msg("tool", "file_exists"),
            msg("assistant", "编译完成"),
        ];
        let text = md(&messages);
        let parts: Vec<&str> = text.split("## 助手").collect();
        assert_eq!(parts.len(), 4, "应该有 3 个助手消息分隔");
        assert!(parts[1].contains("## 工具"), "第一个助手后应该有工具消息");
        assert!(parts[2].contains("## 工具"), "第二个助手后应该有工具消息");
    }

    #[test]
    fn session_file_roundtrip() {
        let file = ChatSessionFile::new(vec![msg("user", "x")]);
        let s = session_to_json_pretty(&file).unwrap();
        assert!(s.contains(CHAT_EXPORT_SCHEMA_ID));
        assert!(s.contains(CHAT_EXPORT_SCHEMA_VERSION));
        let back: ChatSessionFile = serde_json::from_str(&s).unwrap();
        assert_eq!(back.schema, CHAT_EXPORT_SCHEMA_ID);
        assert_eq!(back.schema_version, CHAT_EXPORT_SCHEMA_VERSION);
        assert_eq!(back.version, CHAT_SESSION_FILE_VERSION);
        assert_eq!(back.messages.len(), 1);
        assert_eq!(back.messages[0].role, "user");
    }

    /// Golden：模拟「编译 hpcg」多轮 outer loop 的 agent 消息序列。
    #[test]
    fn golden_compile_hpcg_fine_grained_bubbles() {
        let messages = vec![
            msg("user", "编译hpcg"),
            msg(
                "assistant",
                "意图分析：执行类（直接执行）\n综合置信度：0.95\n主意图：execute.run_test_build",
            ),
            msg("assistant", "先了解工作区中的 HPCG 源码包情况。"),
            msg(
                "tool",
                "unpack hpcg-HPCG-release-3-1-0.tar.gz\n已解压 184 个文件",
            ),
            msg("tool", "mkdir -p hpcg-HPCG-release-3-1-0/build\n退出码：0"),
            msg("assistant", "解压成功。现在查看目录结构。"),
            msg(
                "tool",
                "read dir: hpcg-HPCG-release-3-1-0\n显示 setup/ 目录等",
            ),
            msg("tool", "read file: INSTALL\n显示构建说明"),
            msg(
                "assistant",
                "用的是传统 Makefile 构建系统，有 configure 和 Makefile。",
            ),
            msg(
                "tool",
                "read dir: hpcg-HPCG-release-3-1-0/setup\n显示 Make.Linux_Serial 等模板",
            ),
            msg("assistant", "用 Make.Linux_Serial 模板来配置。"),
            msg("tool", "bash configure Linux_Serial\n退出码：0"),
            msg("assistant", "配置成功。现在编译。"),
            msg("tool", "make -j4\n编译完成，exit=0"),
            msg("tool", "ls -lh bin/xhpcg\n-rwxrwxr-x 194K bin/xhpcg"),
            msg(
                "assistant",
                "编译成功！\n\n产物：bin/xhpcg (194K)\n\n使用 Make.Linux_Serial 模板，g++ -O3 编译。",
            ),
        ];
        let text = md(&messages);

        let assistant_count = text.matches("## 助手\n").count();
        assert!(
            assistant_count >= 5,
            "应有 ≥5 个 ## 助手 气泡，实际 {assistant_count} 个\nmd={text}"
        );

        assert!(text.contains("## 工具"), "应包含 ## 工具 节");

        let mut sections: Vec<&str> = vec![];
        let mut start = 0usize;
        while let Some(pos) = text[start..].find("## 助手\n") {
            let abs = start + pos;
            sections.push(&text[abs..]);
            start = abs + "## 助手\n".len();
        }
        for sec in &sections {
            let body = sec.strip_prefix("## 助手\n\n").unwrap_or("");
            let end = body.find("\n## ").unwrap_or(body.len());
            let bubble_text = &body[..end];
            assert!(
                bubble_text.chars().count() <= 500,
                "单个助手气泡不应超过 500 字符（疑似合并），实际 {} 字符:\n{}",
                bubble_text.chars().count(),
                &bubble_text[..bubble_text.len().min(200)]
            );
        }

        let mut headings: Vec<&str> = vec![];
        for line in text.lines() {
            if line == "## 助手" || line == "## 工具" || line == "## 用户" {
                headings.push(line);
            }
        }
        let first_tool = headings.iter().position(|h| *h == "## 工具");
        let last_tool = headings.iter().rposition(|h| *h == "## 工具");
        assert!(first_tool.is_some(), "应有工具节");
        let fi = first_tool.unwrap();
        let li = last_tool.unwrap();
        let has_assistant_between_tools = headings[fi..=li].contains(&"## 助手");
        assert!(
            has_assistant_between_tools,
            "工具节之间应有助手节交错分布: {:?}",
            headings
        );

        let first_assistant = sections.first().unwrap();
        assert!(
            first_assistant.contains("意图分析") || first_assistant.contains("执行类"),
            "首条助手应为意图分析"
        );

        let last_assistant = sections.last().unwrap();
        assert!(
            last_assistant.contains("编译成功"),
            "末条助手应包含编译成功摘要"
        );
    }

    #[test]
    fn session_file_deserialize_legacy_without_schema() {
        let json = r#"{"version":1,"messages":[]}"#;
        let f: ChatSessionFile = serde_json::from_str(json).unwrap();
        assert_eq!(f.schema, CHAT_EXPORT_SCHEMA_ID);
        assert_eq!(f.schema_version, CHAT_EXPORT_SCHEMA_VERSION);
        assert_eq!(f.version, 1);
        assert!(f.messages.is_empty());
    }
}
