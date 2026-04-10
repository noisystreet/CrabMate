//! 启发式「变更影响面草图」：按符号在工作区 `.rs` 中扫描 `use` 与简单调用形态，按目录聚合为检查清单。
//! 非 rustc 真调用图；宏/动态派发/跨语言调用可能漏报，字符串中的标识符可能误报。

use ignore::WalkBuilder;
use regex::{Regex, RegexBuilder};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

const MAX_FILE_BYTES: usize = 2 * 1024 * 1024;
const DEFAULT_MAX_EDGES: usize = 400;
const MAX_EDGES_CAP: usize = 3000;
const DEFAULT_MAX_FILES: usize = 12_000;

struct SketchParams {
    symbols: Vec<String>,
    sub_path: Option<String>,
    max_edges: usize,
    max_files: usize,
    include_hidden: bool,
}

#[derive(Debug, Clone)]
struct Edge {
    file_rel: String,
    line_no: usize,
    kind: &'static str,
    line_trim: String,
}

fn parse_params(args_json: &str) -> Result<SketchParams, String> {
    let v: serde_json::Value = crate::tools::parse_args_json(args_json)?;
    let mut symbols: Vec<String> = v
        .get("symbols")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(str::trim).filter(|s| !s.is_empty()))
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if symbols.is_empty()
        && let Some(s) = v.get("symbol").and_then(|x| x.as_str()).map(str::trim)
        && !s.is_empty()
    {
        symbols.push(s.to_string());
    }
    if symbols.is_empty() {
        return Err("缺少 symbols（或非空 symbol）".to_string());
    }
    symbols.sort();
    symbols.dedup();

    let sub_path = v
        .get("path")
        .and_then(|p| p.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let max_edges = v
        .get("max_edges")
        .and_then(|n| n.as_u64())
        .map(|n| n.max(1) as usize)
        .unwrap_or(DEFAULT_MAX_EDGES)
        .min(MAX_EDGES_CAP);

    let max_files = v
        .get("max_files")
        .and_then(|n| n.as_u64())
        .map(|n| n.max(1) as usize)
        .unwrap_or(DEFAULT_MAX_FILES)
        .min(50_000);

    let include_hidden = v
        .get("include_hidden")
        .and_then(|b| b.as_bool())
        .unwrap_or(false);

    Ok(SketchParams {
        symbols,
        sub_path,
        max_edges,
        max_files,
        include_hidden,
    })
}

fn resolve_root(base: &Path, sub: Option<&str>) -> Result<PathBuf, String> {
    match sub {
        None => Ok(base.to_path_buf()),
        Some(s) => {
            let sub_path = Path::new(s);
            if sub_path.is_absolute() {
                return Err("路径必须为工作区内相对路径，不能使用绝对路径".to_string());
            }
            if s.contains("..") {
                return Err("路径不能包含 ..".to_string());
            }
            let joined = base.join(sub_path);
            let canon_base = base
                .canonicalize()
                .map_err(|e| format!("工作区根目录无法解析: {}", e))?;
            let canon_joined = joined
                .canonicalize()
                .map_err(|e| format!("搜索路径无法解析: {}", e))?;
            if !canon_joined.starts_with(&canon_base) {
                return Err("搜索路径不能超出工作区根目录".to_string());
            }
            Ok(canon_joined)
        }
    }
}

fn rel_path_under_workspace(workspace_root: &Path, abs: &Path) -> String {
    let Ok(base) = crate::tools::file::canonical_workspace_root(workspace_root) else {
        return abs.display().to_string();
    };
    match abs.strip_prefix(&base) {
        Ok(rel) => {
            let s = rel.to_string_lossy().replace('\\', "/");
            if s.is_empty() { ".".to_string() } else { s }
        }
        Err(_) => abs.display().to_string(),
    }
}

fn coarse_dir_for_file(rel_file: &str) -> String {
    Path::new(rel_file)
        .parent()
        .map(|p| {
            let s = p.to_string_lossy();
            if s.is_empty() || s == "." {
                ".".to_string()
            } else {
                s.replace('\\', "/")
            }
        })
        .unwrap_or_else(|| ".".to_string())
}

