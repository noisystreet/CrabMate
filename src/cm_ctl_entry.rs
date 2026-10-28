//! `crabmate ctl` 在 CLI 入口的薄包装。
//!
//! 独立成文件的原因：`cli_run.rs` 已逼近单文件行数上限（`fn-nloc-ratchet.sh` 的 920 行），
//! 把这段分派逻辑挪出来可以避免继续压缩那个已经很拥挤的分派文件。
//!
//! 语义要点：`ctl` 是**短命命令**，不加载 LLM 配置、不要求 `API_KEY`、不进 agent 循环；
//! 失败一律转成 [`crate::CliExitError`]，以便 `ctl` 自己的 `exit_code`（1/2）能透出到进程退出码。

use crate::config::cli::CtlCmd;

/// 运行 `ctl` 子命令。
pub(crate) fn run_ctl(
    cmd: &CtlCmd,
    workspace_cli: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(unix)]
    {
        crate::cm_ctl::run(cmd, workspace_cli)
            .map_err(|e| crate::CliExitError::new(e.exit_code, e.to_string()))?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = (cmd, workspace_cli);
        Err(crate::CliExitError::new(
            crate::EXIT_GENERAL,
            "`crabmate ctl` 目前仅支持 Unix（Linux/macOS）",
        )
        .into())
    }
}
