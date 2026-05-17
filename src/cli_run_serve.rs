//! `serve` 子命令：会话后端与长期记忆运行时组装（从 [`crate::cli_run`] 拆出以降低圈复杂度）。

use std::sync::Arc;

use log::info;

use crate::config::SharedAgentConfig;
use crate::memory::long_term_memory::LongTermMemoryRuntime;
use crate::web;

pub(super) fn conversation_backing_from_sqlite_path(
    conv_sqlite: &str,
) -> Result<web::ConversationBacking, std::io::Error> {
    if conv_sqlite.trim().is_empty() {
        return Ok(web::ConversationBacking::memory_default());
    }
    let p = std::path::Path::new(conv_sqlite.trim());
    let conn = web::open_conversation_sqlite(p).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("无法初始化会话 SQLite {}: {}", p.display(), e),
        )
    })?;
    info!(
        target: "crabmate",
        "Web 会话持久化已启用 path={}",
        p.display()
    );
    Ok(web::ConversationBacking::Sqlite(conn))
}

pub(super) fn serve_long_term_memory_runtime(
    ltm_enabled: bool,
    conversation_backing: &web::ConversationBacking,
    ltm_store_path: &str,
) -> Option<Arc<LongTermMemoryRuntime>> {
    if !ltm_enabled {
        return None;
    }
    match conversation_backing {
        web::ConversationBacking::Sqlite(conn) => {
            Some(LongTermMemoryRuntime::new_shared_sqlite(Arc::clone(conn)))
        }
        web::ConversationBacking::Memory(_) => {
            let p = ltm_store_path.trim();
            if p.is_empty() {
                info!(
                    target: "crabmate",
                    "长期记忆已启用：Web 会话为内存模式且未配置 long_term_memory_store_sqlite_path，跳过持久化记忆"
                );
                None
            } else {
                match LongTermMemoryRuntime::open(std::path::Path::new(p)) {
                    Ok(r) => Some(r),
                    Err(e) => {
                        log::warn!(
                            target: "crabmate",
                            "长期记忆库打开失败 path={} error={}",
                            p,
                            e
                        );
                        None
                    }
                }
            }
        }
    }
}

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
            "已启用 web_api_require_bearer（或 CM_WEB_API_REQUIRE_BEARER），但未配置非空的 web_api_bearer_token / CM_WEB_API_BEARER_TOKEN；请设置共享密钥后再启动 serve，或在配置中关闭 web_api_require_bearer。",
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

/// `serve` 启动前：与 `GET /health` 同源的可选依赖与工具链检查，并写启动日志。
pub(super) async fn serve_log_startup_health(
    cfg_holder: &SharedAgentConfig,
    workspace_cli: &Option<String>,
    api_key: &str,
) {
    let (work_dir, auth_mode) = {
        let g = cfg_holder.read().await;
        let wd = workspace_cli
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(g.command_exec.run_command_working_dir.clone())
            });
        (wd, g.llm.llm_http_auth_mode)
    };
    let report = crate::health::build_health_report(&work_dir, api_key, auth_mode, true).await;
    crate::health::log_startup_dep_compat_summary(&report);
}
