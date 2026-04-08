//! 工作区 `.crabmate/tui_session.json`：加载/保存与导出（实现委托 `runtime::chat_export`）。
//! CLI REPL：`initial_workspace_messages` 可在独立线程中构建，经 [`try_merge_background_initial_workspace`] 并入对话；[`repl_bootstrap_messages_fast`] 为不阻塞的占位首条 `system`。
#![allow(dead_code)]

use crate::config::AgentConfig;
use crate::project_profile::build_first_turn_user_context_markdown;
use crate::runtime::chat_export::{self, ChatSessionFile, session_to_json_pretty};
use crate::types::{Message, normalize_messages_for_openai_compatible_request};
use log::warn;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub fn session_file_path(workspace: &Path) -> PathBuf {
    workspace.join(".crabmate").join("tui_session.json")
}

/// 超过上限时保留首条 `system` 与尾部最近若干条，减轻启动与 TUI 渲染负担。
///
/// 若保留的尾部以**两条连续** `assistant` 开头，且丢弃的前缀里仍有 `user`，会把**该前缀中最后一条 user** 插回尾部，
/// 避免变成 `[system, assistant, assistant, …]`（供应商报 `Invalid consecutive assistant message at message index 2`）。
fn truncate_loaded_messages(mut msgs: Vec<Message>, max_total: usize) -> Vec<Message> {
    let max_total = max_total.max(2);
    if msgs.len() <= max_total {
        return normalize_messages_for_openai_compatible_request(msgs);
    }
    let system = msgs.remove(0);
    let after = msgs;
    let tail_keep = max_total.saturating_sub(1);
    let skip = after.len().saturating_sub(tail_keep);
    let mut tail: Vec<Message> = after.iter().skip(skip).cloned().collect();

    let tail_opens_with_assistant_run = tail.len() >= 2
        && tail[0].role.trim().eq_ignore_ascii_case("assistant")
        && tail[1].role.trim().eq_ignore_ascii_case("assistant");
    if tail_opens_with_assistant_run
        && let Some(ui) = after[..skip]
            .iter()
            .rposition(|m| m.role.trim().eq_ignore_ascii_case("user"))
    {
        tail.insert(0, after[ui].clone());
        while tail.len() > tail_keep {
            if tail.len() <= 1 {
                break;
            }
            tail.remove(1);
        }
    }

    let mut out = vec![system];
    out.extend(tail);
    normalize_messages_for_openai_compatible_request(out)
}

/// 若存在会话文件则加载；首条 `system` 会替换为当前配置的 `system_prompt`。
/// `max_messages` 为加载后的消息条数上限（含 `system`）；超出则丢弃最旧的用户/助手/工具消息。
pub fn load_workspace_session(
    workspace: &Path,
    system_prompt: &str,
    max_messages: usize,
) -> Option<Vec<Message>> {
    let path = session_file_path(workspace);
    let data = std::fs::read_to_string(&path).ok()?;
    let parsed: ChatSessionFile = serde_json::from_str(&data).ok()?;
    if parsed.messages.is_empty() {
        return None;
    }
    let mut msgs = parsed.messages;
    if msgs.first().is_some_and(|m| m.role == "system") {
        msgs[0].content = Some(system_prompt.to_string());
    } else {
        msgs.insert(0, Message::system_only(system_prompt.to_string()));
    }
    Some(truncate_loaded_messages(msgs, max_messages))
}

/// REPL 启动后**立刻**可用的消息列表（仅 `system`），不阻塞项目画像 / 会话恢复等耗时逻辑。
pub fn repl_bootstrap_messages_fast(cfg: &AgentConfig, agent_role: Option<&str>) -> Vec<Message> {
    let base = cfg
        .system_prompt_for_new_conversation(agent_role)
        .unwrap_or(cfg.system_prompt.as_str())
        .to_string();
    let system = crate::tool_stats::augment_system_prompt(&base, cfg);
    vec![Message::system_only(system)]
}

/// 从后台线程槽位取出至多一次的 [`initial_workspace_messages`] 结果，合并进当前 REPL `messages`。
///
/// - 若当前仍为「仅一条 system」引导状态：用 `full` **整体替换**（含磁盘恢复的完整 transcript）。
/// - 若用户已追加输入（至少两条消息）且 `full` 为 `[system, user_ctx]`：将 `user_ctx` **插入**在索引 1（首轮上下文仍在首条用户消息之前）。
/// - 若 `full` 为长会话且当前已有不止一条消息：**不合并**（避免覆盖用户已输入内容），并打一条 `warn` 日志。
pub fn try_merge_background_initial_workspace(
    messages: &mut Vec<Message>,
    pending_slot: Option<&Arc<Mutex<Option<Vec<Message>>>>>,
) {
    let Some(pending_slot) = pending_slot else {
        return;
    };
    let full = {
        let mut g = pending_slot.lock().unwrap_or_else(|e| e.into_inner());
        g.take()
    };
    let Some(full) = full else {
        return;
    };
    merge_initial_workspace_into(messages, full);
}

fn merge_initial_workspace_into(messages: &mut Vec<Message>, full: Vec<Message>) {
    if full.is_empty() {
        return;
    }

    if messages.len() == 1 {
        *messages = full;
        return;
    }

    if full.len() == 2 {
        let inj = full[1].clone();
        if inj.role.trim().eq_ignore_ascii_case("user") {
            messages.insert(1, inj);
        }
        return;
    }

    if full.len() > 2 {
        warn!(
            target: "crabmate",
            "后台会话恢复已完成，但当前 REPL 已有 {} 条消息，跳过合并磁盘会话（可 /clear 后重启 REPL）",
            messages.len()
        );
    }
}

