//! 单轮 `run_agent_turn` 内的 `read_file` 缓存与 revision 绑定。
//!
//! - 正文缓存：按 **规范化路径 + 读取参数** 索引，命中时比对 [`FileStamp`]
//!   （Unix 下含 `dev` / `ino` / `ctime`，避免同大小同 mtime 时命中旧票据）；
//!   任意写类工具或 `workspace_changed` 后整表清空以防脏读。
//! - revision 绑定：记录 **规范化路径 → 最近一次成功 `read_file` 的内容 revision**，
//!   供 `modify_file` 的行号类模式自动校验。写类工具后的 [`clear`](ReadFileTurnCache::clear)
//!   **不会**清空绑定：同轮 `read → write A → write B` 的第二次写入仍须被旧 revision 拦住。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

/// 每轮 revision 绑定上限；超限时整表清空（与正文缓存同策略）。
const REVISION_BIND_MAX_ENTRIES: usize = 1024;

/// 文件新鲜度指纹。
///
/// 仅比对 `mtime + size` 时，同一秒内被原子替换（temp + rename）成相同长度的文件会
/// 命中旧缓存。Unix 下额外记录 `dev` / `ino` / `ctime`：原子替换必然更换 inode，
/// 原地改写同一 inode 也会推进 ctime。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileStamp {
    modified: SystemTime,
    len: u64,
    #[cfg(unix)]
    dev: u64,
    #[cfg(unix)]
    ino: u64,
    #[cfg(unix)]
    ctime: i64,
    #[cfg(unix)]
    ctime_nsec: i64,
}

impl FileStamp {
    pub fn from_metadata(meta: &std::fs::Metadata) -> Self {
        let modified = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
        let len = meta.len();
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Self {
                modified,
                len,
                dev: meta.dev(),
                ino: meta.ino(),
                ctime: meta.ctime(),
                ctime_nsec: meta.ctime_nsec(),
            }
        }
        #[cfg(not(unix))]
        {
            Self { modified, len }
        }
    }
}

/// 缓存条目：磁盘指纹与 [`FileStamp`] 全等时才可复用正文。
#[derive(Clone)]
pub struct ReadFileCacheEntry {
    pub stamp: FileStamp,
    pub output: String,
    pub revision: String,
}

/// 一轮对话内的 `read_file` 缓存（`Arc<Mutex<…>>` 由 `run_agent_turn` 创建并共享给各工具调用）。
pub struct ReadFileTurnCache {
    inner: Mutex<ReadFileTurnCacheInner>,
}

struct ReadFileTurnCacheInner {
    map: HashMap<String, ReadFileCacheEntry>,
    /// 规范化路径 → 最近一次成功 `read_file` 的内容 revision。
    ///
    /// 生命周期与整轮一致，**不**随 [`clear`](ReadFileTurnCache::clear) 清空；只在新的
    /// `read_file` 成功（含缓存命中）时更新，从而保证同轮多次写入仍以“最近一次读取”为基准。
    revisions: HashMap<PathBuf, String>,
    /// `max_entries == 0` 时关闭正文缓存，但 revision 绑定仍然工作。
    content_enabled: bool,
    max_entries: usize,
}

impl ReadFileTurnCache {
    /// `max_entries == 0`：关闭正文缓存（仍保留 revision 自动绑定）。
    pub fn new(max_entries: usize) -> Self {
        Self {
            inner: Mutex::new(ReadFileTurnCacheInner {
                map: HashMap::new(),
                revisions: HashMap::new(),
                content_enabled: max_entries > 0,
                max_entries: max_entries.max(1),
            }),
        }
    }

    /// 写工具、`workspace_changed` 等之后调用，避免返回陈旧正文。
    ///
    /// 注意：**不清** revision 绑定，否则同轮第二次写入会因绑定丢失而漏检。
    pub fn clear(&self) {
        if let Ok(mut g) = self.inner.lock() {
            g.map.clear();
        }
    }

    /// 命中时返回 `(正文, revision)`；指纹（含 inode / ctime）须全等。
    pub fn try_get(&self, key: &str, stamp: &FileStamp) -> Option<(String, String)> {
        let g = self.inner.lock().ok()?;
        if !g.content_enabled {
            return None;
        }
        let e = g.map.get(key)?;
        if &e.stamp == stamp {
            Some((e.output.clone(), e.revision.clone()))
        } else {
            None
        }
    }

    pub fn insert(
        &self,
        key: String,
        stamp: FileStamp,
        output: String,
        revision: String,
    ) {
        let Ok(mut g) = self.inner.lock() else {
            return;
        };
        if !g.content_enabled {
            return;
        }
        if g.map.len() >= g.max_entries {
            g.map.clear();
        }
        g.map.insert(
            key,
            ReadFileCacheEntry {
                stamp,
                output,
                revision,
            },
        );
    }

