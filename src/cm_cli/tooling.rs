//! 工具面运维子命令：`tool-replay`（导出/重放）与 `plugin`（动态工具模板）。
//!
//! 两者都要读配置（工具表、插件目录在配置里），但不初始化会话句柄。

use crate::cm_config::cli::CliGlobals;
use crate::cm_config::cli::definitions::{
    PluginCmd, PluginInitCli, PluginListCli, PluginSubCmd, PluginValidateCli, ToolReplayCli,
    ToolReplayCmd, ToolReplaySubCmd,
};

/// `crabmate tool-replay export|run`：工具调用时间线导出与重放。
pub(super) async fn run_tool_replay(
    global: &CliGlobals,
    cmd: ToolReplayCmd,
) -> Result<(), Box<dyn std::error::Error>> {
    let cfg = super::config_load::load_cli_agent_config(
        global.config_path.as_deref(),
        global.llm_context_tokens,
    )?;
    let cli = match cmd.sub {
        ToolReplaySubCmd::Export(e) => ToolReplayCli::Export {
            session_file: e.session_file,
            output: e.output,
            note: e.note,
        },
        ToolReplaySubCmd::Run(r) => ToolReplayCli::Run {
            fixture: r.fixture,
            compare_recorded: r.compare_recorded,
        },
    };
    crate::runtime::cli::run_tool_replay_command(&cfg, &global.workspace, cli).await?;
    Ok(())
}

/// `crabmate plugin init|validate|list`：工作区 `plugins/*.json` 的模板与校验。
pub(super) fn run_plugin(
    global: &CliGlobals,
    cmd: PluginCmd,
) -> Result<(), Box<dyn std::error::Error>> {
    let cfg = super::config_load::load_cli_agent_config(
        global.config_path.as_deref(),
        global.llm_context_tokens,
    )?;
    match cmd.sub {
        PluginSubCmd::Init(i) => crate::runtime::cli::run_plugin_init_command(
            &cfg,
            &global.workspace,
            PluginInitCli {
                name: i.name,
                description: i.description,
                command: i.command,
                args: i.args,
                pass_args_json: i.pass_args_json,
                output: i.output,
            },
        )?,
        PluginSubCmd::Validate(v) => crate::runtime::cli::run_plugin_validate_command(
            &cfg,
            &global.workspace,
            PluginValidateCli {
                file: v.file,
                json: v.json,
                jsonl: v.jsonl,
            },
        )?,
        PluginSubCmd::List(l) => crate::runtime::cli::run_plugin_list_command(
            &cfg,
            &global.workspace,
            PluginListCli {
                file: l.file,
                json: l.json,
                jsonl: l.jsonl,
            },
        )?,
    }
    Ok(())
}
