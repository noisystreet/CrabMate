//! 金样 fixture 的公共读取工具（`#[cfg(test)]`）。
//!
//! 8 个 golden 测试原先各自手抄同一段「读文件 → 逐行 `trim` → 跳过空行与 `#`
//! 注释 → `serde_json` 解析 → 带 `文件:行号` 的失败文案」，另有 3 份 `line_ctx`
//! 与 2 份 `repo_root` 副本。副本各自漂移的风险高于收益，故集中于此。语义与
//! 失败文案保持不变（唯一例外见 [`for_each_jsonl_record`]）。

use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;

/// 仓库根。本 crate 就是 workspace 根，故 `CARGO_MANIFEST_DIR` 即所需路径。
#[must_use]
pub(crate) fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// 金样断言的上下文串：`<path>:<1-based 行号> (<id>)`。
///
/// 断言失败时靠它定位「哪个 fixture 的第几行、哪条用例」，因此格式被各金样共用。
#[must_use]
pub(crate) fn line_ctx(path: &Path, line_no: usize, id: &str) -> String {
    format!("{}:{} ({})", path.display(), line_no + 1, id)
}

/// 逐条读取 JSONL 金样，对每条记录回调 `(path, 0-based line_no, record)`。
///
/// `rel_path` 相对仓库根（如 `fixtures/xxx.jsonl`）。空行（`trim` 后）与 `#`
/// 开头的行跳过；解析失败立即 panic，并打印 `path:line: invalid json` 与原始行，
/// 便于改 fixture 时定位。
///
/// 与旧副本的**唯一**差异：`turn_completion_golden` 过去用
/// `expect("parse golden line")`，现在统一成上面这条可诊断文案。
pub(crate) fn for_each_jsonl_record<T, F>(rel_path: &str, mut f: F)
where
    T: DeserializeOwned,
    F: FnMut(&Path, usize, T),
{
    let path = repo_root().join(rel_path);
    let raw =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));

    for (line_no, line) in raw.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let row: T = serde_json::from_str(trimmed).unwrap_or_else(|e| {
            panic!(
                "{}:{}: invalid json: {e}\n{trimmed}",
                path.display(),
                line_no + 1
            )
        });
        f(&path, line_no, row);
    }
}
