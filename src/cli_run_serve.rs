//! `serve` 子命令：会话后端与长期记忆运行时组装（从 [`crate::cli_run`] 拆出以降低圈复杂度）。

#![cfg(feature = "web")]

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use log::{info, warn};

use crate::config::SharedAgentConfig;
use crate::memory::long_term_memory::LongTermMemoryRuntime;
use crate::web;

/// 可选实例标识：`[A-Za-z0-9._-]`，最长 64 字节。
const ENV_INSTANCE_ID: &str = "CM_INSTANCE_ID";
/// 显式跳过实例锁（多 `serve` 共享同一隔离域时的逃生开关）。
const ENV_DISABLE_INSTANCE_LOCK: &str = "CM_SERVE_DISABLE_INSTANCE_LOCK";
const INSTANCE_ID_MAX_LEN: usize = 64;

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

/// 解析并校验实例标识；`None` 表示未设置/空串。
fn parse_instance_id(raw: Option<&str>) -> Result<Option<String>, std::io::Error> {
    let Some(raw) = raw else { return Ok(None) };
    let id = raw.trim();
    if id.is_empty() {
        return Ok(None);
    }
    if id.len() > INSTANCE_ID_MAX_LEN {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "{ENV_INSTANCE_ID} 过长（{} 字节，上限 {INSTANCE_ID_MAX_LEN}）",
                id.len()
            ),
        ));
    }
    if !id
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{ENV_INSTANCE_ID} 仅允许 [A-Za-z0-9._-]：{id:?}"),
        ));
    }
    Ok(Some(id.to_string()))
}

/// `serve` 实例标识（`CM_INSTANCE_ID`，可选）；非法值拒绝启动。
pub(super) fn serve_instance_id() -> Result<Option<String>, std::io::Error> {
    let Some(raw) = std::env::var_os(ENV_INSTANCE_ID) else {
        return Ok(None);
    };
    let raw = raw.to_str().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{ENV_INSTANCE_ID} 不是有效 UTF-8"),
        )
    })?;
    parse_instance_id(Some(raw))
}

fn env_flag_enabled(key: &str) -> bool {
    std::env::var(key).ok().is_some_and(|v| {
        matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
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

/// 一个实例锁目标；`label` 仅用于日志与错误提示。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ServeLockTarget {
    pub label: &'static str,
    pub path: PathBuf,
}

/// 实例锁目标推导（顺序固定，避免并发启动时交叉等待）。
///
/// - 会话 SQLite 模式：长期记忆复用会话连接，仅锁 `<conversation_store_sqlite_path>.serve.lock`；
/// - 内存会话模式：锁 `<workspace>/.crabmate/serve.lock`；若长期记忆独立持久化，
///   再锁 `<long_term_memory_store_sqlite_path>.serve.lock`。
#[must_use]
pub(super) fn serve_instance_lock_targets(paths: &ServeResolvedPaths) -> Vec<ServeLockTarget> {
    let conv = paths.conversation_sqlite.trim();
    if !conv.is_empty() {
        return vec![ServeLockTarget {
            label: "会话库",
            path: PathBuf::from(format!("{conv}.serve.lock")),
        }];
    }
    let mut targets = vec![ServeLockTarget {
        label: "工作区",
        path: paths.workspace_root.join(".crabmate").join("serve.lock"),
    }];
    let ltm = paths.long_term_memory_sqlite.trim();
    if paths.long_term_memory_enabled && !ltm.is_empty() {
        targets.push(ServeLockTarget {
            label: "长期记忆库",
            path: PathBuf::from(format!("{ltm}.serve.lock")),
        });
    }
    targets
}

/// 持有的实例锁；`Drop` 时自动释放（进程退出亦由内核释放）。
pub(super) struct ServeInstanceLock {
    _lock: nix::fcntl::Flock<std::fs::File>,
    path: PathBuf,
}

impl ServeInstanceLock {
    fn path(&self) -> &Path {
        &self.path
    }
}

fn serve_acquire_instance_lock_at(
    label: &str,
    path: &Path,
) -> Result<ServeInstanceLock, std::io::Error> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| {
            std::io::Error::new(
                e.kind(),
                format!("无法创建实例锁目录 {}: {e}", parent.display()),
            )
        })?;
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .map_err(|e| {
            std::io::Error::new(
                e.kind(),
                format!("无法打开 serve 实例锁（{label}）{}: {e}", path.display()),
            )
        })?;
    match nix::fcntl::Flock::lock(file, nix::fcntl::FlockArg::LockExclusiveNonblock) {
        Ok(mut lock) => {
            // 仅供排障：记录持有者 PID；写失败不阻断启动。
            let _ = lock.set_len(0);
            let _ = writeln!(lock, "pid={}", std::process::id());
            let _ = lock.flush();
            Ok(ServeInstanceLock {
                _lock: lock,
                path: path.to_path_buf(),
            })
        }
        Err((_file, errno)) => {
            let holder = std::fs::read_to_string(path)
                .ok()
                .map(|s| s.trim().replace('\n', " "))
                .filter(|s| !s.is_empty())
                .map(|s| format!("（{s}）"))
                .unwrap_or_default();
            Err(std::io::Error::new(
                std::io::ErrorKind::AddrInUse,
                format!(
                    "无法获取 serve 实例锁（{label}）：{} 已被另一个 serve 进程占用：{errno}{holder}。如确需多个 serve 共享同一隔离域，请设置 {ENV_DISABLE_INSTANCE_LOCK}=1。",
                    path.display()
                ),
            ))
        }
    }
}

