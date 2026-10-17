//! `run_command` 进程执行与输出格式化（同步 / 异步共用）。

use std::path::Path;
use std::process::Command;

use crate::cm_tools::subprocess_session::{
    SessionStopKind, SessionWaitResult, SubprocessWaitCtl, prepare_piped_process_group,
    wait_child_session, wait_child_session_async,
};

use super::output_util;
use super::test_result_cache::store_cached;

use super::command::{CacheWritePlan, SpawnPlan};
use super::command_error::{RunCommandError, map_spawn_error};

pub(super) const MAX_OUTPUT_LINES: usize = 500;

pub(super) fn execute_spawn_plan_sync(
    plan: SpawnPlan,
    max_output_len: usize,
    wait: &SubprocessWaitCtl,
) -> Result<String, RunCommandError> {
    let SpawnPlan {
        mut cmd,
        cmd_label,
        working_dir,
        invocation,
        cache_write,
    } = plan;
    let formatted = run_spawned_command(
        &mut cmd,
        &cmd_label,
        &working_dir,
        &invocation,
        max_output_len,
        wait,
    )?;
    write_test_cache(cache_write, &formatted);
    Ok(formatted)
}

pub(super) async fn execute_spawn_plan_async(
    plan: SpawnPlan,
    max_output_len: usize,
    wait: &SubprocessWaitCtl,
) -> Result<String, RunCommandError> {
    let SpawnPlan {
        cmd,
        cmd_label,
        working_dir,
        invocation,
        cache_write,
    } = plan;
    let formatted = run_spawned_command_async(
        cmd,
        &cmd_label,
        &working_dir,
        &invocation,
        max_output_len,
        wait,
    )
    .await?;
    write_test_cache(cache_write, &formatted);
    Ok(formatted)
}

fn write_test_cache(cache_write: Option<CacheWritePlan>, formatted: &str) {
    if let Some(w) = cache_write {
        store_cached(w.enabled, w.max_entries, w.key, formatted.to_string());
    }
}

fn format_command_streams(
    invocation: &str,
    exit_code: i32,
    stdout: &str,
    stderr: &str,
    max_output_len: usize,
) -> String {
    let truncate =
        |s: &str| output_util::truncate_output_lines(s, max_output_len, MAX_OUTPUT_LINES);
    let mut out = format!("命令：{invocation}\n退出码：{exit_code}\n");
    if !stdout.is_empty() {
        out.push_str("标准输出：\n");
        out.push_str(&truncate(stdout));
    }
    if !stderr.is_empty() {
        out.push_str("标准错误：\n");
        out.push_str(&truncate(stderr));
    }
    if stdout.is_empty() && stderr.is_empty() && exit_code == 0 {
        out.push_str("(无输出)");
    }
    out.trim_end().to_string()
}

fn run_spawned_command(
    cmd: &mut Command,
    cmd_label: &str,
    working_dir: &Path,
    invocation: &str,
    max_output_len: usize,
    wait: &SubprocessWaitCtl,
) -> Result<String, RunCommandError> {
    prepare_piped_process_group(cmd);
    let child = cmd
        .spawn()
        .map_err(|e| map_spawn_error(cmd_label, working_dir, e))?;
    let session = wait_child_session(child, wait, max_output_len)
        .map_err(|e| map_spawn_error(cmd_label, working_dir, e))?;
    finish_session(session, invocation, max_output_len, wait)
}

/// 异步版 [`run_spawned_command`]：`std::process::Command` 转 `tokio::process::Command` 后 await。
///
/// `kill_on_drop(true)` 保证 session 被未来取消时子进程组随 drop 终止。
async fn run_spawned_command_async(
    mut cmd: Command,
    cmd_label: &str,
    working_dir: &Path,
    invocation: &str,
    max_output_len: usize,
    wait: &SubprocessWaitCtl,
) -> Result<String, RunCommandError> {
    prepare_piped_process_group(&mut cmd);
    let mut cmd = tokio::process::Command::from(cmd);
    cmd.kill_on_drop(true);
    let child = cmd
        .spawn()
        .map_err(|e| map_spawn_error(cmd_label, working_dir, e))?;
    let session = wait_child_session_async(child, wait, max_output_len)
        .await
        .map_err(|e| map_spawn_error(cmd_label, working_dir, e))?;
    finish_session(session, invocation, max_output_len, wait)
}

fn finish_session(
    session: SessionWaitResult,
    invocation: &str,
    max_output_len: usize,
    wait: &SubprocessWaitCtl,
) -> Result<String, RunCommandError> {
    let stdout = String::from_utf8_lossy(&session.stdout);
    let stderr = String::from_utf8_lossy(&session.stderr);
    let exit_code = session.status.and_then(|s| s.code()).unwrap_or(-1);
    let body = format_command_streams(invocation, exit_code, &stdout, &stderr, max_output_len);
    match session.kind {
        SessionStopKind::Exited => Ok(body),
        SessionStopKind::Timeout => {
            let timeout_secs = wait.wall.map(|d| d.as_secs()).unwrap_or(0);
            let formatted = format!(
                "命令执行超时（{timeout_secs} 秒）；子进程已发送终止信号。\n{body}"
            );
            Err(RunCommandError::TimedOut {
                timeout_secs,
                formatted,
            })
        }
        SessionStopKind::Cancelled => {
            let formatted = format!("命令已取消；子进程已发送终止信号。\n{body}");
            Err(RunCommandError::Cancelled { formatted })
        }
    }
}