/// TUI / CLI REPL 启动时：按配置决定是否从磁盘恢复会话，否则仅一条 `system`（与当前 `system_prompt` 对齐）。
pub fn initial_workspace_messages(
    cfg: &AgentConfig,
    workspace: &Path,
    load_from_disk: bool,
    agent_role: Option<&str>,
) -> Vec<Message> {
    let base_system = match cfg.system_prompt_for_new_conversation(agent_role) {
        Ok(s) => s.to_string(),
        Err(_) => cfg.system_prompt.clone(),
    };
    if !load_from_disk {
        let system_seed = crate::tool_stats::augment_system_prompt(&base_system, cfg);
        if let Some(ctx) = build_first_turn_user_context_markdown(workspace, cfg, None) {
            return vec![Message::system_only(system_seed), Message::user_only(ctx)];
        }
        return vec![Message::system_only(system_seed)];
    }
    load_workspace_session(workspace, &base_system, cfg.tui_session_max_messages).unwrap_or_else(
        || {
            let system_seed = crate::tool_stats::augment_system_prompt(&base_system, cfg);
            if let Some(ctx) = build_first_turn_user_context_markdown(workspace, cfg, None) {
                vec![
                    Message::system_only(system_seed.clone()),
                    Message::user_only(ctx),
                ]
            } else {
                vec![Message::system_only(system_seed)]
            }
        },
    )
}

pub fn save_workspace_session(workspace: &Path, messages: &[Message]) -> std::io::Result<()> {
    let dir = workspace.join(".crabmate");
    std::fs::create_dir_all(&dir)?;
    let path = session_file_path(workspace);
    let body = ChatSessionFile::new(messages.to_vec());
    let json = session_to_json_pretty(&body).map_err(std::io::Error::other)?;
    std::fs::write(&path, json)
}

pub fn export_json(workspace: &Path, messages: &[Message]) -> std::io::Result<PathBuf> {
    chat_export::write_json_export(workspace, messages)
}

pub fn export_markdown(workspace: &Path, messages: &[Message]) -> std::io::Result<PathBuf> {
    chat_export::write_markdown_export(workspace, messages)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(role: &str, content: &str) -> Message {
        Message {
            role: role.to_string(),
            content: Some(content.to_string()),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        }
    }

    #[test]
    fn truncate_keeps_system_and_tail() {
        let v = vec![
            msg("system", "s"),
            msg("user", "a"),
            msg("assistant", "b"),
            msg("user", "c"),
            msg("assistant", "d"),
        ];
        let t = truncate_loaded_messages(v, 3);
        assert_eq!(t.len(), 3);
        assert_eq!(t[0].role, "system");
        assert_eq!(t[1].content.as_deref(), Some("c"));
        assert_eq!(t[2].content.as_deref(), Some("d"));
    }

    #[test]
    fn truncate_noop_when_short() {
        let v = vec![msg("system", "s"), msg("user", "u")];
        let t = truncate_loaded_messages(v, 10);
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn truncate_inserts_user_when_tail_would_be_two_assistants() {
        let v = vec![
            msg("system", "s"),
            msg("user", "old_u"),
            msg("assistant", "a1"),
            msg("assistant", "a2"),
        ];
        let t = truncate_loaded_messages(v, 3);
        assert_eq!(t.len(), 3);
        assert_eq!(t[0].role, "system");
        assert_eq!(t[1].role, "user");
        assert_eq!(t[1].content.as_deref(), Some("old_u"));
        assert_eq!(t[2].role, "assistant");
    }

    #[test]
    fn merge_initial_replaces_when_only_system() {
        let mut m = vec![msg("system", "a")];
        let full = vec![msg("system", "b"), msg("user", "ctx")];
        super::merge_initial_workspace_into(&mut m, full);
        assert_eq!(m.len(), 2);
        assert_eq!(m[1].content.as_deref(), Some("ctx"));
    }

    #[test]
    fn merge_initial_inserts_ctx_before_first_user_line() {
        let mut m = vec![msg("system", "s"), msg("user", "hi")];
        let full = vec![msg("system", "s2"), msg("user", "ctx")];
        super::merge_initial_workspace_into(&mut m, full);
        assert_eq!(m.len(), 3);
        assert_eq!(m[1].content.as_deref(), Some("ctx"));
        assert_eq!(m[2].content.as_deref(), Some("hi"));
    }

    #[test]
    fn merge_initial_skips_long_session_when_already_branched() {
        let mut m = vec![msg("system", "s"), msg("user", "hi")];
        let full = vec![
            msg("system", "s"),
            msg("user", "old"),
            msg("assistant", "a"),
        ];
        super::merge_initial_workspace_into(&mut m, full);
        assert_eq!(m.len(), 2);
        assert_eq!(m[1].content.as_deref(), Some("hi"));
    }

    #[test]
    fn try_merge_noops_when_pending_none() {
        let mut m = vec![msg("system", "s")];
        try_merge_background_initial_workspace(&mut m, None);
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn try_merge_takes_once_from_slot() {
        let slot = Arc::new(Mutex::new(Some(vec![msg("system", "s"), msg("user", "x")])));
        let mut m = vec![msg("system", "s")];
        try_merge_background_initial_workspace(&mut m, Some(&slot));
        assert_eq!(m.len(), 2);
        try_merge_background_initial_workspace(&mut m, Some(&slot));
        assert_eq!(m.len(), 2);
    }
}
