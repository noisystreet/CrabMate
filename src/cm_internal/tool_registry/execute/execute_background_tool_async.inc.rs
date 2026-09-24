// 非 `run_command` 工具的 `async=true` 后台任务门闩（契约 §1.2）。
//
// 载荷一律为**进程载荷**：从各工具自身的参数校验逻辑装配 argv，复用 `tool_jobs` 既有的
// 取消（进程组 SIGTERM→SIGKILL）、墙钟超时与输出流式侧表；`tool_jobs` 侧零改动。
// `run_command` 不走本文件（见 `execute_run_command_async.inc.rs`）。

const BACKGROUND_ASYNC_DISABLED_MSG: &str = "错误：后台任务（async）未启用（[tool_registry] background_jobs_enabled=false）。请启用后台任务并确保该工具已在 `background_job_async_tools` 白名单中，或去掉 async 参数。";
const BACKGROUND_ASYNC_NO_REGISTRY_MSG: &str =
    "错误：当前执行环境不支持后台任务（async）。请去掉 async 参数。";
const BACKGROUND_ASYNC_DOCKER_MSG: &str = "错误：后台任务（async）暂不支持 Docker 同步工具沙盒模式；请使用宿主模式或去掉 async 参数。";

/// 后台 argv 装配函数签名：`(args_json, workspace_root) → 可复刻命令快照`。
type BackgroundArgvAssembler =
    fn(&str, &Path) -> Result<crate::cm_tools::tools::AssembledCommand, String>;

/// **装配表（唯一事实来源）**：支持后台执行（进程载荷）的内置工具 → 其 argv 装配函数。
///
/// 门闩的 `supported` 判定与 [`assemble_background_job_spawn`] 的装配共用本表，
/// 避免「支持清单」与「装配分支」两处漂移；新增工具只需在此登记一行。
fn background_async_argv_assembler(name: &str) -> Option<BackgroundArgvAssembler> {
    match name {
        "cargo_test" => Some(|args_json, workspace_root| {
            crate::cm_tools::tools::cargo_tools::cargo_subcommand_background_argv(
                "test",
                args_json,
                workspace_root,
            )
            .map_err(|e| e.message)
        }),
        "pytest_run" => Some(crate::cm_tools::tools::python_tools::pytest_run_background_argv),
        _ => None,
    }
}

/// 门闩判定结果。
enum BackgroundAsyncGate {
    /// 不接管该调用（保持既有「忽略未知参数」语义）。
    Skip,
    /// 接管并直接返回该错误文本。
    Deny(String),
    /// 接管并装配后台任务。
    Launch,
}

/// 非 `run_command` 工具的 `async=true` 后台路径入口。
///
/// 返回 `Some` 表示该调用已被接管（已发起后台任务，或返回错误帧）；`None` 表示不拦截。
fn try_dispatch_background_async_tool(
    cfg: &Arc<AgentConfig>,
    effective_working_dir: &Path,
    web_ctx: Option<&WebToolRuntime>,
    name: &str,
    args: &str,
    tool_jobs: Option<&Arc<crate::cm_internal::tool_jobs::ToolJobRegistry>>,
) -> Option<(String, Option<serde_json::Value>)> {
    let cfg_ref: &AgentConfig = cfg.as_ref();
    match background_async_gate(
        cfg_ref
            .tool_registry_policy
            .tool_registry_background_jobs_enabled,
        &cfg_ref
            .tool_registry_policy
            .tool_registry_background_job_async_tools,
        name,
        args,
    ) {
        BackgroundAsyncGate::Skip => return None,
        BackgroundAsyncGate::Deny(msg) => return Some((msg, None)),
        BackgroundAsyncGate::Launch => {}
    }
    let Some(registry) = tool_jobs else {
        return Some((BACKGROUND_ASYNC_NO_REGISTRY_MSG.to_string(), None));
    };
    if cfg_ref.sync_tool_sandbox.sync_default_tool_sandbox_mode == SyncDefaultToolSandboxMode::Docker {
        return Some((BACKGROUND_ASYNC_DOCKER_MSG.to_string(), None));
    }
    let spawn = match assemble_background_job_spawn(cfg_ref, name, args, effective_working_dir) {
        Ok(s) => s,
        Err(e) => return Some((e, None)),
    };
    Some(launch_background_job(
        registry,
        web_ctx,
        name,
        args,
        effective_working_dir,
        spawn,
    ))
}

/// 门闩判定（纯函数，便于单测）：仅当 `args` 显式请求 `async` 且工具名落在
/// 「装配表 ∪ `background_job_async_tools` 白名单」内才接管；否则一律 `Skip`。
fn background_async_gate(
    background_jobs_enabled: bool,
    allowlist: &HashSet<String>,
    name: &str,
    args: &str,
) -> BackgroundAsyncGate {
    if !args_request_background_async(args) {
        return BackgroundAsyncGate::Skip;
    }
    if name == "run_command" {
        // `run_command` 走既有 `execute_run_command_async` 路径。
        return BackgroundAsyncGate::Skip;
    }
    let supported = background_async_argv_assembler(name).is_some();
    let allowed = allowlist.contains(name);
    if !supported && !allowed {
        return BackgroundAsyncGate::Skip;
    }
    if !background_jobs_enabled {
        return BackgroundAsyncGate::Deny(BACKGROUND_ASYNC_DISABLED_MSG.to_string());
    }
    if !allowed {
        return BackgroundAsyncGate::Deny(format!(
            "错误：工具 `{name}` 未在 `[tool_registry] background_job_async_tools` 白名单中；请加入白名单或去掉 async 参数。"
        ));
    }
    if !supported {
        return BackgroundAsyncGate::Deny(background_async_unsupported_message(name));
    }
    BackgroundAsyncGate::Launch
}

