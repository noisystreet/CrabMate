//! `http_fetch` / `http_request` 共用的出站网络闸门：前缀白名单、Web 审批与 Docker 分流。
//!
//! 原为 `execute/http_tool_exec.inc.rs`，现为真子模块（#1198）。本文件同时持有网络轴的
//! 权限矩阵测试——`docs/design/permission_levels.md` 的「网络」一行直接锚定此处。

use std::path::Path;
use std::time::Duration;

use log::error;

use crate::cm_internal::tool_approval;
use crate::cm_internal::tool_registry::runtime::WebToolRuntime;
use crate::cm_internal::tools;

use super::super::run_command::dispatch_non_sync_tool_to_docker;
use super::super::ToolExecEnv;

/// URL 是否已由**本等级作用域内**的配置前缀或 persistent allowlist 放行。
///
/// 前缀只认 `caps.net_scope` 里的那份列表（MB-8「二级执行面不得回读 cfg」）；
/// `permission = None` 表示该路径未接线等级，回落 `cfg`，与引入等级前逐字一致。
///
/// persistent allowlist 的命中键带**等级前缀**（设计 §6.3）：L3 批过的 URL 在 L2 会话
/// 不得继续命中，否则「L2 = 人工例外通道关闭」名不副实。
async fn http_tool_url_allowed(
    cfg: &crate::cm_config::AgentConfig,
    web_ctx: Option<&WebToolRuntime>,
    permission: Option<&crate::cm_agent::permission::PermissionCaps>,
    url: &reqwest::Url,
    storage_key: &str,
) -> bool {
    use crate::cm_agent::permission::NetScope;
    let prefixes: &[String] = match permission.map(|c| &c.net_scope) {
        Some(NetScope::Prefixes(list)) => list.as_slice(),
        // `Any` / `None` 已由 `http_net_gate` 先行定论。这两个调用点都有
        // `needs_prefix_lookup` 守卫，所以 `Any` 分支当前不可达；保留它是为了让
        // 「误加调用点」时失败方向是**更严**（不放行）而不是放宽。
        Some(_) => &[],
        None => cfg.http_fetch.http_fetch_allowed_prefixes.as_slice(),
    };
    if tools::http_fetch::url_matches_allowed_prefixes(url, prefixes) {
        return true;
    }
    let key = crate::cm_types::scoped_allowlist_key(permission.map(|c| c.level), storage_key);
    match web_ctx {
        Some(w) => w.persistent_allowlist_shared.lock().await.contains(&key),
        None => false,
    }
}

/// 出站网络门的三态结论（S3：`net_scope` + `approval` 两轴）。
enum HttpNetGate {
    /// 直接放行：`net_scope = Any`（L4），或地址已命中前缀 / persistent allowlist。
    Allow,
    /// 需要人工审批：L3（`Prefixes` + `Gate`）。
    NeedApproval,
    /// 直接拒：L0（`None`）或 L2（`Prefixes` + `Deny`）。拒绝**不弹审批**——
    /// 无人值守时可解释、可复现，弹窗只会把「被拒」变成「挂着等超时」。
    Deny(String),
}

/// 权限等级拒绝出站访问时的正文。只报 origin，不回显完整 URL（query 里可能有敏感参数）。
fn http_tool_net_denied_message(
    caps: &crate::cm_agent::permission::PermissionCaps,
    url: &reqwest::Url,
) -> String {
    format!(
        "错误：当前权限等级（{}）下不允许访问出站地址 {}：{}。",
        caps.level.as_str(),
        url.origin().ascii_serialization(),
        caps.level.promise_zh()
    )
}

/// 由 `caps` + 前缀命中结果判定三态；`caps = None`（未接线）回落现状「命中即放行，否则审批」。
fn http_net_gate(
    caps: Option<&crate::cm_agent::permission::PermissionCaps>,
    url: &reqwest::Url,
    url_allowed: bool,
) -> HttpNetGate {
    use crate::cm_agent::permission::{ApprovalPolicy, NetScope};
    let Some(caps) = caps else {
        return if url_allowed {
            HttpNetGate::Allow
        } else {
            HttpNetGate::NeedApproval
        };
    };
    // 懒算：`Prefixes(_) if url_allowed`（L0–L3 常态放行）既不记录也不需要它。
    let audit_origin = || url.origin().ascii_serialization();
    match caps.net_scope {
        NetScope::None => {
            crate::cm_agent::permission_audit::record_denied(
                caps,
                crate::cm_agent::permission_audit::PermissionAxis::Net,
                None,
                Some(&audit_origin()),
            );
            HttpNetGate::Deny(http_tool_net_denied_message(caps, url))
        }
        NetScope::Any => {
            // L4 的唯一留痕点之一：出站不再有前缀与审批，全靠这条审计事件。
            crate::cm_agent::permission_audit::record_allowed(
                caps,
                crate::cm_agent::permission_audit::PermissionAxis::Net,
                None,
                Some(&audit_origin()),
            );
            HttpNetGate::Allow
        }
        NetScope::Prefixes(_) if url_allowed => HttpNetGate::Allow,
        NetScope::Prefixes(_) if caps.approval == ApprovalPolicy::Deny => {
            crate::cm_agent::permission_audit::record_denied(
                caps,
                crate::cm_agent::permission_audit::PermissionAxis::Net,
                None,
                Some(&audit_origin()),
            );
            HttpNetGate::Deny(http_tool_net_denied_message(caps, url))
        }
        NetScope::Prefixes(_) => HttpNetGate::NeedApproval,
    }
}

