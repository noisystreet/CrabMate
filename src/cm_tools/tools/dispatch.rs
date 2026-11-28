//! 工具执行分派：`run_tool*` / 前置校验 / 结果收尾 / `run_command` 异步路径。
//!
//! `#1530`：从 `tools/mod.rs` 拆出。

use super::command;
use super::specs_index::find_spec;
use super::{ToolContext, ToolRunner, ToolSpec, tool_spec_requires_fastembed};
use crate::cm_names as tool_names;
use crate::cm_tool_result::{ToolError, ToolResult};
use crate::cm_tools::tools::command::RunCommandError;
use crate::cm_tools::tools::{tool_args_validate, workspace_image_chat_hint};

/// 执行本地工具并返回结果字符串。
/// `ToolContext` 聚合 `run_command`、`get_weather`、`web_search` 等工具所需的配置项。
pub fn run_tool(name: &str, args_json: &str, ctx: &ToolContext<'_>) -> String {
    let (spec, args_eff) = match prepare_builtin_dispatch(name, args_json) {
        Ok(v) => v,
        Err(e) => return render_string_face_error(&e),
    };
    match spec.runner {
        // Typed 的成败由 `Result` 决定；`run_tool` 只返回 String，故在不一致时注入
        // 显式 `ok` 头，供下游 `parse_tool_output` 复用（见 `typed_outcome_output`）。
        ToolRunner::Typed(f) => match f(args_eff.as_ref(), ctx) {
            Ok(output) => typed_outcome_output(name, true, None, output),
            Err(e) => typed_outcome_output(name, false, Some(e.code.as_str()), e.message),
        },
        ToolRunner::AsyncOnly => async_only_sync_error(name),
    }
}

/// 与 [`run_tool`] 同源，但**不把成败拍平**（#1430）。
///
/// `run_tool` 在 `Result` 与 String 面不一致时会注入 `crabmate_tool_output` 头，让下游
/// `parse_tool_output` 再解析回来——一次往返。调用方本来就需要成败时用本函数。
///
/// 返回 `(正文, 成败, 错误码)`；`error_code` 为 `None` 时下游按 `{tool}_failed` 兜底。
pub fn run_tool_outcome(
    name: &str,
    args_json: &str,
    ctx: &ToolContext<'_>,
) -> (String, bool, Option<String>) {
    let (spec, args_eff) = match prepare_builtin_dispatch(name, args_json) {
        Ok(v) => v,
        Err(e) => {
            let output = render_string_face_error(&e);
            return (output, false, Some(e.code));
        }
    };
    match spec.runner {
        ToolRunner::Typed(f) => match f(args_eff.as_ref(), ctx) {
            Ok(output) => (output, true, None),
            Err(e) => (e.message, false, Some(e.code)),
        },
        // `AsyncOnly` 的同步入口本就是错误正文；错误码留给下游按 `{tool}_failed` 兜底。
        ToolRunner::AsyncOnly => (async_only_sync_error(name), false, None),
    }
}

/// [`prepare_builtin_dispatch`] 失败时，**String 面**的历史渲染。
///
/// 判定的是「`message` 是否自带用户可见前缀」，**不是**错误码语义：目前前置失败里
/// 只有 [`ToolError::unknown_tool`] 的正文自带 `未知工具：…`，其余一律要补 `错误：`。
///
/// **加新的前置错误时必须核对**：若新错误的 `message` 已自带 `错误：`
/// （[`ToolError::path_resolve_failed`] 的正文就是这样，见其构造器注释），要把它加进
/// `SELF_PREFIXED`，否则会输出 `错误：错误：…`。这是运行期字符串问题，编译期抓不到
/// —— 白名单写死在这里，就是为了让「新增一种前置错误」必须在此处过一遍。
fn render_string_face_error(e: &ToolError) -> String {
    /// 正文**自带**用户可见前缀、因此不能再叠 `错误：` 的错误码。
    const SELF_PREFIXED: &[&str] = &["unknown_tool"];
    if SELF_PREFIXED.contains(&e.code.as_str()) {
        e.message.clone()
    } else {
        format!("错误：{}", e.message)
    }
}

