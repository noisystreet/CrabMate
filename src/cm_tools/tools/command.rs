//! 有限的 Linux 命令执行工具（白名单、工作目录限制、无 shell 注入）

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::cm_tools::subprocess_session::SubprocessWaitCtl;

use super::test_result_cache::{
    TestCacheKey, TestCacheKind, fingerprint_rust_workspace_sources, try_get_cached,
    wrap_cache_hit,
};
use crate::cm_tools::tool_result::ToolError;

use super::command_outcome::{CommandDigest, CommandOutcome};

#[path = "command_shell_script.rs"]
mod command_shell_script;
#[path = "command_prepare_invocation.rs"]
mod prepare_invocation;
#[path = "command_rate_limit.rs"]
mod command_rate_limit;

use super::command_error::RunCommandError;
use super::command_exec::{execute_spawn_plan_async, execute_spawn_plan_sync};
use command_rate_limit::check_rate_limit;
use prepare_invocation::prepare_run_command_spec;

pub use command_rate_limit::reset_run_command_rate_limit_for_tests;
pub use prepare_invocation::{CommandSpec, parse_command_spec};
pub use prepare_invocation::scan_run_command_unsafe_args_spec;
pub(crate) use prepare_invocation::parse_run_command_payload_lenient;

pub use command_shell_script::{
    allowlist_allows_all, argv_has_shell_operators, argv_needs_posix_shell_wrap,
    argv_needs_shell_expansion, join_run_command_argv_tail, join_run_command_shell_script,
    peel_cd_prefix_argv_for_shell_policy, posix_shell_on_allowlist,
};

/// 可选：`cargo test …` 的进程内结果缓存（与内置 `cargo_test` 工具共用指纹逻辑）。
pub struct RunCommandTestCacheOpts<'a> {
    pub enabled: bool,
    pub max_entries: usize,
    pub workspace_root: &'a Path,
}

/// [`run`] / [`run_async`] 的执行选项（替代 6–7 个位置参数）。
pub struct RunOptions<'a> {
    pub max_output_len: usize,
    pub allowed_commands: &'a [String],
    pub working_dir: &'a Path,
    pub test_cache: Option<RunCommandTestCacheOpts<'a>>,
    /// 仅当调用方已完成工作区外路径人工审批后为 `true`。
    pub skip_arg_safety: bool,
    pub wait: &'a SubprocessWaitCtl,
}

fn cargo_test_argv_cache_eligible(cmd_args: &[String]) -> bool {
    if cmd_args.first().map(|s| s.as_str()) != Some("test") {
        return false;
    }
    for a in cmd_args {
        if a == "--nocapture" || a == "--test-threads" {
            return false;
        }
    }
    true
}

pub struct PreparedRunCommand {
    pub cmd_raw: String,
    pub cmd_name: String,
    pub exec_path: Option<PathBuf>,
    pub cmd_args: Vec<String>,
    /// `cd rel && …` 前缀展开后的进程工作目录（默认等于调用方传入的 `working_dir`）。
    pub effective_working_dir: PathBuf,
    /// 包装成 `bash -c` 前原命令为 `gh` 时仍注入 token。
    pub inject_gh_token: bool,
}

/// `terminal_session`（PTY）等路径：复用 `run_command` 同级别校验与白名单逻辑（不经 `Command::output`）。
/// `skip_arg_safety`：仅当 async 层已完成工作区外路径人工审批后为 `true`。
pub fn prepare_run_command_for_pty_spawn(
    args_json: &str,
    working_dir: &Path,
    allowed_commands: &[String],
    skip_arg_safety: bool,
) -> Result<PreparedRunCommand, RunCommandError> {
    let spec = parse_command_spec(args_json, working_dir)?;
    prepare_run_command_for_pty_spawn_spec(&spec, working_dir, allowed_commands, skip_arg_safety)
}

/// [`prepare_run_command_for_pty_spawn`] 的已解析版本；调用方不应再重复解析原始 JSON。
pub fn prepare_run_command_for_pty_spawn_spec(
    spec: &CommandSpec,
    working_dir: &Path,
    allowed_commands: &[String],
    skip_arg_safety: bool,
) -> Result<PreparedRunCommand, RunCommandError> {
    let p = prepare_run_command_spec(spec, working_dir, allowed_commands, skip_arg_safety)?;
    check_rate_limit()?;
    Ok(p)
}

