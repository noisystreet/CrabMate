//! `cargo <subcmd>`（check/clippy/run/test）的参数解析、CLI 拼装与执行。
//!
//! 执行走共享子进程会话（`subprocess_session`）：进程组 kill、并发排空、截断、会话统计；
//! `wall_secs = None` 表示无墙钟（保持既有「靠外圈 `spawn_blocking` timeout」语义）。
#![allow(clippy::result_large_err)] // `ToolError` 含 legacy 解析快照，与 `cargo_tools` 一致

use std::path::Path;
use std::process::Command;

use crate::cm_tools::subprocess_session::{self, SessionStopKind};
use crate::cm_tools::tool_result::ToolError;
use crate::cm_tools::tools::output_util;

use super::MAX_OUTPUT_LINES;

struct CargoSubCmdOpts<'a> {
    release: bool,
    all_targets: bool,
    package: Option<&'a str>,
    bin: Option<&'a str>,
    features: Option<&'a str>,
    test_filter: Option<&'a str>,
    no_capture: bool,
    run_args: Vec<serde_json::Value>,
}

fn parse_cargo_subcmd_opts(v: &serde_json::Value) -> Result<CargoSubCmdOpts<'_>, ToolError> {
    let package = v.get("package").and_then(|x| x.as_str()).map(str::trim);
    let bin = v.get("bin").and_then(|x| x.as_str()).map(str::trim);
    if let Some(p) = package
        && (p.is_empty() || p.contains(char::is_whitespace))
    {
        return Err(ToolError::invalid_args(
            "错误：package 参数无效".to_string(),
        ));
    }
    if let Some(b) = bin
        && (b.is_empty() || b.contains(char::is_whitespace))
    {
        return Err(ToolError::invalid_args("错误：bin 参数无效".to_string()));
    }
    Ok(CargoSubCmdOpts {
        release: v.get("release").and_then(|x| x.as_bool()).unwrap_or(false),
        all_targets: v
            .get("all_targets")
            .and_then(|x| x.as_bool())
            .unwrap_or(false),
        package,
        bin,
        features: v
            .get("features")
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty()),
        test_filter: v
            .get("test_filter")
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty()),
        no_capture: v
            .get("nocapture")
            .and_then(|x| x.as_bool())
            .unwrap_or(false),
        run_args: v
            .get("args")
            .and_then(|x| x.as_array())
            .cloned()
            .unwrap_or_default(),
    })
}

fn push_cargo_subcmd_cli(cmd: &mut Command, subcmd: &str, o: &CargoSubCmdOpts<'_>) {
    cmd.arg(subcmd);
    if o.release {
        cmd.arg("--release");
    }
    if o.all_targets && matches!(subcmd, "check" | "clippy") {
        cmd.arg("--all-targets");
    }
    push_opt_str(cmd, "--package", o.package);
    push_opt_str(cmd, "--bin", o.bin);
    push_opt_str(cmd, "--features", o.features);
    push_cargo_subcmd_tail_flags(cmd, subcmd, o);
}

/// 有值时追加 `--flag <value>`。
fn push_opt_str(cmd: &mut Command, flag: &str, v: Option<&str>) {
    if let Some(v) = v {
        cmd.arg(flag).arg(v);
    }
}

/// `cargo test` 过滤器 / `cargo run` 透传参数。
fn push_cargo_subcmd_tail_flags(cmd: &mut Command, subcmd: &str, o: &CargoSubCmdOpts<'_>) {
    if subcmd == "test" {
        if let Some(filter) = o.test_filter {
            cmd.arg(filter);
        }
        if o.no_capture {
            cmd.arg("--").arg("--nocapture");
        }
    } else if subcmd == "run" && !o.run_args.is_empty() {
        cmd.arg("--");
        for a in &o.run_args {
            if let Some(s) = a.as_str() {
                cmd.arg(s);
            }
        }
    }
}

pub(super) fn run_cargo_subcommand_str_try(
    subcmd: &str,
    args_json: &str,
    workspace_root: &Path,
    max_output_len: usize,
    wall_secs: Option<u64>,
) -> Result<String, ToolError> {
    let v = crate::cm_tools::tools::parse_args_json(args_json).map_err(ToolError::invalid_args)?;
    run_cargo_subcommand_value_try(subcmd, &v, workspace_root, max_output_len, wall_secs)
}

pub(super) fn run_cargo_subcommand_value_try(
    subcmd: &str,
    v: &serde_json::Value,
    workspace_root: &Path,
    max_output_len: usize,
    wall_secs: Option<u64>,
) -> Result<String, ToolError> {
    if !workspace_root.join("Cargo.toml").is_file() {
        return Err(ToolError::workspace(
            "workspace_no_cargo_toml",
            "错误：当前工作目录未找到 Cargo.toml".to_string(),
        ));
    }

    let o = parse_cargo_subcmd_opts(v)?;
    let mut cmd = Command::new("cargo");
    push_cargo_subcmd_cli(&mut cmd, subcmd, &o);
    cmd.current_dir(workspace_root);
    let tool_code = format!("cargo_{}", subcmd);
    run_and_format_try(
        cmd,
        max_output_len,
        &format!("cargo {}", subcmd),
        &tool_code,
        wall_secs,
    )
}

/// 经共享子进程会话运行并格式化输出（进程组 kill、截断、会话统计）；`wall_secs = None` 表示无墙钟。
pub(super) fn run_and_format_try(
    cmd: Command,
    max_output_len: usize,
    title: &str,
    tool_code: &str,
    wall_secs: Option<u64>,
) -> Result<String, ToolError> {
    match subprocess_session::run_and_capture(cmd, max_output_len, wall_secs) {
        Ok(session) => match session.kind {
            SessionStopKind::Exited => {
                let exit = session.status.and_then(|s| s.code()).unwrap_or(-1);
                let partial = output_util::merge_process_output(
                    &std::process::Output {
                        status: session.status.unwrap_or_default(),
                        stdout: session.stdout,
                        stderr: session.stderr,
                    },
                    output_util::ProcessOutputMerge::ConcatStdoutStderr,
                );
                let message = output_util::format_exited_command_output(
                    title,
                    exit,
                    &partial,
                    max_output_len,
                    MAX_OUTPUT_LINES,
                );
                if session.status.is_some_and(|s| s.success()) {
                    Ok(message)
                } else {
                    Err(ToolError::cargo_subcommand_failed(tool_code, exit, message))
                }
            }
            SessionStopKind::Timeout | SessionStopKind::Cancelled => {
                let stdout = String::from_utf8_lossy(&session.stdout);
                let stderr = String::from_utf8_lossy(&session.stderr);
                let (code, head) = if matches!(session.kind, SessionStopKind::Timeout) {
                    (
                        "timeout",
                        format!(
                            "{} 命令执行超时（{} 秒）；子进程已发送终止信号。",
                            title,
                            wall_secs.unwrap_or_default()
                        ),
                    )
                } else {
                    ("cancelled", format!("{} 命令已取消；子进程已发送终止信号。", title))
                };
                Err(ToolError::session_stop(
                    code,
                    format!(
                        "{head}\n{}",
                        output_util::format_marked_streams_block(
                            &stdout,
                            &stderr,
                            max_output_len,
                            MAX_OUTPUT_LINES,
                        )
                    ),
                ))
            }
        },
        Err(e) => Err(ToolError::subprocess_spawn_error(title, e)),
    }
}