/// 已获取的实例锁集合；`Drop` 时释放全部锁。
pub(super) struct ServeInstanceLocks {
    disabled: bool,
    targets: Vec<ServeLockTarget>,
    _locks: Vec<ServeInstanceLock>,
}

impl ServeInstanceLocks {
    pub(super) fn is_disabled(&self) -> bool {
        self.disabled
    }

    pub(super) fn targets(&self) -> &[ServeLockTarget] {
        &self.targets
    }
}

/// 按固定顺序非阻塞获取全部实例锁；`CM_SERVE_DISABLE_INSTANCE_LOCK` 为真时跳过。
pub(super) fn serve_acquire_instance_locks(
    paths: &ServeResolvedPaths,
) -> Result<ServeInstanceLocks, std::io::Error> {
    if env_flag_enabled(ENV_DISABLE_INSTANCE_LOCK) {
        warn!(
            target: "crabmate",
            "serve 实例锁已跳过（{ENV_DISABLE_INSTANCE_LOCK}）；多进程共享同一隔离域可能导致会话库/长期记忆竞争"
        );
        return Ok(ServeInstanceLocks {
            disabled: true,
            targets: Vec::new(),
            _locks: Vec::new(),
        });
    }
    let targets = serve_instance_lock_targets(paths);
    let mut locks = Vec::with_capacity(targets.len());
    for target in &targets {
        let lock = serve_acquire_instance_lock_at(target.label, &target.path)?;
        info!(
            target: "crabmate",
            "serve 实例锁已获取 kind={} path={}",
            target.label,
            lock.path().display()
        );
        locks.push(lock);
    }
    Ok(ServeInstanceLocks {
        disabled: false,
        targets,
        _locks: locks,
    })
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

/// `serve` 启动期的「绑定地址 → 安全门槛」校验，一次做完两件事：
///
/// 1. Web 鉴权（非 loopback 无 token 直接拒启）；
/// 2. 权限等级上限按部署形态**冻结**（设计 `docs/design/permission_levels.md` §5.1 / §9 MB-3）：
///    非 loopback 下显式要 `unrestricted` 而缺 `allow_unrestricted_permission_for_non_loopback`
///    时直接启动失败，而不是悄悄降档——否则远端部署会以为拿到了 L4。
///
/// 合并成一个入口也避免 `cli_run::run_serve_branch` 的 `?` 计数继续增长（CCN ≤ 10 门禁）。
pub(super) async fn serve_validate_bind_gates(
    cfg_holder: &SharedAgentConfig,
    bind_ip: std::net::IpAddr,
) -> Result<bool, Box<dyn std::error::Error>> {
    let auth_enabled = super::validate_bind_auth(cfg_holder, bind_ip).await?;
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

    fn paths_with(conv_sqlite: &str, workspace: &Path) -> ServeResolvedPaths {
        ServeResolvedPaths {
            workspace_root: workspace.to_path_buf(),
            uploads_dir: workspace.join("uploads"),
            conversation_sqlite: conv_sqlite.to_string(),
            long_term_memory_enabled: false,
            long_term_memory_sqlite: String::new(),
            config_path: None,
        }
    }

    #[test]
    fn instance_id_unset_or_blank_is_none() {
        assert!(parse_instance_id(None).unwrap().is_none());
        assert!(parse_instance_id(Some("")).unwrap().is_none());
        assert!(parse_instance_id(Some("   ")).unwrap().is_none());
    }

    #[test]
    fn instance_id_accepts_allowed_chars() {
        assert_eq!(
            parse_instance_id(Some(" host-a_1.2 ")).unwrap().as_deref(),
            Some("host-a_1.2")
        );
    }

    #[test]
    fn instance_id_rejects_bad_chars_and_oversize() {
        assert!(parse_instance_id(Some("a b")).is_err());
        assert!(parse_instance_id(Some("a/b")).is_err());
        assert!(parse_instance_id(Some(&"x".repeat(INSTANCE_ID_MAX_LEN + 1))).is_err());
    }

    #[test]
    fn lock_targets_follow_conversation_db_or_workspace() {
        let ws = Path::new("/tmp/ws");
        let targets =
            serve_instance_lock_targets(&paths_with("/tmp/ws/.crabmate/conversations.db", ws));
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].label, "会话库");
        assert_eq!(
            targets[0].path,
            PathBuf::from("/tmp/ws/.crabmate/conversations.db.serve.lock")
        );

        let targets = serve_instance_lock_targets(&paths_with("  ", ws));
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].label, "工作区");
        assert_eq!(targets[0].path, PathBuf::from("/tmp/ws/.crabmate/serve.lock"));
    }

    #[test]
    fn lock_targets_add_ltm_only_for_persistent_memory_mode() {
        let ws = Path::new("/tmp/ws");
        let mut paths = paths_with("", ws);
        paths.long_term_memory_enabled = true;
        paths.long_term_memory_sqlite = "/tmp/ltm/memory.db".into();
        let targets = serve_instance_lock_targets(&paths);
        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].label, "工作区");
        assert_eq!(targets[1].label, "长期记忆库");
        assert_eq!(
            targets[1].path,
            PathBuf::from("/tmp/ltm/memory.db.serve.lock")
        );

        paths.long_term_memory_sqlite.clear();
        assert_eq!(
            serve_instance_lock_targets(&paths).len(),
            1,
            "LTM 未持久化时不应附加锁"
        );

        let mut with_conv = paths_with("/tmp/ws/.crabmate/conversations.db", ws);
        with_conv.long_term_memory_enabled = true;
        with_conv.long_term_memory_sqlite = "/tmp/ltm/memory.db".into();
        assert_eq!(
            serve_instance_lock_targets(&with_conv).len(),
            1,
            "会话 SQLite 模式下 LTM 复用同一连接，仅需会话库锁"
        );
    }

    #[test]
    fn instance_lock_conflicts_then_releases() {
        let tmp = tempfile::tempdir().unwrap();
        let lock_path = tmp.path().join("serve.lock");
        let first =
            serve_acquire_instance_lock_at("工作区", &lock_path).expect("首次加锁应成功");
        assert_eq!(first.path(), lock_path.as_path());
        let err = serve_acquire_instance_lock_at("工作区", &lock_path)
            .err()
            .expect("同一锁文件第二次非阻塞加锁应失败");
        assert!(err.to_string().contains("工作区"), "错误应带锁目标标签");
        drop(first);
        assert!(
            serve_acquire_instance_lock_at("工作区", &lock_path).is_ok(),
            "释放后应可重新加锁"
        );
    }

    #[test]
    fn shared_ltm_conflicts_even_with_different_workspaces() {
        let tmp = tempfile::tempdir().unwrap();
        let ws_a = serve_acquire_instance_lock_at(
            "工作区",
            &tmp.path().join("a/.crabmate/serve.lock"),
        )
        .unwrap();
        let ltm_a = serve_acquire_instance_lock_at(
            "长期记忆库",
            &tmp.path().join("ltm/memory.db.serve.lock"),
        )
        .unwrap();
        let ws_b = serve_acquire_instance_lock_at(
            "工作区",
            &tmp.path().join("b/.crabmate/serve.lock"),
        )
        .expect("不同工作区可共存");
        let err = serve_acquire_instance_lock_at(
            "长期记忆库",
            &tmp.path().join("ltm/memory.db.serve.lock"),
        )
        .err()
        .expect("共享同一长期记忆库应冲突");
        assert!(err.to_string().contains("长期记忆库"));
        drop((ws_a, ltm_a, ws_b));
        assert!(
            serve_acquire_instance_lock_at(
                "长期记忆库",
                &tmp.path().join("ltm/memory.db.serve.lock")
            )
            .is_ok()
        );
    }

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

