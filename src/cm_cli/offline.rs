//! 离线、不碰网络的运维子命令：`web-bearer` / `save-session` / `sse-replay`。
//!
//! 三者的共同点是「读本地状态 → 写 stdout / 钥匙串 → 退出」，不建 HTTP client，
//! 也不需要 `API_KEY`。

use crate::cm_config::cli::CliGlobals;
use crate::cm_config::cli::definitions::{
    SaveSessionCli, SaveSessionCmd, SseReplayCli, SseReplayCmd, WebBearerCli, WebBearerCmd,
    WebBearerSubCmd,
};

/// `crabmate web-bearer status|set|clear`：系统钥匙串里的 Web API 共享密钥。
///
/// **不读配置**：密钥槽在 user-data / 钥匙串，配置损坏时也应能修。
pub(super) fn run_web_bearer(cmd: WebBearerCmd) -> Result<(), Box<dyn std::error::Error>> {
    let cli = match cmd.sub {
        WebBearerSubCmd::Status => WebBearerCli::Status,
        WebBearerSubCmd::Set(s) => WebBearerCli::Set {
            token: s.token,
            stdin: s.stdin,
            from_env: s.from_env,
        },
        WebBearerSubCmd::Clear => WebBearerCli::Clear,
    };
    crate::runtime::cli_web_bearer::run_web_bearer_command(cli)?;
    Ok(())
}

/// `crabmate save-session`（别名 `export-session`）：导出到 `<workspace>/.crabmate/exports/`。
pub(super) fn run_save_session(
    global: &CliGlobals,
    cmd: SaveSessionCmd,
) -> Result<(), Box<dyn std::error::Error>> {
    let cfg = super::config_load::load_cli_agent_config(
        global.config_path.as_deref(),
        global.llm_context_tokens,
    )?;
    let cli = SaveSessionCli {
        format: cmd.format,
        projection: cmd.projection,
        session_file: cmd.session_file,
    };
    crate::runtime::cli::run_save_session_command(&cfg, &global.workspace, cli)?;
    Ok(())
}

/// `crabmate sse-replay`：把录制的 AG-UI 事件回放成 `TurnLayout` 投影。
pub(super) fn run_sse_replay(cmd: SseReplayCmd) -> Result<(), Box<dyn std::error::Error>> {
    crate::runtime::cli::run_sse_replay_command(SseReplayCli {
        file: cmd.file,
        format: cmd.format,
        job_id: cmd.job_id,
    })?;
    Ok(())
}
