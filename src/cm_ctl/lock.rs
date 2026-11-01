//! 状态目录级文件锁：把「读记录 → 判是否已存在 → spawn → 预写 starting」做成临界区。
//!
//! **临界区必须窄**：只覆盖「分配端口 + 写 starting 记录」这几十毫秒。
//! readiness 轮询（可能数秒）必须在释放锁之后做，否则一个慢启动会串行化
//! 所有实例的所有操作（`down --all` 会被单个 `up` 卡住）。

use std::fs::OpenOptions;
use std::path::Path;

use nix::fcntl::{Flock, FlockArg};

use super::CtlError;

/// 持有的目录锁；析构即解锁（`Flock` 是 RAII）。
pub struct DirLock {
    _inner: Flock<std::fs::File>,
}

/// 非阻塞尝试加排他锁。
///
/// 非阻塞是刻意的：拿不到就说明另一个 ctl 正在操作，应立刻告诉用户，
/// 而不是排队等待（`up` 可能因 readiness 而长时间持锁——见模块注释）。
pub fn acquire(lock_path: &Path) -> Result<DirLock, CtlError> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(lock_path)
        .map_err(|e| {
            CtlError::general(
                "state_dir_unwritable",
                format!("打开锁文件失败 {}：{e}", lock_path.display()),
            )
        })?;

    let inner = Flock::lock(file, FlockArg::LockExclusiveNonblock).map_err(|(_f, errno)| {
        CtlError::general(
            "lock_busy",
            format!(
                "另一个 ctl 正在操作实例状态（锁 {}）：{errno}",
                lock_path.display()
            ),
        )
    })?;
    Ok(DirLock { _inner: inner })
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_fixtures::tempdir as tmpdir;

    #[test]
    fn second_acquire_fails_while_held() {
        let dir = tmpdir("held");
        let lock_path = dir.join(".lock");
        let first = acquire(&lock_path).expect("首次加锁应成功");
        let Err(second) = acquire(&lock_path) else {
            panic!("锁已被占用时第二次加锁应失败");
        };
        assert_eq!(second.error_code, "lock_busy");
        drop(first);
        // 释放后可再次加锁
        acquire(&lock_path).expect("释放后应可再次加锁");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
