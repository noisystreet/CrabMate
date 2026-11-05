//! `serve` 子命令：会话后端与长期记忆运行时组装（从 [`crate::cli_run`] 拆出以降低圈复杂度）。

#![cfg(feature = "web")]


#[path = "cli_run_serve_lock.rs"]
mod cli_run_serve_lock;
use std::path::PathBuf;
use std::sync::Arc;

use log::info;
use self::cli_run_serve_lock::{
    ENV_DISABLE_INSTANCE_LOCK, ServeInstanceLocks, serve_acquire_instance_locks, serve_instance_id,
};

use crate::config::SharedAgentConfig;
use crate::memory::long_term_memory::LongTermMemoryRuntime;
use crate::web;

pub(super) fn conversation_backing_from_sqlite_path(
    conv_sqlite: &str,
) -> Result<crate::cm_session_store::ConversationBacking, std::io::Error> {
    if conv_sqlite.trim().is_empty() {
        return Ok(crate::cm_session_store::ConversationBacking::memory_default());
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
    Ok(crate::cm_session_store::ConversationBacking::Sqlite(conn))
}

pub(super) fn serve_long_term_memory_runtime(
    ltm_enabled: bool,
    conversation_backing: &crate::cm_session_store::ConversationBacking,
    ltm_store_path: &str,
) -> Option<Arc<LongTermMemoryRuntime>> {
    if !ltm_enabled {
        return None;
    }
    match conversation_backing {
        crate::cm_session_store::ConversationBacking::Sqlite(conn) => {
            Some(LongTermMemoryRuntime::new_shared_sqlite(Arc::clone(conn)))
        }
        crate::cm_session_store::ConversationBacking::Memory(_) => {
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

/// 初始工作区 override：`--workspace-unset` ⇒ `None`（待绑定），否则 `Some(workspace_cli.unwrap_or_default())`。
///
/// `Some("")` 的语义是「用配置的 `run_command_working_dir`」——与不带 `--workspace` 的旧行为
/// 逐字等价。只有 ctl 显式声明「等 client 绑定」时才产生 `None`（Issue #1183 §2 ④）：
/// 若把「未传」也当成 `None`，CLI 直启 `serve` 会因工作区未绑定而无法使用工具。
pub(super) fn initial_workspace_override(
    workspace_cli: &Option<String>,
    workspace_unset: bool,
) -> Option<String> {
    if workspace_unset {
        None
    } else {
        Some(workspace_cli.clone().unwrap_or_default())
    }
}

/// `serve` 启动时解析出的隔离域路径（用于启动自检日志与实例锁定位）。
#[derive(Debug, Clone)]
pub(super) struct ServeResolvedPaths {
    pub workspace_root: PathBuf,
    pub uploads_dir: PathBuf,
    pub conversation_sqlite: String,
    pub long_term_memory_enabled: bool,
    pub long_term_memory_sqlite: String,
    pub config_path: Option<String>,
}

/// 从配置与 CLI 参数解析 `serve` 的隔离域路径（只读，不触碰磁盘）。
pub(super) async fn resolve_serve_paths(
    cfg_holder: &SharedAgentConfig,
    workspace_cli: Option<&str>,
    config_path: Option<&str>,
) -> ServeResolvedPaths {
    let g = cfg_holder.read().await;
    let workspace_root = workspace_cli
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| PathBuf::from(g.command_exec.run_command_working_dir.clone()));
    let conversation_sqlite = g
        .conversation_persistence
        .conversation_store_sqlite_path
        .clone();
    let uploads_dir =
        crate::web::chat_uploads_dir_beside_session_store(&conversation_sqlite, &workspace_root);
    ServeResolvedPaths {
        workspace_root,
        uploads_dir,
        conversation_sqlite,
        long_term_memory_enabled: g.long_term_memory.long_term_memory_enabled,
        long_term_memory_sqlite: g.long_term_memory.long_term_memory_store_sqlite_path.clone(),
        config_path: config_path.map(str::to_string),
    }
}

/// 启动自检结果；`instance_locks` 须由调用方保留到进程退出。
pub(super) struct ServePrecheck {
    pub instance_id: Option<String>,
    pub paths: ServeResolvedPaths,
    pub instance_locks: ServeInstanceLocks,
}

/// 解析实例标识/路径，并在打开任何存储之前加实例锁。
pub(super) async fn serve_precheck(
    cfg_holder: &SharedAgentConfig,
    workspace_cli: Option<&str>,
    config_path: Option<&str>,
) -> Result<ServePrecheck, std::io::Error> {
    let instance_id = serve_instance_id()?;
    let paths = resolve_serve_paths(cfg_holder, workspace_cli, config_path).await;
    // 锁必须在打开/迁移会话库与长期记忆库之前获取。
    let instance_locks = serve_acquire_instance_locks(&paths)?;
    serve_log_resolved_paths(&paths, instance_id.as_deref(), &instance_locks);
    Ok(ServePrecheck {
        instance_id,
        paths,
        instance_locks,
    })
}

fn long_term_memory_display(paths: &ServeResolvedPaths) -> String {
    if !paths.long_term_memory_enabled {
        return "未启用".to_string();
    }
    let conv = paths.conversation_sqlite.trim();
    if !conv.is_empty() {
        return format!("共用会话库 {conv}");
    }
    let ltm = paths.long_term_memory_sqlite.trim();
    if ltm.is_empty() {
        "内存模式（不持久化）".to_string()
    } else {
        ltm.to_string()
    }
}

/// 启动路径自检：打印实际生效的工作区 / 上传目录 / 会话库 / 用户目录，并对可创建性给出警告。
pub(super) fn serve_log_resolved_paths(
    paths: &ServeResolvedPaths,
    instance_id: Option<&str>,
    locks: &ServeInstanceLocks,
) {
    let conversation = {
        let p = paths.conversation_sqlite.trim();
        if p.is_empty() {
            "内存模式（重启即丢）".to_string()
        } else {
            p.to_string()
        }
    };
    let ltm = long_term_memory_display(paths);
    let user_data = crate::cm_internal::user_data::user_data_root();
    let cache = crate::config::user_cache_dir();
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|e| format!("<不可用: {e}>"));
    let config = paths
        .config_path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("默认发现");
    let lock_display = if locks.is_disabled() {
        format!("已禁用（{ENV_DISABLE_INSTANCE_LOCK}）")
    } else {
        locks
            .targets()
            .iter()
            .map(|t| format!("{} {}", t.label, t.path.display()))
            .collect::<Vec<_>>()
            .join("；")
    };

    println!("serve 实例自检");
    println!(
        "  实例标识: {}",
        instance_id.unwrap_or("未设置（CM_INSTANCE_ID）")
    );
    println!("  配置文件: {config}");
    println!("  进程目录: {cwd}");
    println!("  工作区: {}", paths.workspace_root.display());
    println!("  上传目录: {}", paths.uploads_dir.display());
    println!("  会话库: {conversation}");
    println!("  长期记忆: {ltm}");
    println!("  用户数据: {}", user_data.display());
    println!("  缓存: {}", cache.display());
    println!("  实例锁: {lock_display}");

    if !paths.workspace_root.is_dir() {
        eprintln!(
            "  警告: 工作区目录不存在或不是目录：{}",
            paths.workspace_root.display()
        );
    }
    if !paths.uploads_dir.is_dir() {
        eprintln!(
            "  警告: 上传目录尚不存在（启动时会尝试创建）：{}",
            paths.uploads_dir.display()
        );
    }
    if !user_data.is_dir() {
        eprintln!(
            "  警告: 用户数据目录尚不存在（首次写入时创建）：{}",
            user_data.display()
        );
    }
    info!(
        target: "crabmate",
        "serve 启动自检 instance_id={} config={} cwd={} workspace={} uploads={} conversation_db={} long_term_memory={} user_data={} cache={} instance_locks={}",
        instance_id.unwrap_or(""),
        config,
        cwd,
        paths.workspace_root.display(),
        paths.uploads_dir.display(),
        conversation,
        ltm,
        user_data.display(),
        cache.display(),
        lock_display
    );
}

/// `--web-ready-json` / `--desktop-ready-json` 输出的单行 JSON。
///
/// `config` 是**实际加载**的配置文件路径（未加载任何文件时为 `null`）：`crabmate ctl`
/// 依赖它向用户展示 `config_in_use`，避免「声明了 A 却静默加载了 B」这类排查困难。
/// 该字段属 CLI stdout 的纯增量，不改变任何 HTTP 路由契约。
pub(super) fn serve_ready_json(
    actual_addr: std::net::SocketAddr,
    web_api_bearer_layer_enabled: bool,
    instance_id: Option<&str>,
    config: Option<&str>,
) -> serde_json::Value {
    serde_json::json!({
        "event": "web_ready",
        "host": actual_addr.ip().to_string(),
        "port": actual_addr.port(),
        "url": format!("http://{}/", actual_addr),
        "auth_enabled": web_api_bearer_layer_enabled,
        "instance_id": instance_id,
        "config": config,
    })
}

/// Web 鉴权门槛：非 loopback 且未配 `web_api_bearer_token`（也没显式放开）时直接拒启。
///
/// 从 `cli_run.rs` 搬到本文件（#1274 审查跟进）：唯一调用方是 [`serve_validate_bind_gates`]，
/// 与 `serve_bind_auth_flags` 同族，放在这里才不用跨国取父模块的私有项。
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
/// 合并成一个入口也避免 `cli_run_serve_branch::run_serve_branch` 的 `?` 计数继续增长（CCN ≤ 10 门禁）。
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ready_json_includes_instance_id_and_config() {
        let addr: std::net::SocketAddr = "127.0.0.1:18080".parse().unwrap();
        let v = serve_ready_json(addr, true, Some("host-a"), Some("/etc/crabmate/a.toml"));
        assert_eq!(v["event"], "web_ready");
        assert_eq!(v["port"], 18080);
        assert_eq!(v["auth_enabled"], true);
        assert_eq!(v["instance_id"], "host-a");
        assert_eq!(v["config"], "/etc/crabmate/a.toml");
        let none = serve_ready_json(addr, false, None, None);
        assert!(none["instance_id"].is_null());
        assert!(none["config"].is_null(), "未加载配置时应为 null");
    }
}