/// `use` 子句（不含分号）是否以该 `symbol` 为路径终点或被花括号列出。
fn use_clause_touches_symbol(clause: &str, symbol: &str) -> bool {
    let clause = clause.trim();
    if let Some((prefix, inner)) = split_brace_group(clause) {
        let pfx = prefix.trim();
        for part in inner.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let path_part = part.split(" as ").next().unwrap_or(part).trim();
            if path_ends_with_symbol(path_part, symbol) {
                return true;
            }
        }
        if !pfx.is_empty() && use_clause_touches_symbol(pfx, symbol) {
            return true;
        }
        return false;
    }
    let path_part = clause.split(" as ").next().unwrap_or(clause).trim();
    path_ends_with_symbol(path_part, symbol)
}

fn split_brace_group(s: &str) -> Option<(&str, &str)> {
    let open = s.find('{')?;
    let close = s.rfind('}')?;
    if close <= open {
        return None;
    }
    Some((&s[..open], &s[open + 1..close]))
}

fn path_ends_with_symbol(path: &str, symbol: &str) -> bool {
    let path = path.trim();
    if path.is_empty() {
        return false;
    }
    if path == symbol {
        return true;
    }
    let suffix = format!("::{symbol}");
    path.ends_with(&suffix)
}

fn line_is_use_import(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("use ")
        || t.starts_with("pub use ")
        || t.starts_with("pub(crate) use ")
        || t.starts_with("pub(super) use ")
        || t.starts_with("pub(self) use ")
}

fn strip_line_comment_code(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_string: Option<char> = None;
    let mut prev_escape = false;
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if let Some(q) = in_string {
            out.push(c);
            if !prev_escape && c == q {
                in_string = None;
            }
            prev_escape = c == '\\' && !prev_escape;
            i += 1;
            continue;
        }
        if c == '/' && i + 1 < chars.len() {
            if chars[i + 1] == '/' {
                break;
            }
            if chars[i + 1] == '*' {
                i += 2;
                while i + 1 < chars.len() {
                    if chars[i] == '*' && chars[i + 1] == '/' {
                        i += 2;
                        break;
                    }
                    i += 1;
                }
                continue;
            }
        }
        if (c == '"' || c == '\'') && !prev_escape {
            in_string = Some(c);
            out.push(c);
            prev_escape = false;
            i += 1;
            continue;
        }
        out.push(c);
        prev_escape = c == '\\';
        i += 1;
    }
    out
}

