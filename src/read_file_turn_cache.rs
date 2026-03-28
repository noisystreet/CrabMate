//! 单轮 `run_agent_turn` 内 `read_file` 结果缓存：按 **规范化路径 + 读取参数** 索引，命中时比对 **mtime + size**；任意写类工具或 `workspace_changed` 后整表清空以防脏读。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

/// 缓存条目：与磁盘 `metadata().modified()` + `len()` 一致时才可复用正文。
#[derive(Clone)]
pub(crate) struct ReadFileCacheEntry {
    pub modified: SystemTime,
    pub len: u64,
    pub output: String,
}

/// 一轮对话内的 `read_file` 缓存（`Arc<Mutex<…>>` 由 `run_agent_turn` 创建并共享给各工具调用）。
pub struct ReadFileTurnCache {
    inner: Mutex<ReadFileTurnCacheInner>,
}

struct ReadFileTurnCacheInner {
    map: HashMap<String, ReadFileCacheEntry>,
    max_entries: usize,
}

impl ReadFileTurnCache {
    pub(crate) fn new(max_entries: usize) -> Self {
        Self {
            inner: Mutex::new(ReadFileTurnCacheInner {
                map: HashMap::new(),
                max_entries: max_entries.max(1),
            }),
        }
    }

    /// 写工具、`workspace_changed` 等之后调用，避免返回陈旧片段。
    pub(crate) fn clear(&self) {
        if let Ok(mut g) = self.inner.lock() {
            g.map.clear();
        }
    }

    pub(crate) fn try_get(&self, key: &str, modified: SystemTime, len: u64) -> Option<String> {
        let g = self.inner.lock().ok()?;
        let e = g.map.get(key)?;
        if e.modified == modified && e.len == len {
            Some(e.output.clone())
        } else {
            None
        }
    }

    pub(crate) fn insert(&self, key: String, modified: SystemTime, len: u64, output: String) {
        let Ok(mut g) = self.inner.lock() else {
            return;
        };
        if g.map.len() >= g.max_entries {
            g.map.clear();
        }
        g.map.insert(
            key,
            ReadFileCacheEntry {
                modified,
                len,
                output,
            },
        );
    }
}

pub type ReadFileTurnCacheHandle = Arc<ReadFileTurnCache>;

/// 供 `run_agent_turn` 在启用缓存时构造句柄。
pub fn new_turn_cache_handle(max_entries: usize) -> ReadFileTurnCacheHandle {
    Arc::new(ReadFileTurnCache::new(max_entries))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn cache_hit_when_mtime_len_match() {
        let c = ReadFileTurnCache::new(8);
        let t = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        c.insert("k1".into(), t, 42, "hello".into());
        assert_eq!(c.try_get("k1", t, 42).as_deref(), Some("hello"));
        assert!(c.try_get("k1", t, 41).is_none());
    }

    #[test]
    fn clear_removes_entries() {
        let c = ReadFileTurnCache::new(8);
        let t = UNIX_EPOCH;
        c.insert("k".into(), t, 1, "x".into());
        c.clear();
        assert!(c.try_get("k", t, 1).is_none());
    }
}
