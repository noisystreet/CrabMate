//! `serve` 的隔离域路径解析、启动自检与实例锁前置检查。

use std::path::PathBuf;

use log::info;

use crate::config::SharedAgentConfig;

use super::lock::{ENV_DISABLE_INSTANCE_LOCK, ServeInstanceLocks, serve_acquire_instance_locks, serve_instance_id};

/// `serve` 启动时解析出的隔离域路径（用于启动自检日志与实例锁定位）。
#[derive(Debug, Clone)]
pub(super) struct ServeResolvedPaths {
    pub(super) workspace_root: PathBuf,
    pub(super) uploads_dir: PathBuf,
    pub(super) conversation_sqlite: String,
    pub(super) long_term_memory_enabled: bool,
    pub(super) long_term_memory_sqlite: String,
    pub(super) config_path: Option<String>,
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
    pub(super) instance_id: Option<String>,
    pub(super) paths: ServeResolvedPaths,
    pub(super) instance_locks: ServeInstanceLocks,
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
fn serve_log_resolved_paths(
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
    // `cm_cli/` 禁边不许引用 `crate::cm_internal`（`scripts/check-crate-deps.sh`）；
    // 根包 `user_data` 已 re-export 同一函数。
    let user_data = crate::user_data::user_data_root();
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

#[cfg(test)]
mod tests {
    use super::serve_ready_json;

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