/// `args` 中是否显式请求后台执行（`"async": true`）。
fn args_request_background_async(args: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(args)
        .ok()
        .and_then(|v| v.get("async").and_then(serde_json::Value::as_bool))
        .unwrap_or(false)
}

/// 按工具名装配后台任务 [`JobSpawn`](crate::cm_internal::tool_jobs::JobSpawn)。
///
/// 参数校验复用各工具前台同步路径的同一套逻辑（见装配表），并**原样沿用装配时设定的
/// 工作目录**（`Command::current_dir`；`pytest_run` 会在此 canonicalize），使后台与前台
/// 的 argv / cwd 完全一致。墙钟与前台 `registry_policy` 的 `BlockingSync` 取同一配置值。
fn assemble_background_job_spawn(
    cfg: &AgentConfig,
    name: &str,
    args_json: &str,
    workspace_root: &Path,
) -> Result<crate::cm_internal::tool_jobs::JobSpawn, String> {
    let Some(assemble) = background_async_argv_assembler(name) else {
        return Err(background_async_unsupported_message(name));
    };
    let cmd = assemble(args_json, workspace_root)?;
    Ok(crate::cm_internal::tool_jobs::JobSpawn {
        program: cmd.program,
        args: cmd.args,
        cwd: cmd.cwd.unwrap_or_else(|| workspace_root.to_path_buf()),
        extra_env: Vec::new(),
        wall: Duration::from_secs(cfg.command_exec.command_timeout_secs.max(1)),
        max_output_len: cfg.command_exec.command_max_output_len,
    })
}

/// 「工具不在装配表」的统一文案（门闩与装配共用，避免两处字面量漂移）。
fn background_async_unsupported_message(name: &str) -> String {
    format!("错误：工具 `{name}` 暂不支持后台执行（async）；请去掉 async 参数。")
}

/// 后台任务通用发起：登记 + 立即返回启动帧（契约 §1.2）。`run_command` 与各装配工具共用。
///
/// `tool_name` 须为**真实**工具名（终态补发摘要 `summarize_tool_call` 使用）。
fn launch_background_job(
    registry: &Arc<crate::cm_internal::tool_jobs::ToolJobRegistry>,
    web_ctx: Option<&WebToolRuntime>,
    tool_name: &str,
    args: &str,
    effective_working_dir: &Path,
    spawn: crate::cm_internal::tool_jobs::JobSpawn,
) -> (String, Option<serde_json::Value>) {
    let finished_sink = background_job_finished_sink(web_ctx, tool_name, args);
    let id = match crate::cm_internal::tool_jobs::enqueue_and_launch(
        Arc::clone(registry),
        effective_working_dir.to_path_buf(),
        None,
        spawn,
        args.to_string(),
        finished_sink,
    ) {
        Ok(id) => id,
        Err(crate::cm_internal::tool_jobs::RegisterError::QueueFull) => {
            return (
                "错误：后台任务队列已满，请稍后重试或去掉 async 参数。".to_string(),
                None,
            );
        }
        Err(crate::cm_internal::tool_jobs::RegisterError::AtCapacity) => {
            return (
                "错误：后台任务注册表已达条目上限，请稍后重试。".to_string(),
                None,
            );
        }
    };
    let poll_url = format!("/tools/jobs/{id}");
    let output = format!("已创建后台任务 {id}，轮询 GET {poll_url}（结果可通过轮询接口获取）。");
    let inject = Some(serde_json::json!({
        "tool_job": {
            "tool_job_id": id,
            "tool_job_poll_url": poll_url,
            "tool_job_status": "queued",
        }
    }));
    (output, inject)
}

/// 终态补发（契约 §5，尽力而为）：捕获发起时刻 SSE 发送端的**弱引用**。
///
/// 弱引用不阻止通道关闭，故回合结束后 SSE 通道（及其 HTTP 响应）不被本回调钉住；
/// 升级成功即「回合仍持有发送端 = 原连接仍存活」，此时 `try_send` 投递，否则静默丢弃。
/// 仅 Web SSE 路径有 `web_ctx`；运维 CLI 无同进程 SSE，`None` 即不补发。
fn background_job_finished_sink(
    web_ctx: Option<&WebToolRuntime>,
    tool_name: &str,
    args: &str,
) -> Option<crate::cm_internal::tool_jobs::JobFinishedSink> {
    let tx_weak = web_ctx?.out_tx.downgrade();
    let tool_name = tool_name.to_string();
    let args = args.to_string();
    let sink: crate::cm_internal::tool_jobs::JobFinishedSink = Arc::new(
        move |job_id: &str, outcome: &crate::cm_internal::tool_jobs::JobOutcome| {
            let body = crate::cm_sse_protocol::sse::ToolJobFinishedBody {
                tool_job_id: job_id.to_string(),
                status: outcome.status.as_str().to_string(),
                exit_code: outcome.exit_code,
                summary: crate::cm_tools::tools::summarize_tool_call(&tool_name, &args),
                error_code: outcome.error_code.clone(),
            };
            let line = crate::cm_sse_protocol::sse::encode_message(
                crate::cm_sse_protocol::sse::SsePayload::ToolJobFinished {
                    tool_job_finished: body,
                },
            );
            // 非阻塞：连接已关闭/背压满 → 静默丢弃（主通道是轮询）。
            if let Some(tx) = tx_weak.upgrade() {
                let _ = tx.try_send(line);
            }
        },
    );
    Some(sink)
}
