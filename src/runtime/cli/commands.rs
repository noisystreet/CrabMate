//! `save-session` / `tool-replay` 子命令（不要求 `API_KEY`）。

use crate::config::AgentConfig;
use crate::config::cli::{SaveSessionCli, SaveSessionFormat, ToolReplayCli};
use crate::runtime::cli::{ReplExportKind, cli_effective_work_dir};
use crate::runtime::cli_exit::{CliExitError, EXIT_TOOL_REPLAY_MISMATCH, EXIT_USAGE};
use std::io::ErrorKind;
use std::path::PathBuf;

/// `crabmate tool-replay export|run`（不要求 API_KEY；重放路径与对话相同执行真实工具，须在可信工作区）。
pub fn run_tool_replay_command(
    cfg: &AgentConfig,
    workspace_cli: &Option<String>,
    cmd: ToolReplayCli,
) -> Result<(), Box<dyn std::error::Error>> {
    let workspace = cli_effective_work_dir(workspace_cli, &cfg.run_command_working_dir);
    match cmd {
        ToolReplayCli::Export {
            session_file,
            output,
            note,
        } => {
            let session_path = match session_file
                .as_ref()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
            {
                Some(p) => PathBuf::from(p),
                None => crate::runtime::workspace_session::session_file_path(&workspace),
            };
            if !session_path.is_file() {
                eprintln!("会话文件不存在: {}", session_path.display());
                return Err(std::io::Error::new(ErrorKind::NotFound, "会话文件不存在").into());
            }
            let out_path = output
                .as_ref()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(PathBuf::from);
            let note_ref = note.as_deref().map(str::trim).filter(|s| !s.is_empty());
            let written = crate::runtime::tool_replay::export_tool_replay_fixture(
                &session_path,
                &workspace,
                out_path.as_deref(),
                note_ref,
            )?;
            println!("{}", written.display());
        }
        ToolReplayCli::Run {
            fixture,
            compare_recorded,
        } => {
            let f = fixture.trim();
            if f.is_empty() {
                return Err(
                    CliExitError::new(EXIT_USAGE, "tool-replay run：--fixture 不能为空").into(),
                );
            }
            let fixture_path = PathBuf::from(f);
            if !fixture_path.is_file() {
                eprintln!("fixture 不存在: {}", fixture_path.display());
                return Err(std::io::Error::new(ErrorKind::NotFound, "fixture 不存在").into());
            }
            let mut buf = Vec::new();
            let (n_steps, mismatches) = crate::runtime::tool_replay::run_tool_replay_fixture(
                &fixture_path,
                cfg,
                &workspace,
                compare_recorded,
                &mut buf,
            )?;
            let text = String::from_utf8_lossy(&buf);
            print!("{text}");
            if compare_recorded && mismatches > 0 {
                return Err(
                    CliExitError::new(
                        EXIT_TOOL_REPLAY_MISMATCH,
                        format!(
                            "tool-replay：{mismatches} 条步骤与 recorded_output 不一致（共 {n_steps} 步）"
                        ),
                    )
                    .into(),
                );
            }
        }
    }
    Ok(())
}

/// `crabmate save-session`：从磁盘会话文件读取并写入导出目录（兼容别名 `export-session`）。
pub fn run_save_session_command(
    cfg: &AgentConfig,
    workspace_cli: &Option<String>,
    args: SaveSessionCli,
) -> Result<(), Box<dyn std::error::Error>> {
    let workspace = cli_effective_work_dir(workspace_cli, &cfg.run_command_working_dir);
    let session_path = match args
        .session_file
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        Some(p) => PathBuf::from(p),
        None => crate::runtime::workspace_session::session_file_path(&workspace),
    };
    if !session_path.is_file() {
        eprintln!("会话文件不存在: {}", session_path.display());
        return Err(std::io::Error::new(ErrorKind::NotFound, "会话文件不存在").into());
    }
    let data = std::fs::read_to_string(&session_path)?;
    let parsed: crate::runtime::chat_export::ChatSessionFile = serde_json::from_str(&data)
        .map_err(|e| std::io::Error::new(ErrorKind::InvalidData, format!("会话 JSON 无效: {e}")))?;
    let fmt = match args.format {
        SaveSessionFormat::Json => ReplExportKind::Json,
        SaveSessionFormat::Markdown => ReplExportKind::Markdown,
        SaveSessionFormat::Both => ReplExportKind::Both,
    };
    match fmt {
        ReplExportKind::Json => {
            let p = crate::runtime::workspace_session::export_json(&workspace, &parsed.messages)?;
            println!("{}", p.display());
        }
        ReplExportKind::Markdown => {
            let p =
                crate::runtime::workspace_session::export_markdown(&workspace, &parsed.messages)?;
            println!("{}", p.display());
        }
        ReplExportKind::Both => {
            let pj = crate::runtime::workspace_session::export_json(&workspace, &parsed.messages)?;
            let pm =
                crate::runtime::workspace_session::export_markdown(&workspace, &parsed.messages)?;
            println!("{}", pj.display());
            println!("{}", pm.display());
        }
    }
    Ok(())
}
