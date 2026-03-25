//! 在工作区内按正则/关键词搜索文件内容。

use regex::RegexBuilder;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

struct SearchParams {
    pattern: String,
    sub_path: Option<String>,
    max_results: usize,
    case_insensitive: bool,
    ignore_hidden: bool,
    context_before: usize,
    context_after: usize,
    file_glob: Option<String>,
    exclude_glob: Option<String>,
}

const DEFAULT_MAX_RESULTS: usize = 200;
const MAX_FILE_SIZE_BYTES: usize = 2 * 1024 * 1024;

pub fn run(args_json: &str, workspace_root: &Path) -> String {
    let params = match parse_params(args_json) {
        Ok(p) => p,
        Err(e) => return e,
    };

    let re = match RegexBuilder::new(&params.pattern)
        .case_insensitive(params.case_insensitive)
        .build()
    {
        Ok(r) => r,
        Err(e) => return format!("错误：无效的正则表达式：{}", e),
    };

    let root = match resolve_search_root(workspace_root, params.sub_path.as_deref()) {
        Ok(p) => p,
        Err(e) => return e,
    };

    let file_pat = params
        .file_glob
        .as_deref()
        .and_then(|g| glob::Pattern::new(g).ok());
    let exclude_pat = params
        .exclude_glob
        .as_deref()
        .and_then(|g| glob::Pattern::new(g).ok());

    let opts = WalkOpts {
        max_results: params.max_results,
        ignore_hidden: params.ignore_hidden,
        ctx_before: params.context_before,
        ctx_after: params.context_after,
        file_glob: file_pat.as_ref(),
        exclude_glob: exclude_pat.as_ref(),
    };

    let mut results = Vec::new();
    let mut visited = 0usize;

    if let Err(e) = walk_and_search(&root, &re, &mut results, &mut visited, &opts) {
        return format!("搜索过程中发生错误：{}", e);
    }

    if results.is_empty() {
        return format!(
            "未找到匹配：\"{}\"（共遍历 {} 个文件，搜索根目录：{}）",
            params.pattern,
            visited,
            root.display()
        );
    }

    let mut out = String::new();
    out.push_str(&format!(
        "搜索模式：\"{}\"，根目录：{}\n匹配结果（最多 {} 条，实际 {} 条）：\n\n",
        params.pattern,
        root.display(),
        params.max_results,
        results.len()
    ));
    for (path, line_no, line) in results {
        out.push_str(&format!("{}:{}: {}\n", path.display(), line_no, line));
    }
    out
}

