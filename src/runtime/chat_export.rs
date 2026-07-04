//! 会话导出：与 `.crabmate/tui_session.json` 同形的 JSON，以及 Markdown 文本生成。
//! 供 `runtime/workspace_session` 使用；Web 前端 `frontend/src/session_export.rs` 应对齐
//! `CHAT_EXPORT_SCHEMA_*`、`CHAT_SESSION_FILE_VERSION` 与字段含义。
#![allow(dead_code)]

use crate::types::Message;
use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};

/// 与磁盘 `tui_session.json`、导出 `chat_export_*.json` 的消息数组约定版本；破坏性变更时递增。
pub const CHAT_SESSION_FILE_VERSION: u32 = 1;

/// 顶层 JSON 信封的稳定标识（URI 形），与 `CHAT_EXPORT_SCHEMA_VERSION` 一起用于工具链与排障。
pub const CHAT_EXPORT_SCHEMA_ID: &str = "crabmate.chat_session";

/// 信封 SemVer；仅当 `schema` 不变而信封字段或语义兼容扩展时可 bump patch；破坏性改 envelope 时 bump minor/major。
pub const CHAT_EXPORT_SCHEMA_VERSION: &str = "1.0.0";

fn default_chat_export_schema() -> String {
    CHAT_EXPORT_SCHEMA_ID.to_string()
}

fn default_chat_export_schema_version() -> String {
    CHAT_EXPORT_SCHEMA_VERSION.to_string()
}

/// OpenAI 兼容消息列表外包一层版本号，供持久化与导出。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSessionFile {
    /// 固定为 [`CHAT_EXPORT_SCHEMA_ID`]；旧文件缺该键时反序列化默认填充，便于读旧 `tui_session.json`。
    #[serde(default = "default_chat_export_schema")]
    pub schema: String,
    /// 与 [`CHAT_EXPORT_SCHEMA_ID`] 配对的 SemVer 字符串。
    #[serde(default = "default_chat_export_schema_version")]
    pub schema_version: String,
    pub version: u32,
    pub messages: Vec<Message>,
}

impl ChatSessionFile {
    pub fn new(messages: Vec<Message>) -> Self {
        Self {
            schema: CHAT_EXPORT_SCHEMA_ID.to_string(),
            schema_version: CHAT_EXPORT_SCHEMA_VERSION.to_string(),
            version: CHAT_SESSION_FILE_VERSION,
            messages,
        }
    }

    pub fn from_slice(messages: &[Message]) -> Self {
        Self {
            schema: CHAT_EXPORT_SCHEMA_ID.to_string(),
            schema_version: CHAT_EXPORT_SCHEMA_VERSION.to_string(),
            version: CHAT_SESSION_FILE_VERSION,
            messages: messages.to_vec(),
        }
    }
}

pub fn session_to_json_pretty(file: &ChatSessionFile) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(file)
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
fn reorder_messages_for_conversation_flow(messages: Vec<Message>) -> Vec<Message> {
    let mut result: Vec<Message> = Vec::with_capacity(messages.len());
    let mut tool_calls_pending: Vec<Message> = Vec::new();

    for m in messages {
        match m.role.as_str() {
            "assistant" => {
                // 先输出之前积累的工具消息
                result.append(&mut tool_calls_pending);
                result.push(m);
            }
            "tool" => {
                // 工具消息暂存，等待下一个助手消息
                tool_calls_pending.push(m);
            }
            "user" => {
                // user 消息前也输出积累的工具消息
                result.append(&mut tool_calls_pending);
                result.push(m);
            }
            _ => {
                // system 等其他角色直接输出
                result.append(&mut tool_calls_pending);
                result.push(m);
            }
        }
    }
    // 输出剩余的工具消息
    result.append(&mut tool_calls_pending);
    result
}

