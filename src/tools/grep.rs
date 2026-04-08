//! 在工作区内按正则/关键词搜索文件内容。
//!
//! 使用 `ignore` crate（ripgrep 同源）做 .gitignore 感知的文件遍历，
//! `regex` crate 做行级匹配，支持上下文行和 glob 过滤。

use ignore::WalkBuilder;
use regex::RegexBuilder;
use std::fs;
use std::io::Read;
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

    let file_glob_pat = params
        .file_glob
        .as_deref()
        .and_then(|g| glob::Pattern::new(g).ok());
    let exclude_glob_pat = params
        .exclude_glob
        .as_deref()
        .and_then(|g| glob::Pattern::new(g).ok());

    let mut results: Vec<(PathBuf, usize, String)> = Vec::new();
    let mut visited = 0usize;

    let walker = WalkBuilder::new(&root)
        .hidden(!params.ignore_hidden)
        .git_ignore(true)
        .git_global(false)
        .git_exclude(true)
        .build();

    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }
        let path = entry.path();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        if let Some(ref pat) = file_glob_pat
            && !pat.matches(&name)
        {
            continue;
        }
        if let Some(ref pat) = exclude_glob_pat
            && pat.matches(&name)
        {
            continue;
        }

        search_in_file(
            path,
            &re,
            &mut results,
            &mut visited,
            params.max_results,
            params.context_before,
            params.context_after,
        );
        if results.len() >= params.max_results {
            break;
        }
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
    let v: serde_json::Value = crate::tools::parse_args_json(args_json)?;
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

fn search_in_file(
    path: &Path,
    re: &regex::Regex,
    results: &mut Vec<(PathBuf, usize, String)>,
    visited_files: &mut usize,
    max_results: usize,
    ctx_before: usize,
    ctx_after: usize,
) {
    *visited_files += 1;
    let mut f = match fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return,
    };
    let mut buf = String::new();
    if f.read_to_string(&mut buf).is_err() {
        return;
    }
    if buf.len() > MAX_FILE_SIZE_BYTES {
        buf.truncate(MAX_FILE_SIZE_BYTES);
    }
    let lines: Vec<&str> = buf.lines().collect();
    let has_context = ctx_before > 0 || ctx_after > 0;
    let mut last_ctx_end: usize = 0;

    for (idx, line) in lines.iter().enumerate() {
        if !re.is_match(line) {
            continue;
        }
        if has_context {
            let ctx_start = idx.saturating_sub(ctx_before);
            let ctx_end = (idx + ctx_after + 1).min(lines.len());
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
                if results.len() >= max_results {
                    return;
                }
            }
            last_ctx_end = ctx_end;
        } else {
            results.push((path.to_path_buf(), idx + 1, line.to_string()));
            if results.len() >= max_results {
                return;
            }
        }
    }
}