/// 执行前置：`fastembed` 特性门 → 注册表查找 → 入参宽容化 → schema 校验。
///
/// [`run_tool`]（String 面）与 [`run_tool_dispatch`]（`Result` 面）**共用本函数**，
/// 两者只在「错误如何渲染」上不同 —— 否则前置判定会有两份，漏改一处只表现为
/// 某个入面静默少了一项校验。
#[allow(clippy::result_large_err)]
fn prepare_builtin_dispatch<'a>(
    name: &str,
    args_json: &'a str,
) -> Result<(&'static ToolSpec, std::borrow::Cow<'a, str>), ToolError> {
    if !cfg!(feature = "fastembed") && tool_spec_requires_fastembed(name) {
        return Err(ToolError::invalid_args(
            "codebase_semantic_search 需要 `fastembed` Cargo feature；当前构建未启用。".to_string(),
        ));
    }
    let spec = find_spec(name).ok_or_else(|| ToolError::unknown_tool(name))?;
    let args_eff = tool_args_validate::effective_builtin_tool_args_json(name, args_json)
        .map_err(ToolError::invalid_args)?;
    if let Some(Err(e)) =
        tool_args_validate::validate_parsed_str_for_builtin(name, args_eff.as_ref())
    {
        return Err(ToolError::invalid_args(e));
    }
    Ok((spec, args_eff))
}

/// 按 `spec.runner` 执行：[`ToolRunner::Typed`] 显式透传 [`ToolError`]；
/// [`ToolRunner::AsyncOnly`] 在同步面无执行体，返回提示性 [`ToolError`]。
///
/// **`run_command` 不在本函数执行**：它经专用异步入口 [`run_tool_async`] / [`run_tool_result_async`]
/// 走 [`run_command_dispatch_async`]。
#[allow(clippy::result_large_err)]
pub(super) fn run_tool_dispatch(
    name: &str,
    args_json: &str,
    ctx: &ToolContext<'_>,
) -> Result<(String, crate::cm_tool_result::ParsedToolOutput), ToolError> {
    let (spec, args_eff) = prepare_builtin_dispatch(name, args_json)?;
    run_tool_dispatch_fallback(name, args_eff.as_ref(), spec, ctx)
}

/// dispatch 兜底：按 `spec.runner` 执行。
///
/// Typed：`Ok` 即成功（正文只用于抽取流/退出码），`Err(ToolError)` 显式透传；
/// AsyncOnly：同步面不可执行。
#[allow(clippy::result_large_err)]
fn run_tool_dispatch_fallback(
    name: &str,
    args_ref: &str,
    spec: &ToolSpec,
    ctx: &ToolContext<'_>,
) -> Result<(String, crate::cm_tool_result::ParsedToolOutput), ToolError> {
    match spec.runner {
        ToolRunner::Typed(f) => {
            f(args_ref, ctx).map(|output| finish_dispatch_typed_ok(name, args_ref, output))
        }
        ToolRunner::AsyncOnly => Err(ToolError::invalid_args(async_only_message(name))),
    }
}

/// 同步面无执行体时的统一说明（[`run_tool`] 前置「错误：」，[`run_tool_try`] 包成 [`ToolError`]）。
fn async_only_message(name: &str) -> String {
    format!("{name} 须由服务端异步调度执行。")
}

fn async_only_sync_error(name: &str) -> String {
    format!("错误：{}", async_only_message(name))
}

/// Typed 结果 → String 面：仅在正文启发式与真实成败/错误码不一致时注入显式头。
///
/// 主 Agent 路径（[`run_tool`]）丢失了 [`ToolError`]，下游 `dispatch_tool` 重试判定、
/// timeline / 模型信封、统计与验收又都用 `parse_tool_output` 重新推断。正文一致时
/// 保持逐字节不变；不一致时用 `crabmate_tool_output` 头声明真实成败与 `error_code`。
pub(super) fn typed_outcome_output(
    name: &str,
    ok: bool,
    error_code: Option<&str>,
    output: String,
) -> String {
    let parsed = crate::cm_tool_result::parse_tool_output(name, &output);
    if parsed.ok == ok && (ok || error_code.is_none() || parsed.error_code.as_deref() == error_code)
    {
        return output;
    }
    crate::cm_tool_result::prepend_crabmate_tool_output_with_ok_and_error_code(
        name,
        ok,
        error_code,
        serde_json::json!({}),
        &output,
    )
}