fn parse_params(args_json: &str) -> Result<SearchParams, String> {
    let v: serde_json::Value =
        serde_json::from_str(args_json).map_err(|e| format!("参数 JSON 无效：{}", e))?;
    let pattern = v
        .get("pattern")
        .and_then(|p| p.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "缺少 pattern 参数".to_string())?;
    let sub_path = v
        .get("path")
        .and_then(|p| p.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let max_results = v
        .get("max_results")
        .and_then(|m| m.as_u64())
        .map(|n| n.max(1) as usize)
        .unwrap_or(DEFAULT_MAX_RESULTS);
    let case_insensitive = v
        .get("case_insensitive")
        .and_then(|b| b.as_bool())
        .unwrap_or(true);
    let ignore_hidden = v
        .get("ignore_hidden")
        .and_then(|b| b.as_bool())
        .unwrap_or(true);
    let context_before = v
        .get("context_before")
        .and_then(|n| n.as_u64())
        .map(|n| n.min(10) as usize)
        .unwrap_or(0);
    let context_after = v
        .get("context_after")
        .and_then(|n| n.as_u64())
        .map(|n| n.min(10) as usize)
        .unwrap_or(0);
    let file_glob = v
        .get("file_glob")
        .and_then(|g| g.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let exclude_glob = v
        .get("exclude_glob")
        .and_then(|g| g.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    Ok(SearchParams {
        pattern,
        sub_path,
        max_results,
        case_insensitive,
        ignore_hidden,
        context_before,
        context_after,
        file_glob,
        exclude_glob,
    })
}

fn resolve_search_root(base: &Path, sub: Option<&str>) -> Result<PathBuf, String> {
    match sub {
        None => Ok(base.to_path_buf()),
        Some(s) => {
            let sub_path = Path::new(s);
            if sub_path.is_absolute() {
                return Err("路径必须为相对于工作区的相对路径，不能使用绝对路径".to_string());
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

struct WalkOpts<'a> {
    max_results: usize,
    ignore_hidden: bool,
    ctx_before: usize,
    ctx_after: usize,
    file_glob: Option<&'a glob::Pattern>,
    exclude_glob: Option<&'a glob::Pattern>,
}

fn walk_and_search(
    root: &Path,
    re: &regex::Regex,
    results: &mut Vec<(PathBuf, usize, String)>,
    visited_files: &mut usize,
    opts: &WalkOpts<'_>,
) -> io::Result<()> {
    if !root.exists() {
        return Ok(());
    }
    if root.is_file() {
        if matches_glob_filters(root, opts.file_glob, opts.exclude_glob) {
            search_in_file(root, re, results, visited_files, opts)?;
        }
        return Ok(());
    }
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        if opts.ignore_hidden && name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            walk_and_search(&path, re, results, visited_files, opts)?;
            if results.len() >= opts.max_results {
                break;
            }
        } else if path.is_file() && matches_glob_filters(&path, opts.file_glob, opts.exclude_glob) {
            search_in_file(&path, re, results, visited_files, opts)?;
            if results.len() >= opts.max_results {
                break;
            }
        }
    }
    Ok(())
}

fn matches_glob_filters(
    path: &Path,
    file_glob: Option<&glob::Pattern>,
    exclude_glob: Option<&glob::Pattern>,
) -> bool {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    if let Some(pat) = file_glob
        && !pat.matches(&name)
    {
        return false;
    }
    if let Some(pat) = exclude_glob
        && pat.matches(&name)
    {
        return false;
    }
    true
}

fn search_in_file(
    path: &Path,
    re: &regex::Regex,
    results: &mut Vec<(PathBuf, usize, String)>,
    visited_files: &mut usize,
    opts: &WalkOpts<'_>,
) -> io::Result<()> {
    *visited_files += 1;
    let mut f = fs::File::open(path)?;
    let mut buf = String::new();
    f.read_to_string(&mut buf)?;
    if buf.len() > MAX_FILE_SIZE_BYTES {
        buf.truncate(MAX_FILE_SIZE_BYTES);
    }
    let lines: Vec<&str> = buf.lines().collect();
    let has_context = opts.ctx_before > 0 || opts.ctx_after > 0;
    let mut last_ctx_end: usize = 0;

    for (idx, line) in lines.iter().enumerate() {
        if !re.is_match(line) {
            continue;
        }
        if has_context {
            let ctx_start = idx.saturating_sub(opts.ctx_before);
            let ctx_end = (idx + opts.ctx_after + 1).min(lines.len());
            if ctx_start > last_ctx_end && last_ctx_end > 0 {
                results.push((path.to_path_buf(), 0, "---".to_string()));
            }
            for (ci, ctx_line) in lines.iter().enumerate().take(ctx_end).skip(ctx_start) {
                if ci < last_ctx_end {
                    continue;
                }
                let prefix = if ci == idx { ">" } else { " " };
                results.push((
                    path.to_path_buf(),
                    ci + 1,
                    format!("{} {}", prefix, ctx_line),
                ));
                if results.len() >= opts.max_results {
                    return Ok(());
                }
            }
            last_ctx_end = ctx_end;
        } else {
            results.push((path.to_path_buf(), idx + 1, line.to_string()));
            if results.len() >= opts.max_results {
                return Ok(());
            }
        }
    }
    Ok(())
}
