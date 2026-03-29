//! 进程内 **`cargo test` / `npm test`** 结果缓存：输入指纹（源码清单 + 参数）未变时复用上次截断后的输出，并标注 **缓存命中**。
//!
//! **非**跨进程持久化；**不**保证与真实磁盘状态强一致（指纹为 mtime+size 近似）。`RUST_TEST_THREADS` 等环境变量变化**不会**自动失效缓存——需关闭缓存或改源码触发指纹变化。

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use blake3::Hasher;
use ignore::WalkBuilder;
use serde_json::json;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) enum TestCacheKind {
    CargoTest,
    CargoTestViaRunCommand,
    NpmTest { package_subdir: String },
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct TestCacheKey {
    pub workspace_root: PathBuf,
    pub kind: TestCacheKind,
    /// 规范化后的调用参数摘要（JSON 字符串，稳定字段顺序由调用方保证）。
    pub args_fingerprint: String,
    /// 工作区源码 / 清单指纹（blake3 hex）。
    pub inputs_fingerprint: String,
}

struct CachedEntry {
    output: String,
}

struct LruCache {
    map: HashMap<TestCacheKey, CachedEntry>,
    order: VecDeque<TestCacheKey>,
    cap: usize,
}

impl LruCache {
    fn new(cap: usize) -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
            cap: cap.max(1),
        }
    }

    fn get(&mut self, k: &TestCacheKey) -> Option<String> {
        if !self.map.contains_key(k) {
            return None;
        }
        let out = self.map.get(k).map(|e| e.output.clone())?;
        if let Some(pos) = self.order.iter().position(|x| x == k) {
            self.order.remove(pos);
        }
        self.order.push_back(k.clone());
        Some(out)
    }

    fn insert(&mut self, k: TestCacheKey, output: String) {
        if self.map.contains_key(&k) {
            self.map.insert(
                k.clone(),
                CachedEntry {
                    output: output.clone(),
                },
            );
            if let Some(pos) = self.order.iter().position(|x| x == &k) {
                self.order.remove(pos);
            }
            self.order.push_back(k);
            return;
        }
        while self.map.len() >= self.cap && !self.order.is_empty() {
            if let Some(old) = self.order.pop_front() {
                self.map.remove(&old);
            }
        }
        self.map.insert(
            k.clone(),
            CachedEntry {
                output: output.clone(),
            },
        );
        self.order.push_back(k);
    }
}

static CACHE: Mutex<Option<LruCache>> = Mutex::new(None);

fn cache_singleton(max_entries: usize) -> std::sync::MutexGuard<'static, Option<LruCache>> {
    let mut g = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if g.is_none() {
        *g = Some(LruCache::new(max_entries.max(1)));
    } else if let Some(c) = g.as_mut()
        && c.cap != max_entries.max(1)
    {
        *g = Some(LruCache::new(max_entries.max(1)));
    }
    g
}

/// 遍历工作区（尊重 `.gitignore`），对 `.rs` / `.toml` / `Cargo.lock` 记录相对路径 + mtime 纳秒 + 长度，计算 blake3。
pub(crate) fn fingerprint_rust_workspace_sources(root: &Path) -> Option<String> {
    let root = root.canonicalize().ok()?;
    let mut hasher = Hasher::new();
    let mut rows: Vec<(String, u128, u64)> = Vec::new();

    let walker = WalkBuilder::new(&root)
        .hidden(false)
        .git_ignore(true)
        .git_global(false)
        .git_exclude(true)
        .build();

    for entry in walker.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
        let track = matches!(ext, "rs" | "toml") || name == "Cargo.lock";
        if !track {
            continue;
        }
        let rel = path.strip_prefix(&root).ok()?;
        let rel_s = rel.to_string_lossy().replace('\\', "/");
        let meta = std::fs::metadata(path).ok()?;
        let mtime = meta
            .modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_nanos();
        let len = meta.len();
        rows.push((rel_s, mtime, len));
    }

    rows.sort_by(|a, b| a.0.cmp(&b.0));
    for (rel, mt, len) in rows {
        hasher.update(rel.as_bytes());
        hasher.update(&0xffu8.to_le_bytes()); // separator
        hasher.update(&mt.to_le_bytes());
        hasher.update(&len.to_le_bytes());
    }
    Some(hasher.finalize().to_hex().to_string())
}