/// `run_command` 执行计划：命中缓存直接返回正文，否则给出待 spawn 的 [`SpawnPlan`]。
#[allow(clippy::large_enum_variant)] // `SpawnPlan` 内嵌已配置的 `Command`，天然偏大；短生命周期，无需为其加间接层
enum RunCommandPlan {
    CacheHit(String),
    Spawn(SpawnPlan),
}

/// 待执行命令：`cmd` 已配好 argv / cwd / 环境；`cache_write` 非空时在成功退出后写回测试缓存。
pub(super) struct SpawnPlan {
    pub(super) cmd: Command,
    pub(super) cmd_label: String,
    pub(super) working_dir: PathBuf,
    pub(super) invocation: String,
    pub(super) cache_write: Option<CacheWritePlan>,
}

/// `cargo test` 结果缓存的写回参数（未命中且可缓存时携带）。
pub(super) struct CacheWritePlan {
    pub(super) enabled: bool,
    pub(super) max_entries: usize,
    pub(super) key: TestCacheKey,
}

/// 校验、限流并规划执行路径；sync / async 共用（[`run`] / [`run_async`]）。
fn plan_run_command_spec(
    spec: &CommandSpec,
    allowed_commands: &[String],
    working_dir: &Path,
    test_cache: Option<&RunCommandTestCacheOpts<'_>>,
    skip_arg_safety: bool,
) -> Result<RunCommandPlan, RunCommandError> {
    let prepared =
        prepare_run_command_spec(spec, working_dir, allowed_commands, skip_arg_safety)?;
    check_rate_limit()?;
    Ok(plan_prepared(prepared, test_cache))
}

fn plan_prepared(
    prepared: PreparedRunCommand,
    test_cache: Option<&RunCommandTestCacheOpts<'_>>,
) -> RunCommandPlan {
    let invocation = format_invocation_for_display(&prepared.cmd_raw, &prepared.cmd_args);
    if let Some(target_path) = prepared.exec_path.as_ref() {
        let mut cmd = Command::new(target_path);
        cmd.args(&prepared.cmd_args)
            .current_dir(prepared.effective_working_dir.as_path());
        return RunCommandPlan::Spawn(SpawnPlan {
            cmd,
            cmd_label: prepared.cmd_raw.clone(),
            working_dir: prepared.effective_working_dir.clone(),
            invocation,
            cache_write: None,
        });
    }
    if let Some(plan) = plan_cargo_test_cache(&prepared, test_cache, invocation.clone()) {
        return plan;
    }
    RunCommandPlan::Spawn(default_spawn_plan(prepared, invocation))
}

fn default_spawn_plan(prepared: PreparedRunCommand, invocation: String) -> SpawnPlan {
    let mut cmd = Command::new(&prepared.cmd_name);
    cmd.args(&prepared.cmd_args)
        .current_dir(&prepared.effective_working_dir);
    if prepared.inject_gh_token {
        crate::cm_tools::github_token::apply_gh_token_env(&mut cmd);
    } else {
        crate::cm_tools::github_token::apply_gh_token_env_if_gh_command(&mut cmd, &prepared.cmd_name);
    }
    SpawnPlan {
        cmd,
        cmd_label: prepared.cmd_name,
        working_dir: prepared.effective_working_dir,
        invocation,
        cache_write: None,
    }
}

/// `cargo test …` 命中缓存 → `CacheHit`；未命中但可缓存 → 带写回参数的 `Spawn`；不适用 → `None`。
fn plan_cargo_test_cache(
    prepared: &PreparedRunCommand,
    test_cache: Option<&RunCommandTestCacheOpts<'_>>,
    invocation: String,
) -> Option<RunCommandPlan> {
    let opts = test_cache?;
    if prepared.cmd_name != "cargo"
        || !opts.enabled
        || !cargo_test_argv_cache_eligible(&prepared.cmd_args)
    {
        return None;
    }
    let inputs_fp = fingerprint_rust_workspace_sources(opts.workspace_root)?;
    let args_fp = CommandDigest::from_invocation(
        prepared.cmd_name.clone(),
        prepared.cmd_args.clone(),
    )
    .with_cwd(prepared.effective_working_dir.as_path())
    .key();
    let key = TestCacheKey {
        workspace_root: opts.workspace_root.to_path_buf(),
        kind: TestCacheKind::CargoTestViaRunCommand,
        args_fingerprint: args_fp,
        inputs_fingerprint: inputs_fp.clone(),
    };
    if let Some(hit) = try_get_cached(opts.enabled, opts.max_entries, &key) {
        return Some(RunCommandPlan::CacheHit(wrap_cache_hit(&inputs_fp, &hit)));
    }
    let mut cmd = Command::new(&prepared.cmd_name);
    cmd.args(&prepared.cmd_args)
        .current_dir(prepared.effective_working_dir.as_path());
    Some(RunCommandPlan::Spawn(SpawnPlan {
        cmd,
        cmd_label: prepared.cmd_name.clone(),
        working_dir: prepared.effective_working_dir.clone(),
        invocation,
        cache_write: Some(CacheWritePlan {
            enabled: opts.enabled,
            max_entries: opts.max_entries,
            key,
        }),
    }))
}

