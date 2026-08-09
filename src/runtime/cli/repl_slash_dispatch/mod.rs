//! REPL `/…` 命令分派：从 [`super::repl_extras::try_handle_repl_slash_command`] 拆出以降低单函数圈复杂度。

mod api_key;
mod llm;
mod misc;
mod session;
mod shared;

use crate::config::SharedAgentConfig;
use crate::runtime::cli::repl_parse::{ReplBuiltIn, print_repl_version_line};
use crate::runtime::cli_repl_ui::CliReplStyle;
use crate::types::Message;

use super::repl_extras::{ReplSlashHandled, ReplSlashSharedHandles};

use api_key::{
    slash_api_key_clear_persist, slash_api_key_set, slash_api_key_status, slash_api_key_usage,
};
use llm::{
    slash_api_base_set, slash_api_base_show, slash_api_base_usage, slash_model_set,
    slash_model_show, slash_model_usage,
};
use misc::{
    slash_config, slash_doctor, slash_export, slash_probe, slash_save_session, slash_skills_list,
    slash_tools_list, slash_workspace_set, slash_workspace_show,
};
use session::{
    slash_agent_list, slash_agent_set, slash_clear, slash_context, slash_mode_set, slash_mode_show,
};

#[allow(clippy::too_many_arguments)]
pub(super) async fn dispatch_repl_slash_builtin<'a>(
    builtin: ReplBuiltIn<'a>,
    cfg_holder: &SharedAgentConfig,
    tools: &[crate::types::Tool],
    messages: &mut Vec<Message>,
    work_dir: &mut std::path::PathBuf,
    style: &CliReplStyle,
    no_stream: bool,
    agent_role: &mut Option<String>,
    handles: &ReplSlashSharedHandles,
) -> ReplSlashHandled {
    match builtin {
        ReplBuiltIn::BareSlash => {
            let _ = style.print_line(
                "输入 /help 查看内建命令；若以 / 开头的文字要发给模型，请避免仅输入一个 /。",
            );
            ReplSlashHandled::Handled
        }
        ReplBuiltIn::Unknown(head) => {
            let cfg = cfg_holder.read().await;
            match crate::config::skills_slash::prepare_user_message_for_skills(
                // 整行由外层传入；此处仅用 head 探测——实际用 messages 路径会再 prepare。
                // 用假输入 /head 探测是否为 skill。
                &format!("/{head}"),
                cfg.skills.list_opts(work_dir.as_path()),
                cfg.skills.skills_enabled,
            ) {
                Ok(p) if p.forced_skill.is_some() => ReplSlashHandled::NotSlash,
                Ok(_) => {
                    let _ = style.eprint_error(&format!(
                        "未知命令 /{head}。输入 /help 查看内建命令；skill 用 /skills list 查看可调用 id。"
                    ));
                    ReplSlashHandled::Handled
                }
                Err(e) => {
                    let _ = style.eprint_error(&e.to_string());
                    ReplSlashHandled::Handled
                }
            }
        }
        ReplBuiltIn::Clear => {
            slash_clear(
                cfg_holder,
                messages,
                work_dir.as_path(),
                agent_role,
                style,
                &handles.process_handles.tool_outcome_recorder,
            )
            .await
        }
        ReplBuiltIn::ModelShow => slash_model_show(cfg_holder, style).await,
        ReplBuiltIn::ModelSet(name) => slash_model_set(name, cfg_holder, style).await,
        ReplBuiltIn::ModelUsage => slash_model_usage(style),
        ReplBuiltIn::ApiBaseShow => slash_api_base_show(cfg_holder, style).await,
        ReplBuiltIn::ApiBaseSet(url) => slash_api_base_set(url, cfg_holder, style).await,
        ReplBuiltIn::ApiBaseUsage => slash_api_base_usage(style),
        ReplBuiltIn::Config(extra) => {
            slash_config(
                extra,
                cfg_holder,
                work_dir.as_path(),
                tools,
                style,
                no_stream,
            )
            .await
        }
        ReplBuiltIn::Doctor(extra) => {
            slash_doctor(extra, cfg_holder, work_dir.as_path(), style).await
        }
        ReplBuiltIn::Probe(extra) => slash_probe(extra, style),
        ReplBuiltIn::ModelsList => ReplSlashHandled::RunModels,
        ReplBuiltIn::ModelsChoose(model_id) => ReplSlashHandled::RunModelsChoose { model_id },
        ReplBuiltIn::ModelsUsage => {
            let _ = style.eprint_error(
                "用法: /models · /models list（列模型）· /models choose <id>（从列表设当前 model；id 可唯一前缀）",
            );
            ReplSlashHandled::Handled
        }
        ReplBuiltIn::WorkspaceShow => slash_workspace_show(work_dir.as_path(), style),
        ReplBuiltIn::WorkspaceSet(arg) => {
            slash_workspace_set(cfg_holder, work_dir, arg, style).await
        }
        ReplBuiltIn::SkillsList => slash_skills_list(cfg_holder, work_dir.as_path(), style).await,
        ReplBuiltIn::Tools => slash_tools_list(tools, style),
        ReplBuiltIn::Help => {
            let _ = style.print_help();
            ReplSlashHandled::Handled
        }
        ReplBuiltIn::Export(arg) => slash_export(arg, work_dir.as_path(), messages, style),
        ReplBuiltIn::SaveSession(arg) => {
            slash_save_session(arg, cfg_holder, work_dir.as_path(), style).await
        }
        ReplBuiltIn::McpList { probe } => ReplSlashHandled::RunMcpList { probe },
        ReplBuiltIn::McpUnknown(tail) => {
            let _ = style.eprint_error(&format!(
                "未知 /mcp 子命令: {tail}。用法: /mcp · /mcp list · /mcp probe · /mcp list probe"
            ));
            ReplSlashHandled::Handled
        }
        ReplBuiltIn::AgentList => slash_agent_list(cfg_holder, agent_role, style).await,
        ReplBuiltIn::AgentSet(id) => {
            slash_agent_set(
                id,
                cfg_holder,
                messages,
                agent_role,
                style,
                handles,
                work_dir.as_path(),
            )
            .await
        }
        ReplBuiltIn::AgentUsage => {
            let _ = style.eprint_error(
                "用法: /agent · /agent list（列角色 id，含内建 default）· /agent set <id> | /agent set default（default=清除显式角色，回到与 Web 默认相同逻辑）",
            );
            ReplSlashHandled::Handled
        }
        ReplBuiltIn::ModeShow => slash_mode_show(handles, style),
        ReplBuiltIn::ModeSet(mode) => {
            slash_mode_set(
                mode,
                cfg_holder,
                messages,
                agent_role.as_deref(),
                style,
                handles,
                work_dir.as_path(),
            )
            .await
        }
        ReplBuiltIn::ModeUsage => {
            let _ = style.eprint_error("用法: /mode · /mode ask|plan|act");
            ReplSlashHandled::Handled
        }
        ReplBuiltIn::Version => {
            print_repl_version_line();
            ReplSlashHandled::Handled
        }
        ReplBuiltIn::Context => slash_context(cfg_holder, messages, style).await,
        ReplBuiltIn::ApiKeyUsage => slash_api_key_usage(style),
        ReplBuiltIn::ApiKeyStatus => {
            slash_api_key_status(cfg_holder, &handles.api_key_holder, style).await
        }
        ReplBuiltIn::ApiKeyClear { persist } => {
            slash_api_key_clear_persist(&handles.api_key_holder, style, persist)
        }
        ReplBuiltIn::ApiKeySet(secret) => slash_api_key_set(secret, &handles.api_key_holder, style),
    }
}
