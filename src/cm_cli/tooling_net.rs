//! 会碰网络或起服务的工具面子命令：`workflow`（run 调 LLM）/ `mcp`（stdio server）/ `e2e`。
//!
//! 与 [`super::tooling`] 的分界：那一组只读写本地文件，这一组可能发起外部调用。

use std::path::PathBuf;

use crate::cm_config::cli::CliGlobals;
use crate::cm_config::cli::definitions::{
    E2eCmd, McpCmd, McpSubCmd, WorkflowCmd, WorkflowFileCli, WorkflowSubCmd,
};

/// `workflow validate|compile|run`：作者层 YAML/Markdown 的校验、编译与执行。
///
/// `validate` / `compile` 是纯静态检查，**不读配置**；只有 `run` 需要配置与工作区。
pub(super) async fn run_workflow(
    global: &CliGlobals,
    cmd: WorkflowCmd,
) -> Result<(), Box<dyn std::error::Error>> {
    fn usage_err<E: Into<String>>(e: E) -> Box<dyn std::error::Error> {
        Box::new(crate::CliExitError::new(2, e))
    }
    match cmd.sub {
        WorkflowSubCmd::Validate(v) => {
            crate::runtime::cli_workflow::run_workflow_validate_command(&WorkflowFileCli {
                file: v.file,
                json: v.json,
            })
            .map_err(usage_err)?;
        }
        WorkflowSubCmd::Compile(c) => {
            crate::runtime::cli_workflow::run_workflow_compile_command(&WorkflowFileCli {
                file: c.file,
                json: c.json,
            })
            .map_err(usage_err)?;
        }
        WorkflowSubCmd::Run(r) => {
            let cfg = super::config_load::load_cli_agent_config(
                global.config_path.as_deref(),
                global.llm_context_tokens,
            )?;
            crate::runtime::cli_workflow::run_workflow_run_command(
                &cfg,
                &global.workspace,
                WorkflowFileCli {
                    file: r.file,
                    json: r.json,
                },
            )
            .await
            .map_err(usage_err)?;
        }
    }
    Ok(())
}

/// `crabmate mcp list|serve`：MCP stdio 客户端运维。
pub(super) async fn run_mcp(
    global: &CliGlobals,
    cmd: McpCmd,
) -> Result<(), Box<dyn std::error::Error>> {
    let cfg = super::config_load::load_cli_agent_config(
        global.config_path.as_deref(),
        global.llm_context_tokens,
    )?;
    match cmd.sub {
        McpSubCmd::List(l) => crate::runtime::cli_mcp::run_mcp_list(&cfg, l.probe).await,
        McpSubCmd::Serve(s) => {
            crate::runtime::cli_mcp::run_mcp_serve(&cfg, &global.workspace, s.no_tools, s.port)
                .await
                .map_err(std::io::Error::other)?
        }
    }
    Ok(())
}

/// `crabmate e2e`：真实 LLM 端到端测试（`--mode real|record|replay`）。
pub(super) async fn run_e2e(
    global: &CliGlobals,
    cmd: E2eCmd,
) -> Result<(), Box<dyn std::error::Error>> {
    let cfg = super::config_load::load_cli_agent_config(
        global.config_path.as_deref(),
        global.llm_context_tokens,
    )?;
    let api_key = super::config_load::read_llm_api_key_from_env_lenient(&cfg);

    let mode = match cmd.mode.as_str() {
        "record" => crate::cm_llm::E2eMode::Record,
        "replay" => crate::cm_llm::E2eMode::Replay,
        _ => crate::cm_llm::E2eMode::Real,
    };

    let artifacts_root = cmd
        .output_dir
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".crabmate/e2e_artifacts"));
    let recordings_dir = cmd
        .recordings_dir
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("tests/fixtures/llm_recordings"));
    let scenarios_file = cmd.scenarios_file.as_deref().map(PathBuf::from);

    let run_cfg = crate::e2e_scenario::E2eRunConfig {
        api_key,
        artifacts_root,
        recordings_dir,
        mode,
        judge_config: crate::e2e_scenario::JudgeConfig {
            enabled: cmd.judge,
            ..Default::default()
        },
    };
    crate::e2e_scenario::run_e2e_cli(&run_cfg, scenarios_file.as_deref()).await?;
    Ok(())
}