/// 在指定工作目录下执行白名单内的 Linux 命令（同步等待）。
///
/// 返回 [`CommandOutcome`]：进程启动后的成功 / 非零退出 / 超时 / 取消都在 `status` 中；
/// `Err` 仅表示未启动（解析、策略、限流、spawn 失败）。
pub fn run(
    spec: &CommandSpec,
    opts: RunOptions<'_>,
) -> Result<CommandOutcome, RunCommandError> {
    match plan_run_command_spec(
        spec,
        opts.allowed_commands,
        opts.working_dir,
        opts.test_cache.as_ref(),
        opts.skip_arg_safety,
    )? {
        RunCommandPlan::CacheHit(body) => Ok(CommandOutcome::cached(body)),
        RunCommandPlan::Spawn(plan) => execute_spawn_plan_sync(plan, opts.max_output_len, opts.wait),
    }
}

/// [`run`] 的异步版：同一规划路径，spawn/wait 走 `tokio::process`。
pub async fn run_async(
    spec: &CommandSpec,
    opts: RunOptions<'_>,
) -> Result<CommandOutcome, RunCommandError> {
    match plan_run_command_spec(
        spec,
        opts.allowed_commands,
        opts.working_dir,
        opts.test_cache.as_ref(),
        opts.skip_arg_safety,
    )? {
        RunCommandPlan::CacheHit(body) => Ok(CommandOutcome::cached(body)),
        RunCommandPlan::Spawn(plan) => {
            execute_spawn_plan_async(plan, opts.max_output_len, opts.wait).await
        }
    }
}

/// 将 [`CommandOutcome`] 渲染为历史文本；超时 / 取消转回 `ToolError`。
#[allow(clippy::result_large_err)]
fn finish_legacy(outcome: &CommandOutcome, max_output_len: usize) -> Result<String, ToolError> {
    let rendered = outcome.render(max_output_len);
    match outcome.session_stop_tool_error(&rendered) {
        Some(e) => Err(e),
        None => Ok(rendered),
    }
}

fn options_for<'a>(
    max_output_len: usize,
    allowed_commands: &'a [String],
    working_dir: &'a Path,
    test_cache: Option<RunCommandTestCacheOpts<'a>>,
    skip_arg_safety: bool,
    wait: &'a SubprocessWaitCtl,
) -> RunOptions<'a> {
    RunOptions {
        max_output_len,
        allowed_commands,
        working_dir,
        test_cache,
        skip_arg_safety,
        wait,
    }
}

/// 已弃用：请先 `parse_command_spec`，再调用 [`run`]。
#[deprecated(note = "use parse_command_spec + run(&CommandSpec, RunOptions)")]
#[allow(dead_code)]
pub fn run_args_json(
    args_json: &str,
    max_output_len: usize,
    allowed_commands: &[String],
    working_dir: &Path,
    test_cache: Option<RunCommandTestCacheOpts<'_>>,
    skip_arg_safety: bool,
) -> String {
    let wait = SubprocessWaitCtl::default();
    let spec = match parse_command_spec(args_json, working_dir) {
        Ok(s) => s,
        Err(e) => return e.extended_user_message(),
    };
    match run(
        &spec,
        options_for(
            max_output_len,
            allowed_commands,
            working_dir,
            test_cache,
            skip_arg_safety,
            &wait,
        ),
    ) {
        Ok(outcome) => finish_legacy(&outcome, max_output_len)
            .unwrap_or_else(|e| e.message),
        Err(e) => e.extended_user_message(),
    }
}

