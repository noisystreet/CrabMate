//! 代码理解与导航：符号引用搜索、Rust 单文件结构大纲。

use crate::tools::symbol;
use regex::Regex;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

const MAX_FILE_SIZE_BYTES: usize = 2 * 1024 * 1024;
const DEFAULT_MAX_RESULTS: usize = 80;
const MAX_RESULTS_LIMIT: usize = 300;
const DEFAULT_MAX_OUTLINE_ITEMS: usize = 200;

/// 在工作区内搜索某标识符的「引用」（基于词边界；默认跳过疑似定义行）。
pub fn find_references(args_json: &str, workspace_root: &Path) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let symbol = v
        .get("symbol")
        .and_then(|s| s.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let Some(symbol) = symbol else {
        return "错误：缺少 symbol 参数".to_string();
    };

    let sub_path = v
        .get("path")
        .and_then(|p| p.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let max_results = v
        .get("max_results")
        .and_then(|n| n.as_u64())
        .map(|n| n.max(1) as usize)
        .unwrap_or(DEFAULT_MAX_RESULTS)
        .min(MAX_RESULTS_LIMIT);

    // 与 JSON schema 一致：case_sensitive 默认 false 表示忽略大小写
    let case_insensitive = !v
        .get("case_sensitive")
        .and_then(|b| b.as_bool())
        .unwrap_or(false);

    let exclude_definitions = v
        .get("exclude_definitions")
        .and_then(|b| b.as_bool())
        .unwrap_or(true);

    let include_hidden = v
        .get("include_hidden")
        .and_then(|b| b.as_bool())
        .unwrap_or(false);

    let root = match resolve_root(workspace_root, sub_path.as_deref()) {
        Ok(p) => p,
        Err(e) => return e,
    };

    let esc = regex::escape(&symbol);
    let ref_pat = format!(r"\b{}\b", esc);
    let ref_re = match regex::RegexBuilder::new(&ref_pat)
        .case_insensitive(case_insensitive)
        .build()
    {
        Ok(r) => r,
        Err(e) => return format!("错误：无法构建引用搜索规则：{}", e),
    };

    let mut results: Vec<(PathBuf, usize, String)> = Vec::new();
    let mut visited = 0usize;
    if let Err(e) = walk_rs(
        &root,
        &ref_re,
        &symbol,
        exclude_definitions,
        case_insensitive,
        &mut results,
        &mut visited,
        max_results,
        include_hidden,
    ) {
        return format!("搜索过程中发生错误：{}", e);
    }

    if results.is_empty() {
        return format!(
            "未找到引用：\"{}\"（遍历 {} 个 .rs 文件，根：{}）",
            symbol,
            visited,
            root.display()
        );
    }

    let mut out = String::new();
    out.push_str(&format!(
        "引用搜索：\"{}\"，根目录：{}\n（词边界匹配；exclude_definitions={}）\n匹配 {} 条（上限 {}）：\n\n",
        symbol,
        root.display(),
        exclude_definitions,
        results.len(),
        max_results
    ));
    for (path, line_no, line) in results {
        out.push_str(&format!(
            "{}:{}: {}\n",
            path.display(),
            line_no,
            truncate_line(&line, 320)
        ));
    }
    out.trim_end().to_string()
}

/// 列出单个 Rust 源文件中的模块级结构（fn/mod/struct/enum/trait/impl 等行摘要）。
pub fn rust_file_outline(args_json: &str, workspace_root: &Path) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let path = v
        .get("path")
        .and_then(|p| p.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let Some(rel) = path else {
        return "错误：缺少 path 参数".to_string();
    };

    let include_use = v
        .get("include_use")
        .and_then(|b| b.as_bool())
        .unwrap_or(false);

    let max_items = v
        .get("max_items")
        .and_then(|n| n.as_u64())
        .map(|n| n.max(1) as usize)
        .unwrap_or(DEFAULT_MAX_OUTLINE_ITEMS)
        .min(500);

    let target = match resolve_file(workspace_root, rel) {
        Ok(p) => p,
        Err(e) => return e,
    };
    if !target.is_file() {
        return "错误：path 必须是已存在的文件".to_string();
    }
    if target.extension().and_then(|e| e.to_str()) != Some("rs") {
        return "错误：当前仅支持 .rs 文件大纲".to_string();
    }

    let mut buf = String::new();
    if let Err(e) = fs::File::open(&target).and_then(|mut f| f.read_to_string(&mut buf)) {
        return format!("读取文件失败：{}", e);
    }
    if buf.len() > MAX_FILE_SIZE_BYTES {
        buf.truncate(MAX_FILE_SIZE_BYTES);
    }

    let items = collect_outline(&buf, include_use, max_items);
    if items.is_empty() {
        return format!(
            "文件 {} 中未匹配到常见顶层结构（可尝试 include_use=true）",
            rel
        );
    }

    let mut out = String::new();
    out.push_str(&format!(
        "Rust 文件大纲：{}（{} 项，最多 {}）\n\n",
        rel,
        items.len(),
        max_items
    ));
    for (line_no, summary) in items {
        out.push_str(&format!("{:>5}: {}\n", line_no, summary));
    }
    out.trim_end().to_string()
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

fn resolve_file(base: &Path, sub: &str) -> Result<PathBuf, String> {
    let sub_path = Path::new(sub);
    if sub_path.is_absolute() {
        return Err("路径必须为工作区内相对路径，不能使用绝对路径".to_string());
    }
    if sub.contains("..") {
        return Err("路径不能包含 ..".to_string());
    }
    let joined = base.join(sub_path);
    let canon_base = base
        .canonicalize()
        .map_err(|e| format!("工作区根目录无法解析: {}", e))?;
    let canonical = joined
        .canonicalize()
        .map_err(|e| format!("目标文件路径无法解析: {}", e))?;
    if !canonical.starts_with(&canon_base) {
        return Err("目标路径不能超出工作区根目录".to_string());
    }
    Ok(canonical)
}

#[allow(clippy::too_many_arguments)]
fn walk_rs(
    root: &Path,
    ref_re: &Regex,
    symbol: &str,
    exclude_definitions: bool,
    case_insensitive: bool,
    results: &mut Vec<(PathBuf, usize, String)>,
    visited_files: &mut usize,
    max_results: usize,
    ignore_hidden_dirs: bool,
) -> Result<(), String> {
    if !root.exists() {
        return Ok(());
    }
    if root.is_file() {
        if root.extension().and_then(|e| e.to_str()) != Some("rs") {
            return Ok(());
        }
        *visited_files += 1;
        search_refs_in_file(
            root,
            ref_re,
            symbol,
            exclude_definitions,
            case_insensitive,
            results,
            max_results,
        )?;
        return Ok(());
    }
    for entry in fs::read_dir(root).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if ignore_hidden_dirs && name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            walk_rs(
                &path,
                ref_re,
                symbol,
                exclude_definitions,
                case_insensitive,
                results,
                visited_files,
                max_results,
                ignore_hidden_dirs,
            )?;
            if results.len() >= max_results {
                break;
            }
        } else if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("rs") {
            *visited_files += 1;
            search_refs_in_file(
                &path,
                ref_re,
                symbol,
                exclude_definitions,
                case_insensitive,
                results,
                max_results,
            )?;
            if results.len() >= max_results {
                break;
            }
        }
    }
    Ok(())
}