/// 未放行时发起 Web 审批；`Ok(())` 表示可继续本地执行。
async fn http_tool_gate_url_approval(
    web_ctx: Option<&WebToolRuntime>,
    permission: Option<&crate::cm_agent::permission::PermissionCaps>,
    spec: &crate::cm_internal::tool_approval::ApprovalRequestSpec,
    sse_log_label: &'static str,
) -> Result<(), (String, Option<serde_json::Value>)> {
    if web_ctx.is_none() {
        return Err((
            tool_approval::HTTP_TOOL_NO_APPROVAL_CHANNEL_ERR.to_string(),
            None,
        ));
    }
    match tool_approval::interactive_gate_web_runtime(
        web_ctx,
        permission.map(|c| c.level),
        spec,
        sse_log_label,
    )
    .await
    {
        Ok(outcome) => tool_approval::interactive_gate_outcome_to_tool_err(outcome)
            .map_err(|msg| (msg, None)),
        Err(e) => Err((e.user_message_zh().to_string(), None)),
    }
}

/// 前缀白名单 + 可选 Web 审批 + Docker 分流；通过后返回 `Ok(())` 以继续 `spawn_blocking`。
pub(in crate::cm_internal::tool_registry::execute) struct HttpToolPreflightParams<'a> {
    pub(in crate::cm_internal::tool_registry::execute) env: &'a ToolExecEnv<'a>,
    pub(in crate::cm_internal::tool_registry::execute) effective_working_dir: &'a Path,
    pub(in crate::cm_internal::tool_registry::execute) workspace_is_set: bool,
    pub(in crate::cm_internal::tool_registry::execute) web_ctx: Option<&'a WebToolRuntime>,
    pub(in crate::cm_internal::tool_registry::execute) args: &'a str,
    pub(in crate::cm_internal::tool_registry::execute) tool_name: &'static str,
    pub(in crate::cm_internal::tool_registry::execute) url: &'a reqwest::Url,
    pub(in crate::cm_internal::tool_registry::execute) storage_key: &'a str,
    pub(in crate::cm_internal::tool_registry::execute) sse_log_label: &'static str,
    pub(in crate::cm_internal::tool_registry::execute) approval_spec:
        &'a crate::cm_internal::tool_approval::ApprovalRequestSpec,
}

pub(in crate::cm_internal::tool_registry::execute) async fn http_tool_preflight(
    p: HttpToolPreflightParams<'_>,
) -> Result<(), (String, Option<serde_json::Value>)> {
    let cfg = p.env.cfg;
    // S3：先按 `net_scope` 判三态。`Any` / `None` 不必再查前缀与 persistent allowlist
    // （省一次加锁，也让 L0 的拒绝不依赖配置长什么样）。
    let needs_prefix_lookup = p.env.permission.is_none_or(|c| {
        matches!(c.net_scope, crate::cm_agent::permission::NetScope::Prefixes(_))
    });
    let url_allowed = needs_prefix_lookup
        && http_tool_url_allowed(
            cfg.as_ref(),
            p.web_ctx,
            p.env.permission,
            p.url,
            p.storage_key,
        )
        .await;
    match http_net_gate(p.env.permission, p.url, url_allowed) {
        HttpNetGate::Allow => {}
        HttpNetGate::Deny(msg) => return Err((msg, None)),
        HttpNetGate::NeedApproval => {
            http_tool_gate_url_approval(
                p.web_ctx,
                p.env.permission,
                p.approval_spec,
                p.sse_log_label,
            )
            .await?;
        }
    }
    if let Some(out) = dispatch_non_sync_tool_to_docker(
        p.env,
        p.effective_working_dir,
        p.workspace_is_set,
        p.tool_name,
        p.args,
        crate::cm_internal::tool_sandbox::write_runner_config_json(cfg.as_ref()),
    )
    .await
    {
        return Err(out);
    }
    Ok(())
}

