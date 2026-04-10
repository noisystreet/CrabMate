//! Web `/chat*` 与 CLI 首轮消息共用的**项目画像 / 依赖摘要**注入与消息拼装（与 `project_profile::build_first_turn_user_context_markdown` 同源）。
//!
//! - **Web**：新会话可带 `agent_memory` 片段；重扫描可走 `spawn_blocking`。
//! - **CLI**：`chat` / REPL 在 `[system, user]` 间插入可选 `user` 上下文条；无备忘文件路径。

use std::path::Path;

use log::debug;

use crate::config::{AgentConfig, SharedAgentConfig};
use crate::project_profile::build_first_turn_user_context_markdown;
use crate::types::Message;

/// 项目画像 / `cargo metadata` 等重扫描是否应放到阻塞线程（与 Web `build_messages_for_turn`、CLI `prepend_cli_first_turn_injection` 对齐）。
pub(crate) fn project_scan_needs_spawn_blocking(cfg: &AgentConfig) -> bool {
    (cfg.project_profile_inject_enabled && cfg.project_profile_inject_max_chars > 0)
        || (cfg.project_dependency_brief_inject_enabled
            && cfg.project_dependency_brief_inject_max_chars > 0)
}

/// 首轮「工作区上下文」`user` 条正文（无则 `None`）。`memory_snippet` 仅 Web 新会话传入；CLI 传 `None`。
pub(crate) async fn first_turn_project_context_user_message(
    workspace_root: &Path,
    cfg: &AgentConfig,
    memory_snippet: Option<String>,
) -> Option<String> {
    if project_scan_needs_spawn_blocking(cfg) {
        let cfg_owned = cfg.clone();
        let root = workspace_root.to_path_buf();
        let mem = memory_snippet.clone();
        match tokio::task::spawn_blocking(move || {
            build_first_turn_user_context_markdown(&root, &cfg_owned, mem)
        })
        .await
        {
            Ok(v) => v,
            Err(e) => {
                debug!(
                    target: "crabmate",
                    "first_turn_project_context spawn_blocking failed: {}",
                    e
                );
                None
            }
        }
    } else {
        build_first_turn_user_context_markdown(workspace_root, cfg, memory_snippet)
    }
}

/// 同步构建首轮项目上下文（供 `workspace_session` 等非 async 路径；重扫描仍在当前线程执行，与历史行为一致）。
pub(crate) fn first_turn_project_context_user_message_sync(
    workspace_root: &Path,
    cfg: &AgentConfig,
    memory_snippet: Option<String>,
) -> Option<String> {
    build_first_turn_user_context_markdown(workspace_root, cfg, memory_snippet)
}

/// `system_prompt_for_new_conversation` + 工具统计附录；角色解析失败时退回全局 `system_prompt`（REPL / 磁盘会话与 `repl_bootstrap_messages_fast` 一致）。
pub(crate) fn augmented_system_for_new_conversation_lenient(
    cfg: &AgentConfig,
    agent_role: Option<&str>,
) -> String {
    let base = match cfg.system_prompt_for_new_conversation(agent_role) {
        Ok(s) => s.to_string(),
        Err(_) => cfg.system_prompt.clone(),
    };
    crate::tool_stats::augment_system_prompt(&base, cfg)
}

/// 新会话首轮：`system` + 可选项目上下文 `user` + 可选本轮用户 `user`（与 Web 新会话、`messages_chat_seed` 组合一致）。
pub(crate) fn compose_new_conversation_messages(
    system_for_turn: &str,
    project_context: Option<String>,
    last_user: Option<Message>,
) -> Vec<Message> {
    match (project_context, last_user) {
        (Some(ctx), Some(u)) => vec![
            Message::system_only(system_for_turn.to_string()),
            Message::user_only(ctx),
            u,
        ],
        (None, Some(u)) => vec![Message::system_only(system_for_turn.to_string()), u],
        (Some(ctx), None) => vec![
            Message::system_only(system_for_turn.to_string()),
            Message::user_only(ctx),
        ],
        (None, None) => vec![Message::system_only(system_for_turn.to_string())],
    }
}

/// `chat` / REPL：在已有 `[system, user]`（如 `messages_chat_seed`）之间插入项目画像 `user` 条（与 Web 新会话同源）。
pub(crate) async fn prepend_first_turn_project_context_between_system_and_user(
    cfg_holder: &SharedAgentConfig,
    work_dir: &Path,
    messages: &mut Vec<Message>,
) {
    if messages.len() < 2 {
        return;
    }
    if !messages[0].role.trim().eq_ignore_ascii_case("system")
        || !messages[1].role.trim().eq_ignore_ascii_case("user")
    {
        return;
    }
    let cfg = cfg_holder.read().await.clone();
    if let Some(body) = first_turn_project_context_user_message(work_dir, &cfg, None).await {
        messages.insert(1, Message::user_only(body));
    }
}
