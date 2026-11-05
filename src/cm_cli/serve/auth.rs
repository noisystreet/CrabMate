//! `serve` 的鉴权门槛与启动日志。

use log::info;

use crate::config::SharedAgentConfig;

pub(super) async fn serve_require_web_api_bearer_when_enabled(
    cfg_holder: &SharedAgentConfig,
) -> Result<(), std::io::Error> {
    let g = cfg_holder.read().await;
    if g.web_api.web_api_require_bearer
        && crate::config::ExposeSecret::expose_secret(&g.web_api.web_api_bearer_token)
            .trim()
            .is_empty()
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "已启用 web_api_require_bearer（或 CM_WEB_API_REQUIRE_BEARER），但未配置非空的 web_api_bearer_token / CM_WEB_API_BEARER_TOKEN（系统钥匙串 web_api_bearer 亦空）；请设置共享密钥（含 `crabmate web-bearer set`）后再启动 serve，或在配置中关闭 web_api_require_bearer。",
        ));
    }
    Ok(())
}

pub(super) async fn serve_web_api_bearer_layer_enabled(cfg_holder: &SharedAgentConfig) -> bool {
    let g = cfg_holder.read().await;
    !crate::config::ExposeSecret::expose_secret(&g.web_api.web_api_bearer_token)
        .trim()
        .is_empty()
}

pub(super) async fn serve_bind_auth_flags(cfg_holder: &SharedAgentConfig) -> (bool, bool) {
    let g = cfg_holder.read().await;
    (
        !crate::config::ExposeSecret::expose_secret(&g.web_api.web_api_bearer_token)
            .trim()
            .is_empty(),
        g.web_api.allow_insecure_no_auth_for_non_loopback,
    )
}

/// 启动日志：CORS 白名单非空时提示（改白名单须重启）。
pub(super) fn serve_log_cors_startup(cors_allowed_origins: &[String]) {
    if cors_allowed_origins.is_empty() {
        return;
    }
    println!(
        "  CORS: 已启用（{} 个 Origin）；改白名单须重启 serve",
        cors_allowed_origins.len()
    );
    info!(
        target: "crabmate",
        "CORS 已启用 allowed_origins_count={}",
        cors_allowed_origins.len()
    );
}

/// `serve` 启动前：与 `GET /health` 同源的可选依赖与工具链检查，并写启动日志。
pub(super) async fn serve_log_startup_health(
    cfg_holder: &SharedAgentConfig,
    workspace_cli: &Option<String>,
) {
    let work_dir = {
        let g = cfg_holder.read().await;
        workspace_cli
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(g.command_exec.run_command_working_dir.clone())
            })
    };
    let report = crate::health::build_health_report(&work_dir).await;
    crate::health::log_startup_dep_compat_summary(&report);
}

/// Web 鉴权门槛：非 loopback 且未配 `web_api_bearer_token`（也没显式放开）时直接拒启。
async fn validate_bind_auth(
    cfg_holder: &SharedAgentConfig,
    bind_ip: std::net::IpAddr,
) -> Result<bool, Box<dyn std::error::Error>> {
    let (auth_enabled, allow_insec) = serve_bind_auth_flags(cfg_holder).await;
    if !bind_ip.is_loopback() && !auth_enabled && !allow_insec {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "当前监听地址为非 loopback（如 0.0.0.0），但未配置 web_api_bearer_token；请设置 [agent].web_api_bearer_token / CM_WEB_API_BEARER_TOKEN、`crabmate web-bearer set`，或显式设置 allow_insecure_no_auth_for_non_loopback=true（不安全）",
        )
        .into());
    }
    Ok(auth_enabled)
}

/// `serve` 启动期的「绑定地址 → 安全门槛」校验，一次做完两件事：
///
/// 1. Web 鉴权（非 loopback 无 token 直接拒启）；
/// 2. 权限等级上限按部署形态**冻结**（设计 `docs/design/permission_levels.md` §5.1 / §9 MB-3）：
///    非 loopback 下显式要 `unrestricted` 而缺 `allow_unrestricted_permission_for_non_loopback`
///    时直接启动失败，而不是悄悄降档——否则远端部署会以为拿到了 L4。
///
/// 合并成一个入口也避免 `serve::run` 的 `?` 计数继续增长（CCN ≤ 10 门禁）。
pub(super) async fn serve_validate_bind_gates(
    cfg_holder: &SharedAgentConfig,
    bind_ip: std::net::IpAddr,
) -> Result<bool, Box<dyn std::error::Error>> {
    let auth_enabled = validate_bind_auth(cfg_holder, bind_ip).await?;
    let (ceiling, source) =
        crate::cm_config::freeze_permission_ceiling_for_bind(cfg_holder, bind_ip).await?;
    info!(
        target: "crabmate",
        "权限等级上限 ceiling={ceiling} source={source}"
    );
    Ok(auth_enabled)
}
