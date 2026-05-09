//! Manager 调用 LLM 分解**之前**，对工作区内目录做轻量 `list_tree`，注入提示词，
//! 减少子目标 `description` 中臆造路径/文件名（与「Manager 污染」缓解配套）。

use std::collections::HashSet;
use std::path::Path;

use regex::Regex;

use crate::tools::list_tree;

/// 单棵树默认深度与条数上限（与 Operator 全量 list_tree 上限解耦，避免提示词爆炸）。
const SNAPSHOT_MAX_DEPTH: u64 = 4;
const SNAPSHOT_MAX_ENTRIES: u64 = 350;
/// 从任务文本解析到的候选路径最多尝试几个
const MAX_EXTRACTED_DIRS: usize = 6;
/// 无显式路径时的回退：工作区一级子目录最多扫几个
const FALLBACK_TOPLEVEL_DIRS: usize = 8;
/// 注入文本总字符上限（超出则截断并附说明）
const SNAPSHOT_TOTAL_CHAR_BUDGET: usize = 16_000;

/// 从用户任务文本中提取可能的工作区相对路径（目录），供分解前 list_tree。
///
/// - 含 `/` 的 token（过滤 `://` 与绝对路径）
/// - 常见「项目名-版本号」单段目录名（如 `BabelStream-5.0`）
fn first_segment_suspected_url_host(seg: &str) -> bool {
    let s = seg.to_ascii_lowercase();
    s.starts_with("www.")
        || s.ends_with(".com")
        || s.ends_with(".org")
        || s.ends_with(".net")
        || s.ends_with(".gov")
}

pub(crate) fn extract_path_candidates(task: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();

    let slash_paths = Regex::new(r"(?m)\b((?:\./)?[A-Za-z0-9_.-]+(?:/[A-Za-z0-9_.-]+)+)\b")
        .expect("regex compile");
    for cap in slash_paths.captures_iter(task) {
        let p = cap.get(1).map(|m| m.as_str()).unwrap_or("").trim();
        let p = p.trim_start_matches("./");
        if p.is_empty() || p.contains("://") || p.starts_with('/') {
            continue;
        }
        if p.split('/')
            .next()
            .is_some_and(first_segment_suspected_url_host)
        {
            continue;
        }
        if seen.insert(p.to_string()) {
            out.push(p.to_string());
        }
    }

    let version_dir =
        Regex::new(r"\b([A-Za-z][A-Za-z0-9_-]{2,}-[0-9]+(?:\.[0-9]+)+)\b").expect("regex compile");
    for cap in version_dir.captures_iter(task) {
        let p = cap.get(1).map(|m| m.as_str()).unwrap_or("").trim();
        if p.is_empty() || seen.contains(p) {
            continue;
        }
        seen.insert(p.to_string());
        out.push(p.to_string());
    }

    out
}

fn strip_list_tree_first_json_line(output: &str) -> String {
    let mut lines = output.lines();
    let Some(first) = lines.next() else {
        return output.to_string();
    };
    if first.trim_start().starts_with('{') {
        lines.collect::<Vec<_>>().join("\n")
    } else {
        output.to_string()
    }
}

fn rel_dir_exists(ws: &Path, rel: &str) -> bool {
    if rel.is_empty() || rel.contains("..") || rel.starts_with('/') {
        return false;
    }
    let p = ws.join(rel);
    p.is_dir()
}

fn fallback_toplevel_dirs(ws: &Path, limit: usize) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(ws) else {
        return Vec::new();
    };
    let mut names: Vec<String> = rd
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| !n.starts_with('.'))
        .collect();
    names.sort();
    names.into_iter().take(limit).collect()
}

fn run_list_tree_snapshot(working_dir: &Path, rel_root: &str) -> String {
    let args = format!(
        r#"{{"path":{:?},"max_depth":{},"max_entries":{},"include_hidden":false}}"#,
        rel_root, SNAPSHOT_MAX_DEPTH, SNAPSHOT_MAX_ENTRIES
    );
    list_tree(&args, working_dir)
}

/// 分解 / 重规划前采集目录树摘要，供 Manager 提示词使用。
///
/// 优先对用户任务中出现的、且在工作区内真实存在的相对目录各跑一次 `list_tree`；
/// 若一个都匹配不上，则对工作区**一级子目录**各做一次浅层树（深度与条数更紧），避免完全盲分解。
pub(crate) fn gather_pre_decompose_snapshots(working_dir: &Path, task: &str) -> String {
    if !working_dir.is_dir() {
        return "(工作目录不可用，跳过分解前快照。)".to_string();
    }

    let mut dirs: Vec<String> = extract_path_candidates(task)
        .into_iter()
        .filter(|p| rel_dir_exists(working_dir, p))
        .take(MAX_EXTRACTED_DIRS)
        .collect();

    if dirs.is_empty() {
        dirs = fallback_toplevel_dirs(working_dir, FALLBACK_TOPLEVEL_DIRS)
            .into_iter()
            .filter(|p| rel_dir_exists(working_dir, p))
            .take(MAX_EXTRACTED_DIRS)
            .collect();
    }

    if dirs.is_empty() {
        return "(工作区内未找到可用于快照的子目录；仅依赖下方「工作目录上下文」。)".to_string();
    }

    let mut blocks = Vec::new();
    let mut total = 0usize;

    'outer: for rel in dirs {
        let raw = run_list_tree_snapshot(working_dir, &rel);
        let body = strip_list_tree_first_json_line(&raw);
        let block = format!("### `{}`\n```text\n{}\n```\n", rel, body.trim());
        if total + block.len() > SNAPSHOT_TOTAL_CHAR_BUDGET {
            blocks.push(format!(
                "…（已达快照总长度上限 {} 字符，后续目录省略）",
                SNAPSHOT_TOTAL_CHAR_BUDGET
            ));
            break 'outer;
        }
        total += block.len();
        blocks.push(block);
    }

    let trees = blocks.join("\n");
    format!(
        r#"## 分解前目录快照（系统自动 list_tree）

以下树来自**当前工作区**内真实目录（分解前自动采集），用于约束子目标描述：
**仅可引用此处出现的相对路径与文件名**；禁止臆造未出现的文件（如虚构的 `.hpp` 名）。

{}

**纪律**：若某路径不在上述任一快照中，子目标应写「先对相应目录 `list_tree`/`glob_files` 再读」，不得在首轮分解中写死具体文件名。
"#,
        trees.trim_end()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_slash_and_version_token() {
        let t = "请分析 BabelStream-5.0/src 与 foo/bar 下的代码";
        let v = extract_path_candidates(t);
        assert!(v.iter().any(|x| x == "BabelStream-5.0"));
        assert!(
            v.iter()
                .any(|x| x.contains("BabelStream-5.0/src") || x == "foo/bar")
        );
    }

    #[test]
    fn extract_ignores_urls() {
        let t = "see https://example.com/foo/bar";
        let v = extract_path_candidates(t);
        assert!(!v.iter().any(|x| x.contains("://")));
    }
}