/// 与 TUI F9 / Web 导出一致：跳过 `system` 角色；`tool` 与 `assistant`/`user` 分段输出。
pub fn messages_to_markdown(messages: &[Message]) -> String {
    let reordered = reorder_messages_for_conversation_flow(messages.to_vec());
    let mut md = String::from("# CrabMate 聊天记录\n\n");
    for m in &reordered {
        if m.role == "system" {
            continue;
        }
        let heading = match m.role.as_str() {
            "user" => "## 用户",
            "assistant" => "## 助手",
            "tool" => "## 工具",
            _ => "## 其它",
        };
        md.push_str(heading);
        md.push_str("\n\n");
        let body = if m.role == "assistant" {
            crate::runtime::message_display::assistant_raw_markdown_body_for_message(m)
        } else {
            crate::types::message_content_as_str(&m.content)
                .unwrap_or("")
                .to_string()
        };
        md.push_str(&body);
        md.push_str("\n\n");
    }
    md
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

    #[test]
    fn markdown_skips_system_and_labels_roles() {
        let md = messages_to_markdown(&[
            msg("system", "sys"),
            msg("user", "hi"),
            msg("assistant", "hey"),
            msg("tool", "out"),
        ]);
        assert!(!md.contains("sys"));
        assert!(md.contains("## 用户"));
        assert!(md.contains("hi"));
        assert!(md.contains("## 助手"));
        assert!(md.contains("## 工具"));
        assert!(md.contains("out"));
    }

    #[test]
    fn markdown_reorders_tool_after_assistant() {
        // 模拟 Agent 主循环的消息顺序：
        // assistant (带 tool_calls) -> tool -> tool -> assistant
        let messages = vec![
            msg("assistant", "意图分析：执行类"),
            msg("tool", "解压缩结果"),
            msg("tool", "list_tree 结果"),
            msg("assistant", "已解压。看看目录结构..."),
        ];
        let md = messages_to_markdown(&messages);
        // 工具消息应该在第一个助手之后
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
    fn markdown_reorders_multiple_tools_with_assistant() {
        // 模拟多轮工具调用的场景
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
        let md = messages_to_markdown(&messages);
        // 验证消息顺序：assistant -> tool -> assistant -> tool -> assistant
        let parts: Vec<&str> = md.split("## 助手").collect();
        assert_eq!(parts.len(), 4, "应该有 3 个助手消息分隔");
        // 第一个助手后应该有工具消息
        assert!(parts[1].contains("## 工具"), "第一个助手后应该有工具消息");
        // 第二个助手后应该有工具消息
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

    /// Golden：模拟「编译 hpcg」多轮 outer loop 的 agent 消息序列，
    /// 验证导出 Markdown 有细粒度气泡（≥5 个 `## 助手`）、工具节交错分布、无巨泡。
    #[test]
    fn golden_compile_hpcg_fine_grained_bubbles() {
        let messages = vec![
            msg("user", "编译hpcg"),
            // intent analysis
            msg(
                "assistant",
                "意图分析：执行类（直接执行）\n综合置信度：0.95\n主意图：execute.run_test_build",
            ),
            // round 1
            msg("assistant", "先了解工作区中的 HPCG 源码包情况。"),
            msg(
                "tool",
                "unpack hpcg-HPCG-release-3-1-0.tar.gz\n已解压 184 个文件",
            ),
            msg("tool", "mkdir -p hpcg-HPCG-release-3-1-0/build\n退出码：0"),
            // round 2
            msg("assistant", "解压成功。现在查看目录结构。"),
            msg(
                "tool",
                "read dir: hpcg-HPCG-release-3-1-0\n显示 setup/ 目录等",
            ),
            msg("tool", "read file: INSTALL\n显示构建说明"),
            // round 3
            msg(
                "assistant",
                "用的是传统 Makefile 构建系统，有 configure 和 Makefile。",
            ),
            msg(
                "tool",
                "read dir: hpcg-HPCG-release-3-1-0/setup\n显示 Make.Linux_Serial 等模板",
            ),
            // round 4 - configure
            msg("assistant", "用 Make.Linux_Serial 模板来配置。"),
            msg("tool", "bash configure Linux_Serial\n退出码：0"),
            // round 5 - build
            msg("assistant", "配置成功。现在编译。"),
            msg("tool", "make -j4\n编译完成，exit=0"),
            msg("tool", "ls -lh bin/xhpcg\n-rwxrwxr-x 194K bin/xhpcg"),
            // final summary
            msg(
                "assistant",
                "编译成功！\n\n产物：bin/xhpcg (194K)\n\n使用 Make.Linux_Serial 模板，g++ -O3 编译。",
            ),
        ];
        let md = messages_to_markdown(&messages);

        // 1. 验证有多个助手节（≥5 才是细粒度）
        let assistant_count = md.matches("## 助手\n").count();
        assert!(
            assistant_count >= 5,
            "应有 ≥5 个 ## 助手 气泡，实际 {assistant_count} 个\nmd={md}"
        );

        // 2. 验证工具节存在
        assert!(md.contains("## 工具"), "应包含 ## 工具 节");

        // 3. 验证每个助手节不超过巨泡阈值（500 字符），
        //    断言单个气泡不是合并巨泡
        let mut sections: Vec<&str> = vec![];
        let mut start = 0usize;
        while let Some(pos) = md[start..].find("## 助手\n") {
            let abs = start + pos;
            sections.push(&md[abs..]);
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

        // 4. 验证顺序：助手 → 工具 交错分布（非工具堆在一端）
        let mut headings: Vec<&str> = vec![];
        for line in md.lines() {
            if line == "## 助手" || line == "## 工具" || line == "## 用户" {
                headings.push(line);
            }
        }
        // 确认不是所有工具都堆在助手之前或之后
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

        // 5. 验证首条 assistant 为意图分析
        let first_assistant = sections.first().unwrap();
        assert!(
            first_assistant.contains("意图分析") || first_assistant.contains("执行类"),
            "首条助手应为意图分析"
        );

        // 6. 验证末条 assistant 包含「编译成功」
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