pub(in crate::cm_internal::tool_registry::execute) async fn spawn_blocking_http_tool(
    tool_label: &'static str,
    name: &str,
    outer_wall_secs: u64,
    f: impl FnOnce() -> String + Send + 'static,
) -> String {
    let name_in = name.to_string();
    let handle = tokio::task::spawn_blocking(f);
    match tokio::time::timeout(Duration::from_secs(outer_wall_secs), handle).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            error!(
                target: "crabmate",
                "{tool_label} 任务异常 tool={} error={:?}",
                name_in,
                e
            );
            format!("{tool_label} 执行异常：{:?}", e)
        }
        Err(_) => format!("{tool_label} 超时（{} 秒）", outer_wall_secs),
    }
}

/// 并行预取：`http_fetch` URL 审批（与单工具路径共用 spec 构建器与网络门）。
pub(in crate::cm_internal::tool_registry::execute) async fn http_fetch_prefetch_approval(
    web_ctx: Option<&WebToolRuntime>,
    cfg: &crate::cm_config::AgentConfig,
    permission: Option<&crate::cm_agent::permission::PermissionCaps>,
    url: &reqwest::Url,
    storage_key: &str,
    approval_args: &str,
) -> Result<(), String> {
    let needs_prefix_lookup = permission.is_none_or(|c| {
        matches!(c.net_scope, crate::cm_agent::permission::NetScope::Prefixes(_))
    });
    let url_allowed = needs_prefix_lookup
        && http_tool_url_allowed(cfg, web_ctx, permission, url, storage_key).await;
    match http_net_gate(permission, url, url_allowed) {
        HttpNetGate::Allow => Ok(()),
        HttpNetGate::Deny(msg) => Err(msg),
        HttpNetGate::NeedApproval => {
            let spec = tool_approval::approval_spec_http_fetch(approval_args, storage_key);
            match http_tool_gate_url_approval(
                web_ctx,
                permission,
                &spec,
                "tool_registry::http_fetch approval parallel prefetch",
            )
            .await
            {
                Ok(()) => Ok(()),
                Err((msg, _)) => Err(msg),
            }
        }
    }
}

#[cfg(test)]
mod http_net_gate_tests {
    use super::*;
    use crate::cm_agent::permission::project_caps;
    use crate::cm_types::PermissionLevel;
    use std::sync::Arc;

    fn test_cfg() -> crate::cm_config::AgentConfig {
        let mut cfg = crate::cm_config::load_config(None).expect("embed default");
        cfg.http_fetch.http_fetch_allowed_prefixes = vec!["https://example.com".to_string()];
        cfg
    }

    /// S3 网络轴：L0 全禁、L2 越界硬拒（不弹窗）、L3 审批、L4 直接放行。
    #[test]
    fn net_gate_matrix_follows_permission_level() {
        let cfg = test_cfg();
        let url = reqwest::Url::parse("https://evil.example.org/x").expect("url");

        let l0 = project_caps(PermissionLevel::Chat, &cfg);
        assert!(matches!(http_net_gate(Some(&l0), &url, false), HttpNetGate::Deny(_)));

        let l2 = project_caps(PermissionLevel::Workspace, &cfg);
        assert!(matches!(http_net_gate(Some(&l2), &url, false), HttpNetGate::Deny(_)));

        let l3 = project_caps(PermissionLevel::Approved, &cfg);
        assert!(matches!(
            http_net_gate(Some(&l3), &url, false),
            HttpNetGate::NeedApproval
        ));
        // 命中配置前缀时 L3 直接放行（等价现状）。
        assert!(matches!(http_net_gate(Some(&l3), &url, true), HttpNetGate::Allow));

        let l4 = project_caps(PermissionLevel::Unrestricted, &cfg);
        assert!(matches!(http_net_gate(Some(&l4), &url, false), HttpNetGate::Allow));

        // 未接线：回落现状「命中即放行，否则审批」。
        assert!(matches!(http_net_gate(None, &url, false), HttpNetGate::NeedApproval));
        assert!(matches!(http_net_gate(None, &url, true), HttpNetGate::Allow));
    }

