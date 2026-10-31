//! `run_command` 流水线的 宿主执行实现与并行审批预取 阶段。
//!
//! 原为 `execute/execute_run_command_host.inc.rs`，通过 `include!` 与其余六片共享
//! `execute` 的单一作用域；现为真子模块，跨片调用必须经 `mod.rs` 的显式接口（#1196）。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::cm_config::{AgentConfig, SyncDefaultToolSandboxMode};
use crate::cm_internal::tool_approval::{self, InteractiveGateOutcome};
use crate::cm_internal::tools;
use crate::cm_types::{CommandApprovalDecision, ToolCall};

// 祖先层（`execute`）的项：子模块可见祖先的私有项，故直接 glob 取用。
use super::super::*;
// 同一条流水线的兄弟子模块：只能看到 `mod.rs` 里 `pub(in crate::cm_internal::tool_registry::execute) use` 出来的显式接口。
use super::*;

pub(in crate::cm_internal::tool_registry::execute) async fn execute_run_command_impl(
    invoke: RunCommandHostInvoke<'_>,
) -> (String, Option<serde_json::Value>) {
    let RunCommandHostInvoke {
        env,
        effective_working_dir,
        workspace_is_set,
        workspace_changed,
        web_ctx,
        args,
        tool_call_id,
        sse_out_tx,
        sse_control_mirror,
        cancel,
        tool_jobs,
    } = invoke;
    if !workspace_is_set {
        return (web_tool_err_workspace_not_set("执行命令"), None);
    }
    let cfg = env.cfg;
    let args_value: serde_json::Value = serde_json::from_str(args).unwrap_or_default();
    let async_flag = args_value
        .get("async")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let timeout_secs = args_value.get("timeout_secs").and_then(|x| x.as_u64());
    let docker_sandbox =
        cfg.sync_tool_sandbox.sync_default_tool_sandbox_mode == SyncDefaultToolSandboxMode::Docker;
    let capabilities = RunCommandCapabilities {
        mode: if async_flag {
            ExecMode::Async
        } else if docker_sandbox {
            ExecMode::Docker
        } else {
            ExecMode::Sync
        },
        background_jobs_enabled: cfg
            .tool_registry_policy
            .tool_registry_background_jobs_enabled,
        has_job_registry: tool_jobs.is_some(),
        docker_sandbox,
    };
    if async_flag
        && let Some(message) = capabilities.async_unavailable_reason()
    {
        return (message.to_string(), None);
    }
    let parsed = match parse_run_command_json(args, effective_working_dir) {
        Ok(p) => p,
        Err(e) => return (e.extended_user_message(), None),
    };
    let policy_ctx = PolicyContext {
        cfg,
        workspace: effective_working_dir,
        web_ctx,
        mode: capabilities.mode,
        sse_command: crate::cm_names::RUN_COMMAND,
        permission: env.permission,
    };
    // 命令能力快照由本层持有：`evaluate_policy` 就地扩展它，`PolicyDecision` 只借用（R3）。
    // 命名避开 `capabilities`（那是执行形态 `RunCommandCapabilities`）。
    let mut policy_caps = policy_ctx.command_caps();
    let decision = match evaluate_policy(&parsed, &policy_ctx, &mut policy_caps).await {
        Ok(d) => d,
        Err(e) => return e.into_message(),
    };
    execute_run_command_facade(RunCommandFacadeInvoke {
        env,
        effective_working_dir,
        workspace_is_set,
        workspace_changed,
        web_ctx,
        args,
        tool_call_id,
        sse_out_tx,
        sse_control_mirror,
        cancel,
        tool_jobs,
        timeout_secs,
        parsed,
        decision,
        capabilities,
    })
    .await
}

/// 并行只读批内 **`http_fetch`**：在 `spawn_blocking` 之前串行完成解析与白名单/审批，避免多请求竞态修改 `persistent_allowlist`。
/// 返回 `(name, args) -> 错误文案`；未出现的键表示已获准或本就匹配前缀。
pub async fn prefetch_http_fetch_parallel_approvals(
    tool_calls: &[ToolCall],
    cfg: &Arc<AgentConfig>,
    web_ctx: Option<&WebToolRuntime>,
    permission: Option<&crate::cm_agent::permission::PermissionCaps>,
) -> HashMap<(String, String), String> {
    let mut failures: HashMap<(String, String), String> = HashMap::new();
    let mut seen: HashSet<(String, String)> = HashSet::new();
    for tc in tool_calls {
        if tc.function.name != "http_fetch" {
            continue;
        }
        let key = (tc.function.name.clone(), tc.function.arguments.clone());
        if !seen.insert(key.clone()) {
            continue;
        }
        let args = tc.function.arguments.as_str();
        let (url, method, _) = match tools::http_fetch::parse_http_fetch_args(args) {
            Ok(x) => x,
            Err(e) => {
                failures.insert(key, format!("错误：{}", e));
                continue;
            }
        };
        let storage_key = tools::http_fetch::storage_key(&url);
        let approval_args = tools::http_fetch::approval_args_display(method, &url);
        match http_fetch_prefetch_approval(
            web_ctx,
            cfg.as_ref(),
            permission,
            &url,
            &storage_key,
            &approval_args,
        )
        .await
        {
            Ok(()) => {}
            Err(msg) => {
                failures.insert(key, msg);
            }
        }
    }
    failures
}
