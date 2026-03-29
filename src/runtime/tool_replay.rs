//! 从会话消息中提取工具调用时间线，导出为 JSON fixture，并在本地重放（不调用大模型）。
//! 用于复现工具链路与回归对比；**与正常对话相同**走 `tools::run_tool`，须在可信工作区使用。

use crate::runtime::chat_export::ChatSessionFile;
use crate::tools::{run_tool, tool_context_for};
use crate::types::{Message, ToolCall};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// fixture 文件版本；演进时同步文档与测试样例。
pub const TOOL_REPLAY_FILE_VERSION: u32 = 1;

/// 单条可重放步骤（与会话中顺序一致）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolReplayStep {
    pub index: usize,
    pub tool_call_id: String,
    pub name: String,
    /// 与 OpenAI `function.arguments` 相同：JSON 字符串。
    pub arguments: String,
    /// 会话中 `role=tool` 的 `content`（可能为信封 JSON）；无对应条则为 null。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recorded_output: Option<String>,
}

/// 写入 `.crabmate/exports/` 的 fixture 外形。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolReplayFile {
    pub version: u32,
    /// 固定标识，便于脚本识别。
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub steps: Vec<ToolReplayStep>,
}

impl ToolReplayFile {
    pub fn new(steps: Vec<ToolReplayStep>, note: Option<String>) -> Self {
        Self {
            version: TOOL_REPLAY_FILE_VERSION,
            source: "crabmate-tool-replay".to_string(),
            note,
            steps,
        }
    }
}

#[inline]
fn role_is_assistant(role: &str) -> bool {
    role.trim().eq_ignore_ascii_case("assistant")
}

#[inline]
fn role_is_tool(role: &str) -> bool {
    role.trim().eq_ignore_ascii_case("tool")
}

/// 从 OpenAI 形消息列表中提取按时间顺序的「助手 tool_calls + 紧随的 tool 结果」对。
pub fn extract_tool_replay_steps(messages: &[Message]) -> Vec<ToolReplayStep> {
    let mut out: Vec<ToolReplayStep> = Vec::new();
    let mut i = 0usize;
    while i < messages.len() {
        let m = &messages[i];
        if !role_is_assistant(&m.role) {
            i += 1;
            continue;
        }
        let Some(calls) = m.tool_calls.as_ref().filter(|c| !c.is_empty()) else {
            i += 1;
            continue;
        };
        let mut results: HashMap<String, String> = HashMap::new();
        let mut j = i + 1;
        while j < messages.len() && role_is_tool(&messages[j].role) {
            if let Some(id) = messages[j].tool_call_id.as_deref() {
                let body = messages[j].content.clone().unwrap_or_default();
                results.insert(id.to_string(), body);
            }
            j += 1;
        }
        for tc in calls {
            push_step_from_call(&mut out, tc, &results);
        }
        i = j;
    }
    out
}

fn push_step_from_call(
    out: &mut Vec<ToolReplayStep>,
    tc: &ToolCall,
    results: &HashMap<String, String>,
) {
    let idx = out.len();
    let recorded = results.get(tc.id.as_str()).cloned();
    out.push(ToolReplayStep {
        index: idx,
        tool_call_id: tc.id.clone(),
        name: tc.function.name.clone(),
        arguments: tc.function.arguments.clone(),
        recorded_output: recorded,
    });
}

pub fn load_chat_session_file(path: &Path) -> io::Result<ChatSessionFile> {
    let data = std::fs::read_to_string(path)?;
    serde_json::from_str(&data).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("会话或导出 JSON 无效（期望 ChatSessionFile）: {e}"),
        )
    })
}

pub fn load_tool_replay_file(path: &Path) -> io::Result<ToolReplayFile> {
    let data = std::fs::read_to_string(path)?;
    serde_json::from_str(&data).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("tool-replay fixture 无效: {e}"),
        )
    })
}

fn default_tool_replay_export_path(workspace: &Path) -> PathBuf {
    let dir = crate::runtime::chat_export::workspace_exports_dir(workspace);
    let name = format!(
        "tool_replay_{}.json",
        chrono::Local::now().format("%Y%m%d_%H%M%S")
    );
    dir.join(name)
}

/// 从会话文件导出 fixture；`output` 为 None 时写入 `exports/tool_replay_*.json`。
pub fn export_tool_replay_fixture(
    session_path: &Path,
    workspace: &Path,
    output: Option<&Path>,
    note: Option<&str>,
) -> io::Result<PathBuf> {
    let file = load_chat_session_file(session_path)?;
    let steps = extract_tool_replay_steps(&file.messages);
    let replay = ToolReplayFile::new(
        steps,
        note.map(|s| s.to_string()).filter(|s| !s.trim().is_empty()),
    );
    let json = serde_json::to_string_pretty(&replay).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("序列化 fixture 失败: {e}"),
        )
    })?;
    let dest = match output {
        Some(p) => p.to_path_buf(),
        None => {
            let dir = crate::runtime::chat_export::workspace_exports_dir(workspace);
            std::fs::create_dir_all(&dir)?;
            default_tool_replay_export_path(workspace)
        }
    };
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&dest, json)?;
    Ok(dest)
}