fn search_refs_in_file(
    path: &Path,
    ref_re: &Regex,
    symbol: &str,
    exclude_definitions: bool,
    case_insensitive: bool,
    results: &mut Vec<(PathBuf, usize, String)>,
    max_results: usize,
) -> Result<(), String> {
    let mut buf = String::new();
    let mut f = fs::File::open(path).map_err(|e| e.to_string())?;
    f.read_to_string(&mut buf).map_err(|e| e.to_string())?;
    if buf.len() > MAX_FILE_SIZE_BYTES {
        buf.truncate(MAX_FILE_SIZE_BYTES);
    }
    for (idx, line) in buf.lines().enumerate() {
        if !ref_re.is_match(line) {
            continue;
        }
        if exclude_definitions
            && symbol::line_looks_like_rust_definition(line, symbol, case_insensitive)
        {
            continue;
        }
        results.push((path.to_path_buf(), idx + 1, line.to_string()));
        if results.len() >= max_results {
            break;
        }
    }
    Ok(())
}

fn collect_outline(content: &str, include_use: bool, max_items: usize) -> Vec<(usize, String)> {
    // 预编译常见顶层结构（非完整语法树，仅辅助导航）。
    let re_mod = Regex::new(r"^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\b").unwrap();
    let re_fn = Regex::new(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+(\w+)\b").unwrap();
    let re_struct = Regex::new(r"^\s*(?:pub(?:\([^)]*\))?\s+)?struct\s+(\w+)\b").unwrap();
    let re_enum = Regex::new(r"^\s*(?:pub(?:\([^)]*\))?\s+)?enum\s+(\w+)\b").unwrap();
    let re_trait = Regex::new(r"^\s*(?:pub(?:\([^)]*\))?\s+)?trait\s+(\w+)\b").unwrap();
    let re_type = Regex::new(r"^\s*(?:pub(?:\([^)]*\))?\s+)?type\s+(\w+)\b").unwrap();
    let re_const = Regex::new(r"^\s*(?:pub(?:\([^)]*\))?\s+)?const\s+(\w+)\b").unwrap();
    let re_static = Regex::new(r"^\s*(?:pub(?:\([^)]*\))?\s+)?static\s+(\w+)\b").unwrap();
    let re_macro = Regex::new(r"^\s*macro_rules!\s+(\w+)\b").unwrap();
    let re_impl = Regex::new(r"^\s*(?:unsafe\s+)?impl\b").unwrap();
    let re_use = Regex::new(r"^\s*(?:pub(?:\([^)]*\))?\s+)?use\s+").unwrap();

    let mut out = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        if out.len() >= max_items {
            break;
        }
        let line_no = idx + 1;
        let t = line.trim_start();
        if t.starts_with("//") || t.starts_with("//!") {
            continue;
        }
        if let Some(c) = re_mod.captures(line) {
            out.push((line_no, format!("mod {}", &c[1])));
            continue;
        }
        if re_fn.is_match(line) {
            out.push((line_no, truncate_one_line(line, 100)));
            continue;
        }
        if let Some(c) = re_struct.captures(line) {
            out.push((line_no, format!("struct {}", &c[1])));
            continue;
        }
        if let Some(c) = re_enum.captures(line) {
            out.push((line_no, format!("enum {}", &c[1])));
            continue;
        }
        if let Some(c) = re_trait.captures(line) {
            out.push((line_no, format!("trait {}", &c[1])));
            continue;
        }
        if re_type.is_match(line) {
            out.push((line_no, truncate_one_line(line, 100)));
            continue;
        }
        if let Some(c) = re_const.captures(line) {
            out.push((line_no, format!("const {}", &c[1])));
            continue;
        }
        if let Some(c) = re_static.captures(line) {
            out.push((line_no, format!("static {}", &c[1])));
            continue;
        }
        if let Some(c) = re_macro.captures(line) {
            out.push((line_no, format!("macro_rules! {}", &c[1])));
            continue;
        }
        if re_impl.is_match(line) {
            out.push((line_no, truncate_one_line(line, 120)));
            continue;
        }
        if include_use && re_use.is_match(line) {
            out.push((line_no, truncate_one_line(line, 100)));
        }
    }
    out
}

fn truncate_one_line(s: &str, max: usize) -> String {
    let t = s.trim_end();
    let mut out: String = t.chars().take(max).collect();
    if t.chars().count() > max {
        out.push('…');
    }
    out
}

fn truncate_line(s: &str, max_chars: usize) -> String {
    let s = s.trim_end();
    let mut chars = s.chars();
    let mut out = String::new();
    for _ in 0..max_chars {
        if let Some(ch) = chars.next() {
            out.push(ch);
        } else {
            return out;
        }
    }
    if s.chars().count() > max_chars {
        format!("{}... (截断)", out)
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outline_finds_fn_and_mod() {
        let src = r"
mod foo {
}
pub fn bar(x: u32) -> u32 { x }
";
        let v = collect_outline(src, false, 50);
        assert!(v.iter().any(|(_, s)| s.contains("mod foo")));
        assert!(v.iter().any(|(_, s)| s.contains("fn bar")));
    }

    #[test]
    fn outline_respects_max_items() {
        let mut s = String::new();
        for i in 0..30 {
            s.push_str(&format!("fn f{i}() {{}}\n"));
        }
        let v = collect_outline(&s, false, 5);
        assert_eq!(v.len(), 5);
    }
}