fn scan_file_for_edges(
    rel_path: &str,
    content: &str,
    symbol_res: &[(String, Regex)],
    edges: &mut Vec<Edge>,
    max_edges: usize,
) {
    if edges.len() >= max_edges {
        return;
    }
    for (idx, raw_line) in content.lines().enumerate() {
        if edges.len() >= max_edges {
            break;
        }
        let line_no = idx + 1;
        let code = strip_line_comment_code(raw_line);
        let trimmed = code.trim();
        if trimmed.is_empty() {
            continue;
        }

        if line_is_use_import(trimmed) {
            if let Some(rest) = trimmed
                .strip_prefix("use ")
                .or_else(|| trimmed.strip_prefix("pub use "))
                .or_else(|| trimmed.strip_prefix("pub(crate) use "))
                .or_else(|| trimmed.strip_prefix("pub(super) use "))
                .or_else(|| trimmed.strip_prefix("pub(self) use "))
            {
                let rest = rest.trim().trim_end_matches(';').trim();
                for (sym, _) in symbol_res {
                    if use_clause_touches_symbol(rest, sym) {
                        edges.push(Edge {
                            file_rel: rel_path.to_string(),
                            line_no,
                            kind: "use",
                            line_trim: truncate(trimmed, 200),
                        });
                        break;
                    }
                }
            }
            continue;
        }

        for (sym, re) in symbol_res {
            if !re.is_match(&code) {
                continue;
            }
            if crate::tools::symbol::line_looks_like_rust_definition(trimmed, sym, false) {
                continue;
            }
            edges.push(Edge {
                file_rel: rel_path.to_string(),
                line_no,
                kind: "reference",
                line_trim: truncate(trimmed, 200),
            });
            break;
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    format!(
        "{}…",
        s.chars().take(max.saturating_sub(1)).collect::<String>()
    )
}

pub fn run(args_json: &str, workspace_root: &Path) -> String {
    let params = match parse_params(args_json) {
        Ok(p) => p,
        Err(e) => return format!("错误：{e}"),
    };

    let root = match resolve_root(workspace_root, params.sub_path.as_deref()) {
        Ok(p) => p,
        Err(e) => return format!("错误：{e}"),
    };

    let mut symbol_res: Vec<(String, Regex)> = Vec::new();
    for sym in &params.symbols {
        let esc = regex::escape(sym);
        let pat = format!(r"\b{esc}\b");
        match RegexBuilder::new(&pat).build() {
            Ok(r) => symbol_res.push((sym.clone(), r)),
            Err(e) => return format!("错误：无法为符号 \"{sym}\" 构建匹配规则：{e}"),
        }
    }

    let mut edges: Vec<Edge> = Vec::new();
    let mut files_seen = 0usize;

    let walker = WalkBuilder::new(&root)
        .hidden(!params.include_hidden)
        .git_ignore(true)
        .git_global(false)
        .git_exclude(true)
        .build();

    for entry in walker {
        if files_seen >= params.max_files || edges.len() >= params.max_edges {
            break;
        }
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        files_seen += 1;

        let rel = rel_path_under_workspace(workspace_root, path);
        let mut buf = String::new();
        if let Ok(mut f) = fs::File::open(path)
            && f.read_to_string(&mut buf).is_ok()
        {
            if buf.len() > MAX_FILE_BYTES {
                buf.truncate(MAX_FILE_BYTES);
            }
            scan_file_for_edges(&rel, &buf, &symbol_res, &mut edges, params.max_edges);
        }
    }

    let truncated_edges = edges.len() >= params.max_edges;
    let truncated_files = files_seen >= params.max_files;

    let mut by_dir: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for e in &edges {
        let dir = coarse_dir_for_file(&e.file_rel);
        by_dir.entry(dir).or_default().insert(e.file_rel.clone());
    }

    let mut out = String::new();
    out.push_str("# 变更影响面草图（启发式静态扫描）\n\n");
    out.push_str("> **说明**：基于符号词边界匹配 + `use` 子句解析 + 非定义行引用启发式；**不是** rustc 调用图。宏展开、动态派发、FFI、生成的代码可能漏报；字符串或注释已尽量剥离但仍可能有误报。\n\n");
    out.push_str("## 默认检查清单（按目录聚合）\n\n");
    out.push_str(
        "改 API / 重命名符号前，建议依次核对下列目录下的命中文件（或运行测试覆盖相关 crate）。\n\n",
    );

    if by_dir.is_empty() {
        out.push_str("_（未发现命中；可缩小 path、检查符号拼写，或改用 `find_references` / `rust_analyzer_find_references` 精查。）_\n\n");
    } else {
        out.push_str("| 目录（粗粒度） | 命中文件数 | 表示例文件 |\n| --- | ---: | --- |\n");
        for (dir, files) in &by_dir {
            let mut sample: Vec<_> = files.iter().take(3).cloned().collect();
            sample.sort();
            let sample_s = sample.join("`, `");
            let tail = if files.len() > 3 {
                format!(" …（共 {} 个文件）", files.len())
            } else {
                String::new()
            };
            out.push_str(&format!(
                "| `{}` | {} | `{}`{} |\n",
                dir,
                files.len(),
                sample_s,
                tail
            ));
        }
        out.push('\n');
    }

    out.push_str("## 符号\n\n");
    out.push_str(&format!("`{}`\n\n", params.symbols.join("`, `")));

    out.push_str("## 扫描范围\n\n");
    out.push_str(&format!(
        "- 根：`{}`\n- 已扫描 `.rs` 文件数（上限 {}）：{}\n- 命中边数（上限 {}）：{}{}\n",
        root.display(),
        params.max_files,
        files_seen,
        params.max_edges,
        edges.len(),
        if truncated_edges || truncated_files {
            "（**已达上限，结果可能截断**）"
        } else {
            ""
        }
    ));
    out.push('\n');

    out.push_str("## 明细（节选）\n\n");
    let show = edges.len().min(80);
    for e in edges.iter().take(show) {
        out.push_str(&format!(
            "- `{}:{}` — **{}** — {}\n",
            e.file_rel, e.line_no, e.kind, e.line_trim
        ));
    }
    if edges.len() > show {
        out.push_str(&format!(
            "\n… 另有 {} 条未展示（可调大 `max_edges` 或缩小 `path`）\n",
            edges.len() - show
        ));
    }

    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn use_clause_brace_list() {
        assert!(use_clause_touches_symbol("foo::{Bar, Baz}", "Bar"));
        assert!(!use_clause_touches_symbol("foo::{Bar, Baz}", "Qux"));
    }

    #[test]
    fn use_clause_as_rename() {
        assert!(use_clause_touches_symbol("crate::api::Thing as T", "Thing"));
    }
}