/// Typed 收尾：`Ok` 即成功，正文只用于抽取 `exit_code/stdout/stderr`。
///
/// 成功正文（如搜索结果）可能含「失败」「超时」等词，因此**不能**再把正文交给
/// [`crate::cm_tool_result::parse_tool_output`] 推断 `ok`；失败必须由执行体
/// 返回 [`ToolError`]。返回值同样注入显式 `ok` 头（若必要），保证下游再次解析一致。
pub(super) fn finish_dispatch_typed_ok(
    name: &str,
    args_json: &str,
    output: String,
) -> (String, crate::cm_tool_result::ParsedToolOutput) {
    let mut parsed = crate::cm_tool_result::parse_tool_output(name, &output);
    parsed.ok = true;
    parsed.error_code = None;
    if parsed.exit_code.is_some_and(|code| code != 0) {
        parsed.exit_code = None;
    }
    let output = typed_outcome_output(name, true, None, output);
    let output = workspace_image_chat_hint::append_if_needed(args_json, output);
    (output, parsed)
}

/// 文本收尾：先按原始正文推断成功/失败（**早于**图片提示附加），`ok` 时附加工作区图片提示，否则转 [`ToolError`]。异步 `run_command` 也复用本函数。
#[allow(clippy::result_large_err)]
fn finish_dispatch_text(
    name: &str,
    args_json: &str,
    output: String,
) -> Result<(String, crate::cm_tool_result::ParsedToolOutput), ToolError> {
    let parsed = crate::cm_tool_result::parse_tool_output(name, &output);
    if parsed.ok {
        let output = workspace_image_chat_hint::append_if_needed(args_json, output);
        Ok((output, parsed))
    } else {
        Err(ToolError::from_parsed(name, &parsed, output))
    }
}

/// 执行本地工具并返回结构化结果（兼容既有字符串输出）。
pub fn run_tool_result(name: &str, args_json: &str, ctx: &ToolContext<'_>) -> ToolResult {
    match run_tool_dispatch(name, args_json, ctx) {
        Ok((output, parsed)) => ToolResult::from_parsed(output, parsed),
        Err(e) => ToolResult::from_parsed(e.message, e.parsed),
    }
}

/// 异步工具执行入口（面向非 chat 消费点：tool-replay / MCP / workflow / 容器 runner）。
///
/// **`run_command`** 经本文件新增的异步实现 `run_command_dispatch_async`（[`run_tool_async`] 的 String 面）；
/// 其余工具直接委派既有同步 [`run_tool`]，行为逐字节保持不变。
///
/// `run_command` 实现体基于 [`tokio::process`] 事件驱动等待（含 `ctx.command_timeout_secs` 墙钟超时与测试缓存），
/// 不再占用阻塞线程。
///
/// **与 [`run_tool_result_async`] 的不对称是既有事实**（不是本函数引入）：非 `run_command`
/// 工具这里委派 **String 面** [`run_tool`]（不加工作区图片提示、不修正 `parsed.ok`），而
/// [`run_tool_result_async`] 委派 [`run_tool_result`] —— 后者经 `finish_dispatch_typed_ok`
/// **会**附加提示并修正。两侧消费者已分叉（`tool_sandbox/runner.rs` 用本函数，
/// `cm_workflow/execute/node.rs` 用 Result 面）。统一语义属 #1378 第二步；此处**不**擅自
/// 抹平，因为那会改变沙盒内拿到的正文。
pub async fn run_tool_async(name: &str, args_json: &str, ctx: &ToolContext<'_>) -> String {
    match try_run_command_dispatch_async(name, args_json, ctx).await {
        Some(Ok((output, _))) => output,
        Some(Err(e)) => e.message,
        None => run_tool(name, args_json, ctx),
    }
}

