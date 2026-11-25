//! 离线、不碰网络的运维子命令：`web-bearer` / `save-session` / `sse-replay`。
//!
//! 三者的共同点是「读本地状态 → 写 stdout / 钥匙串 → 退出」，不建 HTTP client，
//! 也不需要 `API_KEY`。

use crate::cm_config::cli::CliGlobals;
use crate::cm_config::cli::definitions::{
    BackfillLayoutCmd, SaveSessionCli, SaveSessionCmd, SseReplayCli, SseReplayCmd, WebBearerCli,
    WebBearerCmd, WebBearerSubCmd,
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

/// `crabmate backfill-layout`：用 `layout_meta_from_messages` 回填会话库的 `layout_meta_json`。
///
/// 默认 **dry-run**（只报告候选 / 可回填行数），`--apply` 才写盘；解析失败的行只报告、
/// **不删**。回填把 #1338 的 S3 前置从「等占比低于阈值」变成「库内 legacy 行 = 0」。
pub(super) fn run_backfill_layout(
    global: &CliGlobals,
    cmd: BackfillLayoutCmd,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = match cmd.db {
        Some(db) => std::path::PathBuf::from(db),
        None => {
            let cfg = super::config_load::load_cli_agent_config(
                global.config_path.as_deref(),
                global.llm_context_tokens,
            )?;
            std::path::PathBuf::from(
                cfg.conversation_persistence
                    .conversation_store_sqlite_path
                    .clone(),
            )
        }
    };
    let report = crate::conversation_store::backfill_layout_meta(&path, cmd.apply)?;
    println!("会话库：{}（共 {} 行）", path.display(), report.total);
    println!(
        "需要回填（无 layout / 坏 JSON / not v2）：{}；可回填 / 已回填：{}；\
         messages_json 解析失败跳过：{}",
        report.candidates, report.filled, report.skipped_invalid
    );
    if !report.applied {
        println!("dry-run：未写盘。确认后加 `--apply` 执行。");
        if report.skipped_invalid > 0 {
            return Err(backfill_bad_rows(&report));
        }
        return Ok(());
    }
    if report.ready_for_contract() {
        println!("回填完成：库内已无不可用的 layout 行（S3 删读路径的前置满足）。");
        return Ok(());
    }
    println!(
        "[!] 回填后仍有 {} 行不可用（解析失败行未被删除，需人工处理）。",
        report.remaining_legacy_rows()
    );
    Err(backfill_bad_rows(&report))
}

/// 判据不满足时的错误：带 `EXIT_GENERAL` 退出码，脚本据以拒绝「删读路径」。
fn backfill_bad_rows(
    report: &crate::conversation_store::LayoutBackfillReport,
) -> Box<dyn std::error::Error> {
    crate::cm_runtime::cli_exit::CliExitError::new(
        crate::cm_runtime::cli_exit::EXIT_GENERAL,
        format!(
            "回填判据未满足：{} 行 messages_json 解析失败（未被删除，需人工处理），另有 {} 行仍未回填",
            report.skipped_invalid,
            report.remaining_legacy_rows()
        ),
    )
    .into()
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

#[cfg(test)]
mod tests {
    use crate::cm_runtime::cli_exit::{CliExitError, EXIT_GENERAL};
    use crate::conversation_store::LayoutBackfillReport;

    use super::backfill_bad_rows;

    /// 判据未满足必须以 `CliExitError(EXIT_GENERAL)` 结束：裸字符串错误虽同样非零，
    /// 但 `main` 的 `downcast_ref` 分不出码，脚本也就没法据此区分失败原因。
    #[test]
    fn bad_rows_error_carries_general_exit_code() {
        let report = LayoutBackfillReport {
            total: 3,
            candidates: 3,
            filled: 2,
            skipped_invalid: 1,
            applied: true,
        };
        let err = backfill_bad_rows(&report);
        let cli = err
            .downcast_ref::<CliExitError>()
            .expect("must be CliExitError");
        assert_eq!(cli.code, EXIT_GENERAL);
        assert!(cli.message.contains("解析失败"), "{}", cli.message);
        // 回填后仍不可用的行 = candidates - filled（skipped 已含在内，不得重复相加）。
        assert!(cli.message.contains("另有 1 行"), "{}", cli.message);
    }
}
