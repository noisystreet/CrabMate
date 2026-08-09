//! `/mode`、`/context`、`/clear`、`/agent` 斜杠命令实现。

use std::path::Path;
use std::sync::Arc;

use crate::agent_role_turn::apply_agent_role_switch_to_messages;
use crate::config::SharedAgentConfig;
use crate::runtime::cli::repl_parse::repl_agent_role_set_is_default_pseudo;
use crate::runtime::cli_repl_ui::CliReplStyle;
use crate::types::Message;

use super::super::repl_extras::{
    ReplSlashHandled, ReplSlashSharedHandles, repl_rebuild_bootstrap_messages,
};

pub(super) fn slash_mode_show(
    handles: &ReplSlashSharedHandles,
    style: &CliReplStyle,
) -> ReplSlashHandled {
    let mode = handles
        .session_mode
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let _ = style.print_line(&format!(
        "当前会话模式: {} — {}",
        mode.as_str(),
        mode.display_title_zh()
    ));
    ReplSlashHandled::Handled
}

pub(super) async fn slash_mode_set(
    mode: crate::types::SessionMode,
    cfg_holder: &SharedAgentConfig,
    messages: &mut [Message],
    agent_role: Option<&str>,
    style: &CliReplStyle,
    handles: &ReplSlashSharedHandles,
    work_dir: &Path,
) -> ReplSlashHandled {
    {
        let mut g = handles
            .session_mode
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        *g = mode;
    }
    let cfg = cfg_holder.read().await.clone();
    let role_id =
        match crate::context_bootstrap::prompt_compose::resolve_agent_role_for_prompt_compose(
            &cfg, agent_role, None,
        ) {
            Ok(id) => id,
            Err(e) => {
                let _ = style.eprint_error(&format!("解析角色失败: {e}"));
                return ReplSlashHandled::Handled;
            }
        };
    match crate::context_bootstrap::prompt_compose::compose_first_system_for_turn(
        &cfg,
        &handles.process_handles.tool_outcome_recorder,
        crate::context_bootstrap::prompt_compose::FirstSystemComposeOpts {
            agent_role: role_id.as_deref(),
            user_msg_for_skills: None,
            skills_base_dir: Some(work_dir.to_path_buf()),
            forced_skill: None,
            role_resolution: crate::context_bootstrap::prompt_compose::RoleSystemResolution::Strict,
            session_mode: Some(mode),
        },
    ) {
        Ok(sys) => {
            let refreshed = if let Some(first) = messages.first_mut()
                && first.role == "system"
            {
                first.content = Some(crate::types::MessageContent::Text(sys));
                true
            } else {
                false
            };
            if refreshed {
                let _ = style.print_success(&format!(
                    "已切换会话模式为 {}（{}）；对话历史已保留。",
                    mode.as_str(),
                    mode.display_title_zh()
                ));
            } else {
                let _ = style.print_success(&format!(
                    "已切换会话模式为 {}（{}）。",
                    mode.as_str(),
                    mode.display_title_zh()
                ));
                let _ = style
                    .eprint_error("会话缺少首条 system，模式附录未刷新；下轮对话会带上新模式。");
            }
        }
        Err(e) => {
            let _ = style.eprint_error(&format!("切换 mode 后刷新 system 失败: {e}"));
        }
    }
    ReplSlashHandled::Handled
}

pub(super) async fn slash_context(
    cfg_holder: &SharedAgentConfig,
    messages: &[Message],
    style: &CliReplStyle,
) -> ReplSlashHandled {
    let cfg = cfg_holder.read().await;
    for line in crate::runtime::context_usage::context_usage_report_lines(&cfg, messages) {
        let _ = style.print_line(&line);
    }
    ReplSlashHandled::Handled
}