/// 已弃用：请先 `parse_command_spec`，再调用 [`run`] 并映射错误。
#[deprecated(note = "use parse_command_spec + run(&CommandSpec, RunOptions)")]
#[allow(dead_code)]
#[allow(clippy::result_large_err)] // `ToolError` 含 legacy 解析快照，与 `run_tool_dispatch` 一致
pub fn run_try(
    args_json: &str,
    max_output_len: usize,
    allowed_commands: &[String],
    working_dir: &Path,
    test_cache: Option<RunCommandTestCacheOpts<'_>>,
    skip_arg_safety: bool,
) -> Result<String, ToolError> {
    let wait = SubprocessWaitCtl::default();
    let spec =
        parse_command_spec(args_json, working_dir).map_err(RunCommandError::into_tool_error)?;
    run(
        &spec,
        options_for(
            max_output_len,
            allowed_commands,
            working_dir,
            test_cache,
            skip_arg_safety,
            &wait,
        ),
    )
    .map_err(RunCommandError::into_tool_error)
    .and_then(|outcome| finish_legacy(&outcome, max_output_len))
}

/// 已弃用：请先 `parse_command_spec`，再调用 [`run`]。
#[deprecated(note = "use parse_command_spec + run(&CommandSpec, RunOptions)")]
#[allow(dead_code)]
#[allow(clippy::result_large_err)]
pub fn run_checked(
    args_json: &str,
    max_output_len: usize,
    allowed_commands: &[String],
    working_dir: &Path,
    test_cache: Option<RunCommandTestCacheOpts<'_>>,
    skip_arg_safety: bool,
) -> Result<String, ToolError> {
    let wait = SubprocessWaitCtl::default();
    let spec =
        parse_command_spec(args_json, working_dir).map_err(RunCommandError::into_tool_error)?;
    run(
        &spec,
        options_for(
            max_output_len,
            allowed_commands,
            working_dir,
            test_cache,
            skip_arg_safety,
            &wait,
        ),
    )
    .map_err(RunCommandError::into_tool_error)
    .and_then(|outcome| finish_legacy(&outcome, max_output_len))
}

/// 已弃用：请先 `parse_command_spec`，再调用 [`run`] 并传入 `wait`。
#[deprecated(note = "use parse_command_spec + run(&CommandSpec, RunOptions)")]
#[allow(dead_code)]
#[allow(clippy::result_large_err)]
pub fn run_checked_wait(
    args_json: &str,
    max_output_len: usize,
    allowed_commands: &[String],
    working_dir: &Path,
    test_cache: Option<RunCommandTestCacheOpts<'_>>,
    skip_arg_safety: bool,
    wait: &SubprocessWaitCtl,
) -> Result<String, ToolError> {
    let spec =
        parse_command_spec(args_json, working_dir).map_err(RunCommandError::into_tool_error)?;
    run(
        &spec,
        options_for(
            max_output_len,
            allowed_commands,
            working_dir,
            test_cache,
            skip_arg_safety,
            wait,
        ),
    )
    .map_err(RunCommandError::into_tool_error)
    .and_then(|outcome| finish_legacy(&outcome, max_output_len))
}

/// 已弃用：请先 `parse_command_spec`，再调用 [`run_async`]。
#[deprecated(note = "use parse_command_spec + run_async(&CommandSpec, RunOptions)")]
#[allow(dead_code)]
#[allow(clippy::result_large_err)]
pub async fn run_try_wait_async(
    args_json: &str,
    max_output_len: usize,
    allowed_commands: &[String],
    working_dir: &Path,
    test_cache: Option<RunCommandTestCacheOpts<'_>>,
    skip_arg_safety: bool,
    wait: &SubprocessWaitCtl,
) -> Result<String, ToolError> {
    let spec =
        parse_command_spec(args_json, working_dir).map_err(RunCommandError::into_tool_error)?;
    run_async(
        &spec,
        options_for(
            max_output_len,
            allowed_commands,
            working_dir,
            test_cache,
            skip_arg_safety,
            wait,
        ),
    )
    .await
    .map_err(RunCommandError::into_tool_error)
    .and_then(|outcome| finish_legacy(&outcome, max_output_len))
}