/// 重放 fixture；若 `compare_recorded` 为 true，对含 `recorded_output` 的步骤做字符串全等比较。
/// 返回 (执行步数, 比较不匹配数)。
pub fn run_tool_replay_fixture(
    fixture_path: &Path,
    cfg: &crate::config::AgentConfig,
    working_dir: &Path,
    compare_recorded: bool,
    mut out: impl Write,
) -> io::Result<(usize, usize)> {
    let file = load_tool_replay_file(fixture_path)?;
    if file.version != TOOL_REPLAY_FILE_VERSION {
        writeln!(
            out,
            "警告：fixture version={} 与当前实现 {} 不一致，仍尝试执行。",
            file.version, TOOL_REPLAY_FILE_VERSION
        )?;
    }
    let allowed = cfg.allowed_commands.as_ref();
    let ctx = tool_context_for(cfg, allowed, working_dir);
    let mut mismatches = 0usize;
    for step in &file.steps {
        writeln!(
            out,
            "[{}] {} id={} args={}",
            step.index, step.name, step.tool_call_id, step.arguments
        )?;
        let fresh = run_tool(&step.name, &step.arguments, &ctx);
        writeln!(out, "—— 本次输出 ——\n{}", fresh)?;
        if compare_recorded {
            match &step.recorded_output {
                Some(recorded) if recorded == &fresh => {
                    writeln!(out, "—— compare-recorded: 与录制一致 ——")?;
                }
                Some(recorded) => {
                    mismatches += 1;
                    writeln!(
                        out,
                        "—— compare-recorded: 不一致（录制长度 {}，本次长度 {}）——",
                        recorded.len(),
                        fresh.len()
                    )?;
                    // 避免巨屏：仅预览前几行
                    let rec_preview: String =
                        recorded.lines().take(3).collect::<Vec<_>>().join("\n");
                    let fresh_preview: String =
                        fresh.lines().take(3).collect::<Vec<_>>().join("\n");
                    writeln!(
                        out,
                        "录制预览:\n{rec_preview}\n…\n本次预览:\n{fresh_preview}\n…"
                    )?;
                }
                None => writeln!(out, "—— compare-recorded: 跳过（无 recorded_output）——")?,
            }
        }
    }
    Ok((file.steps.len(), mismatches))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FunctionCall, Message};

    fn sample_messages() -> Vec<Message> {
        vec![
            Message::user_only("hi"),
            Message {
                role: "assistant".into(),
                content: Some("call calc".into()),
                reasoning_content: None,
                tool_calls: Some(vec![ToolCall {
                    id: "c1".into(),
                    typ: "function".into(),
                    function: FunctionCall {
                        name: "calc".into(),
                        arguments: r#"{"expression":"2+2"}"#.into(),
                    },
                }]),
                name: None,
                tool_call_id: None,
            },
            Message {
                role: "tool".into(),
                content: Some("4".into()),
                reasoning_content: None,
                tool_calls: None,
                name: None,
                tool_call_id: Some("c1".into()),
            },
        ]
    }

    #[test]
    fn extract_single_tool_call_with_result() {
        let steps = extract_tool_replay_steps(&sample_messages());
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].name, "calc");
        assert_eq!(steps[0].arguments, r#"{"expression":"2+2"}"#);
        assert_eq!(steps[0].recorded_output.as_deref(), Some("4"));
    }

    #[test]
    fn extract_parallel_tool_calls() {
        let messages = vec![
            Message {
                role: "assistant".into(),
                content: None,
                reasoning_content: None,
                tool_calls: Some(vec![
                    ToolCall {
                        id: "a".into(),
                        typ: "function".into(),
                        function: FunctionCall {
                            name: "calc".into(),
                            arguments: r#"{"expression":"1"}"#.into(),
                        },
                    },
                    ToolCall {
                        id: "b".into(),
                        typ: "function".into(),
                        function: FunctionCall {
                            name: "calc".into(),
                            arguments: r#"{"expression":"2"}"#.into(),
                        },
                    },
                ]),
                name: None,
                tool_call_id: None,
            },
            Message {
                role: "tool".into(),
                content: Some("one".into()),
                reasoning_content: None,
                tool_calls: None,
                name: None,
                tool_call_id: Some("a".into()),
            },
            Message {
                role: "tool".into(),
                content: Some("two".into()),
                reasoning_content: None,
                tool_calls: None,
                name: None,
                tool_call_id: Some("b".into()),
            },
        ];
        let steps = extract_tool_replay_steps(&messages);
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].recorded_output.as_deref(), Some("one"));
        assert_eq!(steps[1].recorded_output.as_deref(), Some("two"));
    }
}