pub(super) async fn slash_clear(
    cfg_holder: &SharedAgentConfig,
    messages: &mut Vec<Message>,
    work_dir: &Path,
    agent_role: &mut Option<String>,
    style: &CliReplStyle,
    tool_recorder: &Arc<crate::tool_stats::ToolOutcomeRecorder>,
) -> ReplSlashHandled {
    let cfg = cfg_holder.read().await.clone();
    *messages =
        repl_rebuild_bootstrap_messages(&cfg, work_dir, agent_role.as_deref(), tool_recorder).await;
    let _ = style.print_success(&format!(
        "已清空对话（保留当前 system 提示词），共 {} 条消息。",
        messages.len()
    ));
    ReplSlashHandled::Handled
}
pub(super) async fn slash_agent_list(
    cfg_holder: &SharedAgentConfig,
    agent_role: &Option<String>,
    style: &CliReplStyle,
) -> ReplSlashHandled {
    let cfg = cfg_holder.read().await;
    if cfg.roles_prompts.agent_roles.is_empty() {
        let _ = style.print_line(
            "当前配置未启用多角色（agent_roles 为空）。可在配置中加入 [[agent_roles]] 或 config/agent_roles.toml。",
        );
    } else {
        let mut ids: Vec<&String> = cfg.roles_prompts.agent_roles.keys().collect();
        ids.sort();
        let def = cfg.roles_prompts.default_agent_role_id.as_deref();
        let _ = style.print_line("可用角色 id：");
        let _ = style.print_line(
            "  · default（内建：未显式选用命名角色；与 Web「默认」一致：先按 default_agent_role_id，未配置则用全局 system_prompt）",
        );
        for id in ids {
            let mark = def.is_some_and(|d| d == id.as_str());
            let suffix = if mark { "（配置默认）" } else { "" };
            let _ = style.print_line(&format!("  · {id}{suffix}"));
        }
        let cur = agent_role.as_deref().filter(|s| !s.is_empty()).map_or_else(
            || "当前 REPL: default（未显式设置命名角色）".to_string(),
            |r| format!("当前 REPL 选用命名角色: {r}"),
        );
        let _ = style.print_line(&cur);
    }
    ReplSlashHandled::Handled
}

pub(super) async fn slash_agent_set(
    id: String,
    cfg_holder: &SharedAgentConfig,
    messages: &mut [Message],
    agent_role: &mut Option<String>,
    style: &CliReplStyle,
    handles: &ReplSlashSharedHandles,
    work_dir: &std::path::Path,
) -> ReplSlashHandled {
    let cfg = cfg_holder.read().await;
    if cfg.roles_prompts.agent_roles.is_empty() {
        let _ = style.eprint_error(
            "当前未配置多角色，无法 /agent set。请先配置 [[agent_roles]] 或 agent_roles.toml。",
        );
    } else if repl_agent_role_set_is_default_pseudo(id.as_str()) {
        drop(cfg);
        *agent_role = None;
        let cfg = cfg_holder.read().await.clone();
        let session_mode = cfg.roles_prompts.default_session_mode;
        {
            let mut g = handles
                .session_mode
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            *g = session_mode;
        }
        if let Err(e) = apply_agent_role_switch_to_messages(
            &cfg,
            messages,
            None,
            &handles.process_handles.tool_outcome_recorder,
            Some(work_dir),
            None,
            Some(session_mode),
        ) {
            let _ = style.eprint_error(&e);
        } else {
            let _ = style.print_success(&format!(
                "已设回 default（清除显式命名角色），会话模式 {}，已更新首条 system（保留对话 {} 条）。",
                session_mode.as_str(),
                messages.len()
            ));
        }
    } else if let Err(e) = cfg.system_prompt_for_new_conversation(Some(id.as_str())) {
        let _ = style.eprint_error(&e);
    } else {
        let role_label = id.clone();
        let session_mode =
            crate::session_mode_turn::resolve_initial_session_mode(&cfg, Some(role_label.as_str()));
        drop(cfg);
        *agent_role = Some(id);
        {
            let mut g = handles
                .session_mode
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            *g = session_mode;
        }
        let cfg = cfg_holder.read().await.clone();
        if let Err(e) = apply_agent_role_switch_to_messages(
            &cfg,
            messages,
            Some(role_label.as_str()),
            &handles.process_handles.tool_outcome_recorder,
            Some(work_dir),
            None,
            Some(session_mode),
        ) {
            let _ = style.eprint_error(&e);
        } else {
            let _ = style.print_success(&format!(
                "已设当前角色为 \"{role_label}\"，会话模式 {}，已更新首条 system（保留对话 {} 条）。",
                session_mode.as_str(),
                messages.len()
            ));
        }
    }
    ReplSlashHandled::Handled
}