/// 已弃用：请先 `parse_command_spec`，再调用 [`run_async`]。
#[deprecated(note = "use parse_command_spec + run_async(&CommandSpec, RunOptions)")]
#[allow(dead_code)]
#[allow(clippy::result_large_err)]
pub async fn run_checked_wait_async(
    args_json: &str,
    max_output_len: usize,
    allowed_commands: &[String],
    working_dir: &Path,
    test_cache: Option<RunCommandTestCacheOpts<'_>>,
    skip_arg_safety: bool,
    wait: &SubprocessWaitCtl,
) -> Result<String, ToolError> {
    let spec =
        parse_command_spec(args_json, working_dir).map_err(RunCommandError::into_tool_error)?;
    run_async(
        &spec,
        options_for(
            max_output_len,
            allowed_commands,
            working_dir,
            test_cache,
            skip_arg_safety,
            wait,
        ),
    )
    .await
    .map_err(RunCommandError::into_tool_error)
    .and_then(|outcome| finish_legacy(&outcome, max_output_len))
}

/// 已弃用：请直接调用 [`run_async`]。
#[deprecated(note = "use run_async(&CommandSpec, RunOptions)")]
#[allow(dead_code)]
#[allow(clippy::result_large_err)]
pub async fn run_try_wait_async_spec(
    spec: &CommandSpec,
    max_output_len: usize,
    allowed_commands: &[String],
    working_dir: &Path,
    test_cache: Option<RunCommandTestCacheOpts<'_>>,
    skip_arg_safety: bool,
    wait: &SubprocessWaitCtl,
) -> Result<String, ToolError> {
    run_async(
        spec,
        options_for(
            max_output_len,
            allowed_commands,
            working_dir,
            test_cache,
            skip_arg_safety,
            wait,
        ),
    )
    .await
    .map_err(RunCommandError::into_tool_error)
    .and_then(|outcome| finish_legacy(&outcome, max_output_len))
}

/// 已弃用：请直接调用 [`run_async`]。
#[deprecated(note = "use run_async(&CommandSpec, RunOptions)")]
#[allow(dead_code)]
#[allow(clippy::result_large_err)]
pub async fn run_checked_wait_async_spec(
    spec: &CommandSpec,
    max_output_len: usize,
    allowed_commands: &[String],
    working_dir: &Path,
    test_cache: Option<RunCommandTestCacheOpts<'_>>,
    skip_arg_safety: bool,
    wait: &SubprocessWaitCtl,
) -> Result<String, ToolError> {
    run_async(
        spec,
        options_for(
            max_output_len,
            allowed_commands,
            working_dir,
            test_cache,
            skip_arg_safety,
            wait,
        ),
    )
    .await
    .map_err(RunCommandError::into_tool_error)
    .and_then(|outcome| finish_legacy(&outcome, max_output_len))
}

pub(crate) fn parse_run_command_value_with_repair(
    args_json: &str,
) -> Result<serde_json::Value, RunCommandError> {
    match serde_json::from_str::<serde_json::Value>(args_json) {
        Ok(v) => Ok(v),
        Err(primary_err) => {
            if let Some(repaired) = super::parse_args::try_repair_run_command_args_json(args_json)
                && let Ok(v) = serde_json::from_str::<serde_json::Value>(&repaired)
            {
                return Ok(v);
            }
            Err(RunCommandError::JsonParse(primary_err))
        }
    }
}

fn format_invocation_for_display(cmd_raw: &str, args: &[String]) -> String {
    if command_shell_script::is_shell_dash_c_invocation(cmd_raw, args)
        && let Some(script) = command_shell_script::dash_c_script_body(args)
    {
        let script = script.trim();
        if !script.is_empty() {
            return script.to_string();
        }
    }
    let cmd = cmd_raw.trim();
    if args.is_empty() {
        cmd.to_string()
    } else {
        format!("{} {}", cmd, args.join(" "))
    }
}

pub(super) fn check_shell_variable_references(
    cmd: &str,
    args: &[String],
) -> Result<(), RunCommandError> {
    for a in std::iter::once(cmd).chain(args.iter().map(String::as_str)) {
        if let Some(pattern) = command_shell_script::detect_shell_expansion_token(a) {
            return Err(RunCommandError::ShellVariableDetected {
                pattern: pattern.to_string(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "command_run_tests.rs"]
mod tests;