    /// 记录某路径最近一次成功 `read_file` 的 revision（供 `modify_file` 自动绑定校验）。
    pub fn record_revision(&self, path: &Path, revision: &str) {
        let Ok(mut g) = self.inner.lock() else {
            return;
        };
        if g.revisions.len() >= REVISION_BIND_MAX_ENTRIES && !g.revisions.contains_key(path) {
            g.revisions.clear();
        }
        g.revisions.insert(path.to_path_buf(), revision.to_string());
    }

    /// 取某路径本轮最近一次成功 `read_file` 的 revision。
    pub fn last_read_revision(&self, path: &Path) -> Option<String> {
        let g = self.inner.lock().ok()?;
        g.revisions.get(path).cloned()
    }
}

pub type ReadFileTurnCacheHandle = Arc<ReadFileTurnCache>;

/// 供 `run_agent_turn` 构造句柄。
pub fn new_turn_cache_handle(max_entries: usize) -> ReadFileTurnCacheHandle {
    Arc::new(ReadFileTurnCache::new(max_entries))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    /// 测试用指纹：Unix 下 `identity` 映射到 inode / ctime，非 Unix 下忽略。
    fn stamp(modified: SystemTime, len: u64, identity: u64) -> FileStamp {
        #[cfg(unix)]
        {
            FileStamp {
                modified,
                len,
                dev: 1,
                ino: identity,
                ctime: identity as i64,
                ctime_nsec: 0,
            }
        }
        #[cfg(not(unix))]
        {
            let _ = identity;
            FileStamp { modified, len }
        }
    }

    #[test]
    fn cache_hit_when_stamp_matches() {
        let c = ReadFileTurnCache::new(8);
        let t = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        c.insert(
            "k1".into(),
            stamp(t, 42, 1),
            "hello".into(),
            "sha256:aa".into(),
        );
        assert_eq!(
            c.try_get("k1", &stamp(t, 42, 1)),
            Some(("hello".into(), "sha256:aa".into()))
        );
        assert!(c.try_get("k1", &stamp(t, 41, 1)).is_none());
        // 同 size / mtime 但身份字段（inode）不同：不得命中。
        #[cfg(unix)]
        assert!(c.try_get("k1", &stamp(t, 42, 2)).is_none());
    }

    #[test]
    fn clear_removes_entries_but_keeps_revision_bindings() {
        let c = ReadFileTurnCache::new(8);
        let s = stamp(UNIX_EPOCH, 1, 1);
        let path = Path::new("/tmp/crabmate-clear-test.txt");
        c.insert("k".into(), s, "x".into(), "sha256:bb".into());
        c.record_revision(path, "sha256:bb");
        c.clear();
        assert!(c.try_get("k", &s).is_none());
        assert_eq!(c.last_read_revision(path).as_deref(), Some("sha256:bb"));
    }

    #[test]
    fn zero_max_entries_disables_content_cache_but_keeps_bindings() {
        let c = ReadFileTurnCache::new(0);
        let s = stamp(UNIX_EPOCH, 1, 1);
        let path = Path::new("/tmp/crabmate-zero-test.txt");
        c.insert("k".into(), s, "x".into(), "sha256:cc".into());
        assert!(c.try_get("k", &s).is_none());
        c.record_revision(path, "sha256:cc");
        assert_eq!(c.last_read_revision(path).as_deref(), Some("sha256:cc"));
    }

    #[test]
    fn record_revision_overwrites_previous_binding() {
        let c = ReadFileTurnCache::new(8);
        let path = Path::new("/tmp/crabmate-overwrite-test.txt");
        c.record_revision(path, "sha256:old");
        c.record_revision(path, "sha256:new");
        assert_eq!(c.last_read_revision(path).as_deref(), Some("sha256:new"));
        assert!(c.last_read_revision(Path::new("/tmp/other.txt")).is_none());
    }

    /// 同 size、同 mtime，但文件被 temp + rename 原子替换（inode 变化）后不得命中旧缓存。
    #[cfg(unix)]
    #[test]
    fn inode_change_invalidates_cache_with_same_size_and_mtime() {
        use std::fs;
        use std::time::SystemTime;

        fn set_mtime(path: &Path, t: SystemTime) {
            let f = fs::File::options().write(true).open(path).unwrap();
            f.set_times(fs::FileTimes::new().set_modified(t)).unwrap();
        }

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.txt");
        fs::write(&path, b"hello").unwrap();
        let fixed = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        set_mtime(&path, fixed);
        let before = FileStamp::from_metadata(&fs::metadata(&path).unwrap());

        let tmp = dir.path().join("f.tmp");
        fs::write(&tmp, b"world").unwrap();
        set_mtime(&tmp, fixed);
        fs::rename(&tmp, &path).unwrap();
        let after = FileStamp::from_metadata(&fs::metadata(&path).unwrap());

        assert_eq!(before.len, after.len);
        assert_eq!(before.modified, after.modified);
        assert_ne!(before, after, "原子替换应改变 inode/ctime 指纹");

        let c = ReadFileTurnCache::new(8);
        c.insert("k1".into(), before, "hello".into(), "sha256:old".into());
        assert!(c.try_get("k1", &after).is_none());
    }
}
