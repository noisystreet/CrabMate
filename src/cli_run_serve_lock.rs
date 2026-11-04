//! `serve` 实例标识与实例锁（从 [`super`] 拆出以降低单文件行数）。
//!
//! 语义与拆分前逐字一致：锁必须在打开/迁移会话库与长期记忆库**之前**获取，
//! `Drop` 时释放（进程退出亦由内核释放）；目标顺序固定，避免并发启动时交叉等待。

#![cfg(feature = "web")]

use std::io::Write as _;
use std::path::{Path, PathBuf};

use log::{info, warn};

use super::ServeResolvedPaths;

/// 可选实例标识：`[A-Za-z0-9._-]`，最长 64 字节。
const ENV_INSTANCE_ID: &str = "CM_INSTANCE_ID";
/// 显式跳过实例锁（多 `serve` 共享同一隔离域时的逃生开关）。
///
/// `pub(super)`：启动自检日志（[`super::serve_log_resolved_paths`]）要把它原样打印出来，
/// 让运维一眼看到是哪个变量关掉了锁。
pub(super) const ENV_DISABLE_INSTANCE_LOCK: &str = "CM_SERVE_DISABLE_INSTANCE_LOCK";
const INSTANCE_ID_MAX_LEN: usize = 64;

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
struct ServeInstanceLock {
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
///
/// 可见性是 `pub(in crate::cli_run)` 而非 `pub(super)`：它出现在
/// [`super::ServePrecheck`] 的**公开字段**类型里，若比该结构体更窄会触发
/// `private_interfaces`。
pub(in crate::cli_run) struct ServeInstanceLocks {
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
}
