//! 查询与记录回收：`ls` / `status` / `logs` / `prune`。
//!
//! 从 [`super::ops`] 拆出（#1213）：那里已逼近 920 行的文件上限，而磁盘回收
//! （`gc` / 日志轮转 / 流式 tail）还要继续往里加东西。本模块只「读记录 + 观测
//! 进程 + 删记录」，不 spawn、不发信号 —— 因此不与启动编排共享任何可变状态。
//!
//! 「哪些条目可以清」的判据仍只有一处来源：[`probe::Observed::is_prunable`] 决定
//! 状态集合，[`guard::live_process_reason`] 决定活进程豁免。`gc` 复用同一组。

use super::guard;
use super::ops::Ctx;
use super::paths;
use super::probe::{self, Observed, Probe};
use super::record::{self, InstanceRecord};
use super::CtlError;

/// 列出全部实例（含派生观测态）。
pub fn ls(ctx: &Ctx) -> Result<Vec<(InstanceRecord, Probe)>, CtlError> {
    let mut out = Vec::new();
    for path in ctx.paths.all_record_paths()? {
        let rec = match record::read_optional(&path) {
            Ok(Some(r)) => r,
            // 坏记录不应让整个 ls 失败
            Ok(None) | Err(_) => continue,
        };
        let p = probe::observe(&rec);
        out.push((rec, p));
    }
    out.sort_by(|a, b| a.0.name.cmp(&b.0.name));
    Ok(out)
}

/// 单实例详情。
pub fn status(ctx: &Ctx, name: &str) -> Result<(InstanceRecord, Probe), CtlError> {
    paths::validate_name(name)?;
    let path = ctx.record_path(name)?;
    let rec = record::read_required(&path)?;
    let p = probe::observe(&rec);
    Ok((rec, p))
}

/// 读日志尾部。
pub fn logs(ctx: &Ctx, name: &str, tail: usize) -> Result<String, CtlError> {
    paths::validate_name(name)?;
    let path = ctx.paths.log_path(name);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(CtlError::general(
                "log_not_found",
                format!("实例 {name} 尚无日志文件：{}", path.display()),
            ));
        }
        Err(e) => {
            return Err(CtlError::general(
                "log_unreadable",
                format!("读取日志失败 {}：{e}", path.display()),
            ));
        }
    };
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(tail.max(1));
    Ok(lines[start..].join("\n"))
}

/// `prune` 的单条结果（既可能是「已清」，也可能是「有意跳过」）。
#[derive(Debug, Clone)]
pub struct PruneItem {
    pub name: String,
    pub observed: Observed,
    /// `Some(原因)` = 有意跳过：记录指向的进程还活着，删掉它等于放生孤儿。
    pub refused: Option<String>,
}

/// 清理 stale / 已退出 / 失败的记录。
///
/// `force` 之外，指向**仍存活且身份校验通过**的进程的记录一律跳过 —— 见
/// [`guard`]：`Observed::Stale` 同时盖住「PID 被复用」与「进程活着但端口不通」，
/// 只有后者不能清。跳过要**逐条报出来**，否则用户只会看到「没清掉」而不知为何。
///
/// **只删状态记录**：实例目录（会话库、上传）与日志不在这里回收，那是 `gc` 的
/// 职责（见 `docs/命令行与路由.md`）。
pub fn prune(ctx: &Ctx, dry_run: bool, force: bool) -> Result<Vec<PruneItem>, CtlError> {
    let mut out = Vec::new();
    for (rec, p) in ls(ctx)? {
        if !p.observed.is_prunable() {
            continue;
        }
        let refused = guard::live_process_reason(&rec, &p).filter(|_| !force);
        if refused.is_none() && !dry_run {
            let path = ctx.record_path(&rec.name)?;
            record::remove(&path)?;
        }
        out.push(PruneItem {
            name: rec.name,
            observed: p.observed,
            refused,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cm_ctl::paths::CtlPaths;
    use crate::cm_ctl::port_pool::PortPool;
    use crate::cm_ctl::record::InstanceState;
    use std::path::{Path, PathBuf};

    fn ctx_at(dir: &Path) -> Ctx {
        Ctx {
            paths: CtlPaths::for_test(dir),
            pool: PortPool::default_pool(),
        }
    }

    #[test]
    fn ls_on_empty_state_dir_returns_empty() {
        let tmp = tempdir("ls_empty");
        let ctx = ctx_at(&tmp);
        assert!(ls(&ctx).expect("应成功").is_empty());
    }

    #[test]
    fn status_reports_missing() {
        let tmp = tempdir("status_missing");
        let ctx = ctx_at(&tmp);
        let err = status(&ctx, "ghost").expect_err("应报不存在");
        assert_eq!(err.error_code, "instance_not_found");
    }

    #[test]
    fn prune_removes_only_terminal_states() {
        let tmp = tempdir("prune");
        let ctx = ctx_at(&tmp);
        ctx.paths.ensure_dirs().expect("建目录");

        let mut stopped = InstanceRecord::new("stopped-one");
        stopped.state = InstanceState::Stopped;
        let p1 = ctx.paths.record_path("stopped-one").expect("路径");
        record::write_atomic(&p1, &stopped).expect("写");

        // 该记录指向不存在的 pid ⇒ stopped
        let mut dead = InstanceRecord::new("dead-one");
        dead.state = InstanceState::Ready;
        dead.pid = Some(u32::MAX);
        let p2 = ctx.paths.record_path("dead-one").expect("路径");
        record::write_atomic(&p2, &dead).expect("写");

        let dry = prune(&ctx, true, false).expect("dry-run");
        assert_eq!(dry.len(), 2, "dry-run 应列出但不动文件：{dry:?}");
        assert!(p1.exists() && p2.exists());

        let real = prune(&ctx, false, false).expect("实删");
        assert_eq!(real.len(), 2);
        assert!(!p1.exists() && !p2.exists());
    }

    #[test]
    fn logs_reports_missing_file() {
        let tmp = tempdir("logs_missing");
        let ctx = ctx_at(&tmp);
        let err = logs(&ctx, "nope", 10).expect_err("应报无日志");
        assert_eq!(err.error_code, "log_not_found");
    }

    fn tempdir(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "cm_ctl_query_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&p).expect("建临时目录");
        p
    }
}