    /// 接线回归：L4 的 `NetScope::Any` 放行必须留痕；L3 的前缀命中不得产出记录。
    ///
    /// `net_gate_matrix_follows_permission_level` 只钉返回值——没有这条，删掉
    /// `record_allowed` 调用也不会红。
    #[test]
    fn net_gate_emits_audit_only_on_l4_direct_allow() {
        use crate::cm_types::permission_audit::test_support::captured_with;
        let cfg = test_cfg();
        let url = reqwest::Url::parse("https://evil.example.org/x").expect("url");

        let l4 = project_caps(PermissionLevel::Unrestricted, &cfg);
        let captured = captured_with(|| {
            let _ = http_net_gate(Some(&l4), &url, false);
        });
        assert!(captured.contains("permission_axis=net"), "{captured}");
        assert!(
            captured.contains("permission_decision=allowed"),
            "{captured}"
        );
        assert!(
            captured.contains("permission_level=unrestricted"),
            "{captured}"
        );
        // 只报 origin，不回显路径。
        assert!(!captured.contains("/x"), "{captured}");

        // L3 命中前缀：常态放行，不记。
        let l3 = project_caps(PermissionLevel::Approved, &cfg);
        let quiet = captured_with(|| {
            let _ = http_net_gate(Some(&l3), &url, true);
        });
        assert!(quiet.is_empty(), "{quiet}");

        // L2 硬拒：记 denied。
        let l2 = project_caps(PermissionLevel::Workspace, &cfg);
        let denied = captured_with(|| {
            let _ = http_net_gate(Some(&l2), &url, false);
        });
        assert!(denied.contains("permission_decision=denied"), "{denied}");
    }

    /// 拒因文案只报 origin，不回显完整 URL（query 里可能有敏感参数）。
    #[test]
    fn net_denied_message_redacts_path_and_query() {
        let cfg = test_cfg();
        let l0 = project_caps(PermissionLevel::Chat, &cfg);
        let url = reqwest::Url::parse("https://example.com/a?token=secret").expect("url");
        let msg = http_tool_net_denied_message(&l0, &url);
        assert!(!msg.contains("secret"), "{msg}");
        assert!(!msg.contains("/a"), "{msg}");
        assert!(msg.contains("https://example.com"), "{msg}");
    }

    /// 设计 §6.3 + MB-8：allowlist 命中按**等级**判定，且前缀只认 `caps.net_scope`
    /// （不再回读 `cfg` 的前缀列表）。
    #[tokio::test]
    async fn url_allowed_ignores_lower_level_allowlist_keys() {
        let cfg = test_cfg();
        let url = reqwest::Url::parse("https://evil.example.org/x").expect("url");
        let storage_key = "http_fetch:https://evil.example.org/x";

        // 只借 `persistent_allowlist_shared`：本用例完全不走审批，所以自己搭一个最小句柄，
        // 不复用 `super::tests::approval_fixture`（那会 preseed 一条没人消费的决策）。
        let (web, _out_rx, _decision_tx) = bare_web_runtime();
        web.persistent_allowlist_shared
            .lock()
            .await
            .insert(crate::cm_types::scoped_allowlist_key(
                Some(PermissionLevel::Approved),
                storage_key,
            ));

        let l3 = project_caps(PermissionLevel::Approved, &cfg);
        assert!(
            http_tool_url_allowed(&cfg, Some(&web), Some(&l3), &url, storage_key).await,
            "L3 会话应命中自己的键"
        );

        let l2 = project_caps(PermissionLevel::Workspace, &cfg);
        assert!(
            !http_tool_url_allowed(&cfg, Some(&web), Some(&l2), &url, storage_key).await,
            "L2 会话不得命中 L3 的键"
        );

        // MB-8：接线后前缀只来自 `caps.net_scope`，另一份 `cfg` 放宽的前缀不生效。
        let mut wider = test_cfg();
        wider.http_fetch.http_fetch_allowed_prefixes =
            vec!["https://evil.example.org".to_string()];
        assert!(
            !http_tool_url_allowed(&wider, Some(&web), Some(&l2), &url, storage_key).await,
            "已接线的等级不得回读 cfg 前缀"
        );
    }

    /// 最小 `WebToolRuntime`：两端一并交回调用方，保证 channel 不会提前关闭。
    fn bare_web_runtime() -> (
        crate::cm_tools::tool_runtime::WebToolRuntime,
        tokio::sync::mpsc::Receiver<String>,
        tokio::sync::mpsc::Sender<crate::cm_types::CommandApprovalDecision>,
    ) {
        use tokio::sync::Mutex as TokioMutex;
        let (out_tx, out_rx) = tokio::sync::mpsc::channel::<String>(2);
        let (decision_tx, decision_rx) =
            tokio::sync::mpsc::channel::<crate::cm_types::CommandApprovalDecision>(2);
        (
            crate::cm_tools::tool_runtime::WebToolRuntime {
                out_tx,
                approval_rx_shared: Arc::new(TokioMutex::new(decision_rx)),
                approval_request_guard: Arc::new(TokioMutex::new(())),
                persistent_allowlist_shared: Arc::new(TokioMutex::new(
                    std::collections::HashSet::new(),
                )),
                turn_job_id: None,
                cancel: None,
            },
            out_rx,
            decision_tx,
        )
    }
}