/// `npm test` 指纹：`subdir/package.json` 与可选 lockfile 的 mtime+size。
pub(crate) fn fingerprint_npm_package_dir(workspace: &Path, subdir: &str) -> Option<String> {
    let dir = workspace.join(subdir);
    let pj = dir.join("package.json");
    if !pj.is_file() {
        return None;
    }
    let mut hasher = Hasher::new();
    for name in ["package.json", "package-lock.json", "npm-shrinkwrap.json"] {
        let p = dir.join(name);
        if !p.is_file() {
            continue;
        }
        hasher.update(name.as_bytes());
        hasher.update(&0xffu8.to_le_bytes());
        let meta = std::fs::metadata(&p).ok()?;
        let mt = meta
            .modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_nanos();
        hasher.update(&mt.to_le_bytes());
        hasher.update(&meta.len().to_le_bytes());
    }
    Some(hasher.finalize().to_hex().to_string())
}

const CACHE_BANNER: &str =
    "[CrabMate 测试输出缓存命中] 输入指纹与上次相同，未重新执行；以下为缓存副本。\n指纹：";

pub(crate) fn wrap_cache_hit(fingerprint: &str, body: &str) -> String {
    format!("{CACHE_BANNER}{fingerprint}\n---\n{body}")
}

/// `cargo test` / `rust_test_one` 参数的稳定 JSON 摘要。
pub(crate) fn cargo_test_args_fingerprint(v: &serde_json::Value) -> String {
    let release = v.get("release").and_then(|x| x.as_bool()).unwrap_or(false);
    let all_targets = v
        .get("all_targets")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let package = v.get("package").and_then(|x| x.as_str()).unwrap_or("");
    let bin = v.get("bin").and_then(|x| x.as_str()).unwrap_or("");
    let features = v.get("features").and_then(|x| x.as_str()).unwrap_or("");
    let test_filter = v.get("test_filter").and_then(|x| x.as_str()).unwrap_or("");
    let test_name = v.get("test_name").and_then(|x| x.as_str()).unwrap_or("");
    let nocapture = v
        .get("nocapture")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    serde_json::to_string(&json!({
        "release": release,
        "all_targets": all_targets,
        "package": package,
        "bin": bin,
        "features": features,
        "test_filter": test_filter,
        "test_name": test_name,
        "nocapture": nocapture,
    }))
    .unwrap_or_else(|_| "{}".to_string())
}

/// `run_command`：`cargo` + `test` 子命令的参数指纹（`args` 为完整数组）。
pub(crate) fn cargo_test_run_command_args_fingerprint(cmd_args: &[String]) -> String {
    serde_json::to_string(&json!({ "argv": cmd_args })).unwrap_or_else(|_| "[]".to_string())
}

/// `npm run test`：`subdir`、`script`、`args` 数组。
pub(crate) fn npm_test_args_fingerprint(subdir: &str, script: &str, extra: &[String]) -> String {
    serde_json::to_string(&json!({
        "subdir": subdir,
        "script": script,
        "extra": extra,
    }))
    .unwrap_or_else(|_| "{}".to_string())
}

pub(crate) fn try_get_cached(
    enabled: bool,
    max_entries: usize,
    key: &TestCacheKey,
) -> Option<String> {
    if !enabled {
        return None;
    }
    let mut guard = cache_singleton(max_entries);
    let cache = guard.as_mut()?;
    cache.get(key)
}

pub(crate) fn store_cached(enabled: bool, max_entries: usize, key: TestCacheKey, output: String) {
    if !enabled || output.is_empty() {
        return;
    }
    let mut guard = cache_singleton(max_entries);
    if let Some(cache) = guard.as_mut() {
        cache.insert(key, output);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn lru_evicts_oldest() {
        let mut c = LruCache::new(2);
        let mk = |i: u8| TestCacheKey {
            workspace_root: PathBuf::from("/w"),
            kind: TestCacheKind::CargoTest,
            args_fingerprint: format!("a{i}"),
            inputs_fingerprint: "x".to_string(),
        };
        c.insert(mk(1), "one".to_string());
        c.insert(mk(2), "two".to_string());
        c.insert(mk(3), "three".to_string());
        assert!(c.get(&mk(1)).is_none());
        assert_eq!(c.get(&mk(2)).as_deref(), Some("two"));
        assert_eq!(c.get(&mk(3)).as_deref(), Some("three"));
    }

    #[test]
    fn fingerprint_changes_when_file_touched() {
        let dir =
            std::env::temp_dir().join(format!("crabmate_test_cache_fp_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        let lib = dir.join("src/lib.rs");
        std::fs::write(&lib, "pub fn a() {}\n").unwrap();
        let a = fingerprint_rust_workspace_sources(&dir).expect("fp");
        std::thread::sleep(std::time::Duration::from_millis(20));
        let mut f = std::fs::OpenOptions::new().append(true).open(&lib).unwrap();
        writeln!(f, "// x").unwrap();
        let b = fingerprint_rust_workspace_sources(&dir).expect("fp2");
        assert_ne!(a, b);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