/// 异步版 [`run_tool_result`]：`run_command` 走 [`run_command_dispatch_async`]，其余委派同步路径。
///
/// 与 [`run_tool_async`] 的差异（Result 面 vs String 面）是**故意的**，理由见那边的说明。
pub async fn run_tool_result_async(
    name: &str,
    args_json: &str,
    ctx: &ToolContext<'_>,
) -> ToolResult {
    match try_run_command_dispatch_async(name, args_json, ctx).await {
        Some(Ok((output, parsed))) => ToolResult::from_parsed(output, parsed),
        Some(Err(e)) => ToolResult::from_parsed(e.message, e.parsed),
        None => run_tool_result(name, args_json, ctx),
    }
}

/// `run_command` 的异步分派：`Some(..)` 表示工具名命中、已由 [`run_command_dispatch_async`] 处理。
///
/// [`run_tool_async`] 与 [`run_tool_result_async`] 共用本函数、各自只做结果投影，
/// 因此 `RUN_COMMAND` 这条特判在文件内**只有一处**（此前两份，改一处漏一处的典型位置）。
#[allow(clippy::result_large_err)]
async fn try_run_command_dispatch_async(
    name: &str,
    args_json: &str,
    ctx: &ToolContext<'_>,
) -> Option<Result<(String, crate::cm_tool_result::ParsedToolOutput), ToolError>> {
    if name != tool_names::RUN_COMMAND {
        return None;
    }
    Some(run_command_dispatch_async(name, args_json, ctx).await)
}

/// 新的异步 `run_command` 实现（解析一次 `CommandSpec`，再以 [`command::run_async`] 异步等待）。
///
/// 实现体：解析一次 [`command::CommandSpec`] 后，以 [`tokio::process`] 异步等待
/// [`command::run_async`]（不占用阻塞线程）；
/// 借用字段（`allowed_commands` / `working_dir`）先 owned 化以延长生命周期；github token 经
/// [`with_request_github_token`](crate::cm_tools::github_token::with_request_github_token) 以 async 作用域注入
/// （命令构建与等待同一 async 任务，task-local 自然可用）；成功/失败收尾复用 [`finish_dispatch_text`]。
#[allow(clippy::result_large_err)]
pub(super) async fn run_command_dispatch_async(
    name: &str,
    args_json: &str,
    ctx: &ToolContext<'_>,
) -> Result<(String, crate::cm_tool_result::ParsedToolOutput), ToolError> {
    let name_owned = name.to_string();
    let spec = command::parse_command_spec(args_json, ctx.working_dir)
        .map_err(RunCommandError::into_tool_error)?;
    let max_out = ctx.command_max_output_len;
    let allowed: Vec<String> = ctx.allowed_commands.to_vec();
    let work_dir = ctx.working_dir.to_path_buf();
    let test_cache_enabled = ctx.test_result_cache_enabled;
    let test_cache_max = ctx.test_result_cache_max_entries;
    let timeout_secs = ctx.command_timeout_secs;
    let github_token = crate::cm_tools::github_token::resolve_token_plaintext();
    let fut = async move {
        let test_cache = test_cache_enabled.then_some(command::RunCommandTestCacheOpts {
            enabled: true,
            max_entries: test_cache_max,
            workspace_root: work_dir.as_path(),
        });
        let wait = crate::cm_tool_exec::SubprocessWaitCtl::with_wall_secs(timeout_secs);
        command::run_async(
            &spec,
            command::RunOptions {
                max_output_len: max_out,
                allowed_commands: &allowed,
                working_dir: work_dir.as_path(),
                test_cache,
                skip_arg_safety: false,
                wait: &wait,
            },
        )
        .await
        .map_err(RunCommandError::into_tool_error)
        .and_then(|outcome| {
            let rendered = outcome.render(max_out);
            match outcome.session_stop_tool_error(&rendered) {
                Some(e) => Err(e),
                None => finish_dispatch_text(&name_owned, &spec.args_json, rendered),
            }
        })
    };
    crate::cm_tools::github_token::with_request_github_token(github_token, fut).await
}
