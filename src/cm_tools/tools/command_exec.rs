//! `run_command` 进程执行与输出格式化（同步 / 异步共用）。
//!
//! 进程启动后的结束状态统一收敛到 [`CommandOutcome`]；启动失败仍返回 [`RunCommandError`]。

use std::path::Path;
use std::process::Command;

use crate::cm_tools::subprocess_session::{
    SessionStopKind, SessionWaitResult, SubprocessWaitCtl, prepare_piped_process_group,
    wait_child_session, wait_child_session_async,
};

use super::command_outcome::{CommandOutcome, ExitStatusKind};
use super::test_result_cache::store_cached;

use super::command::{CacheWritePlan, SpawnPlan};
use super::command_error::{RunCommandError, map_spawn_error};

pub(super) fn execute_spawn_plan_sync(
    plan: SpawnPlan,
    max_output_len: usize,
    wait: &SubprocessWaitCtl,
) -> Result<CommandOutcome, RunCommandError> {
    let SpawnPlan {
        mut cmd,
        cmd_label,
        working_dir,
        invocation,
        cache_write,
        rewrites,
    } = plan;
    let outcome = run_spawned_command(
        &mut cmd,
        &cmd_label,
        &working_dir,
        &invocation,
        max_output_len,
        wait,
    )?
    .with_rewrites(rewrites);
    write_test_cache(cache_write, &outcome, max_output_len);
    Ok(outcome)
}

pub(super) async fn execute_spawn_plan_async(
    plan: SpawnPlan,
    max_output_len: usize,
    wait: &SubprocessWaitCtl,
) -> Result<CommandOutcome, RunCommandError> {
    let SpawnPlan {
        cmd,
        cmd_label,
        working_dir,
        invocation,
        cache_write,
        rewrites,
    } = plan;
    let outcome = run_spawned_command_async(
        cmd,
        &cmd_label,
        &working_dir,
        &invocation,
        max_output_len,
        wait,
    )
    .await?
    .with_rewrites(rewrites);
    write_test_cache(cache_write, &outcome, max_output_len);
    Ok(outcome)
}

fn write_test_cache(
    cache_write: Option<CacheWritePlan>,
    outcome: &CommandOutcome,
    max_output_len: usize,
) {
    // 与历史行为一致：仅正常退出（含非零退出码）写回缓存；超时 / 取消不写。
    if !matches!(outcome.status, ExitStatusKind::Exited { .. }) {
        return;
    }
    if let Some(w) = cache_write {
        store_cached(
            w.enabled,
            w.max_entries,
            w.key,
            outcome.render_body(max_output_len),
        );
    }
}

fn run_spawned_command(
    cmd: &mut Command,
    cmd_label: &str,
    working_dir: &Path,
    invocation: &str,
    max_output_len: usize,
    wait: &SubprocessWaitCtl,
) -> Result<CommandOutcome, RunCommandError> {
    prepare_piped_process_group(cmd);
    let child = cmd
        .spawn()
        .map_err(|e| map_spawn_error(cmd_label, working_dir, e))?;
    let session = wait_child_session(child, wait, max_output_len)
        .map_err(|e| map_spawn_error(cmd_label, working_dir, e))?;
    Ok(finish_session(session, invocation, wait))
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
) -> Result<CommandOutcome, RunCommandError> {
    prepare_piped_process_group(&mut cmd);
    let mut cmd = tokio::process::Command::from(cmd);
    cmd.kill_on_drop(true);
    let child = cmd
        .spawn()
        .map_err(|e| map_spawn_error(cmd_label, working_dir, e))?;
    let session = wait_child_session_async(child, wait, max_output_len)
        .await
        .map_err(|e| map_spawn_error(cmd_label, working_dir, e))?;
    Ok(finish_session(session, invocation, wait))
}

fn finish_session(
    session: SessionWaitResult,
    invocation: &str,
    wait: &SubprocessWaitCtl,
) -> CommandOutcome {
    let stdout = String::from_utf8_lossy(&session.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&session.stderr).into_owned();
    let status = match session.kind {
        SessionStopKind::Exited => ExitStatusKind::Exited {
            code: session.status.and_then(|s| s.code()).unwrap_or(-1),
        },
        SessionStopKind::Timeout => ExitStatusKind::TimedOut {
            timeout_secs: wait.wall.map(|d| d.as_secs()).unwrap_or(0),
        },
        SessionStopKind::Cancelled => ExitStatusKind::Cancelled,
    };
    CommandOutcome::new(invocation.to_string(), status, stdout, stderr)
}
